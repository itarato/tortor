use std::borrow::BorrowMut;
use std::io::{self, Read};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use num_traits::FromPrimitive;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::defs::*;
use crate::macros::*;
use crate::IP;

const HANDSHAKE_PSTR: &'static [u8; 19] = b"BitTorrent protocol";
const MSG_BUF_SIZE: usize = 1024;

#[derive(Debug)]
struct MessageBuf {
    buf: Vec<u8>,
    start: usize,
    end: usize,
    min_available: usize,
}

/**
 * [=======X-------X        ]
 *    start^       ^end     ^len
 *         <current>
 *                  <buffer>
 */
impl MessageBuf {
    pub fn new_with_min_available(min_available: usize) -> Self {
        Self {
            buf: vec![0; min_available],
            start: 0,
            end: 0,
            min_available,
        }
    }

    pub fn buffer(&mut self) -> &mut [u8] {
        &mut self.buf[self.end..]
    }

    pub fn current(&self) -> &[u8] {
        &self.buf[self.start..self.end]
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.start = 0;
        self.end = 0;
    }

    fn set_start(&mut self, pos: usize) {
        self.start = pos;
    }

    fn register_read_len(&mut self, read_len: usize) {
        self.end += read_len;
    }

    fn ensure_read_capacity(&mut self) {
        if self.start == self.end && self.start > 0 {
            self.clear();
            return;
        }

        if self.start + self.min_available <= self.buf.len() {
            return;
        }

        let additional_space = 0usize.max(self.start + self.min_available - self.buf.len());
        self.buf.reserve(additional_space);
        self.buf.resize(self.buf.capacity(), 0);

        log::info!(
            "Reserved {} more bytes in read buffer (now {})",
            additional_space,
            self.buf.len()
        );
    }
}

#[derive(FromPrimitive, ToPrimitive, Debug)]
enum PeerMessageCode {
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have,
    Bitfield,
    Request,
    Piece,
    Cancel,
    Port,
}

impl TryFrom<&[u8]> for PeerMessageCode {
    type Error = ();

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() < 5 {
            log::error!("No 5 bytes");
            return Err(());
        }

        let b = u8::from_be_bytes(value[4..5].try_into().unwrap());
        FromPrimitive::from_u8(b).ok_or(())
    }
}

pub struct Peer {
    idx: usize,
    ip: IP,
    info_hash: Arc<Vec<u8>>,
    is_interested: bool,
    is_choked: bool,
    stream: TcpStream,
    read_buffer: MessageBuf,
}

impl Peer {
    pub async fn try_new(idx: usize, ip: IP, info_hash: Arc<Vec<u8>>) -> Result<Self, io::Error> {
        TcpStream::connect(ip.socket_addr())
            .await
            .map(|stream| Self {
                idx,
                ip,
                info_hash,
                is_interested: false,
                is_choked: true,
                stream,
                read_buffer: MessageBuf::new_with_min_available(MSG_BUF_SIZE),
            })
    }

    pub async fn exec(&mut self) {
        // let listener = TcpListener::bind(
        //     self.stream
        //         .local_addr()
        //         .expect("Cannot obtain the local stream addr"),
        // )
        // .await
        // .expect("Cannot establish local TCP listener");

        log::info!("Peer listens on {:?}", self.stream.local_addr());

        let handshake_res = self.handshake().await;
        if handshake_res.is_err() {
            log::error!("Handshake error: {:?}", handshake_res);
            return;
        }

        if !handshake_res.unwrap() {
            log::warn!("Incorrect handshake");
            return;
        }

        log::info!("Peer closing");
    }

    // async fn handle_incoming(&mut self, mut stream: TcpStream, addr: SocketAddr) {
    //     let mut buf: [u8; MSG_BUF_SIZE] = [0; MSG_BUF_SIZE];

    //     match stream.read(&mut buf).await {
    //         Ok(size) => {
    //             log::info!("Incoming peer msg: {} bytes", size);
    //             dbg!(&buf[..32]);
    //         }
    //         Err(err) => {
    //             log::error!("Incoming peer stream failure: {:?}", err);
    //         }
    //     };
    // }

    pub async fn handshake(&mut self) -> Result<bool, io::Error> {
        let mut buf: Vec<u8> = vec![];

        to_buf!(HANDSHAKE_PSTR.len() as u8, buf);
        vec_to_buf!(HANDSHAKE_PSTR, buf);
        vec_to_buf!(&[0u8; 8][..], buf);
        vec_to_buf!(&self.info_hash[..], buf);
        vec_to_buf!(&PEER_ID[..], buf);

        let msg_len = 49 + HANDSHAKE_PSTR.len();

        assert_eq!(msg_len, buf.len());

        log::info!("Handshake init with: {:?}", self.ip);
        self.stream.write_all(&buf[..]).await?;
        log::info!("Handshake sent");

        self.read_one_message();

        log::info!("Handshake waiting for response");
        let resp_buf = self.read_buffer.current();

        log::info!("Got handshake response");

        if resp_buf.len() < msg_len {
            log::warn!("Handshake response is too small: {}", msg_len);
            return Ok(false);
        }

        if resp_buf[0] != HANDSHAKE_PSTR.len() as u8 {
            log::warn!("Handshake response PSTR len is incorrect: {}", resp_buf[0]);
            return Ok(false);
        }

        if resp_buf[1..1 + HANDSHAKE_PSTR.len()] != buf[1..1 + HANDSHAKE_PSTR.len()] {
            log::warn!("Handske PSTR is not matching");
            return Ok(false);
        }

        if resp_buf[28..48] != buf[28..48] {
            log::warn!("Handske info hash is not matching");
            return Ok(false);
        }

        Ok(true)
    }

    async fn read_one_message(&mut self) -> Result<(), io::Error> {
        loop {
            if self.buffer_has_full_message() {
                break;
            }

            let read_len = self.stream.read(&mut self.read_buffer.buffer()).await?;
            self.read_buffer.register_read_len(read_len);
        }

        Ok(())
    }

    fn buffer_has_full_message(&self) -> bool {
        let current = self.read_buffer.current();
        if current.len() < 4
        /* u32 len + u8 action */
        {
            return false;
        }

        let msg_len = u32::from_be_bytes(current[..4].try_into().unwrap()) as usize;
        current.len() >= msg_len
    }

    // pub fn unchoke(&mut self) -> Result<PeerMessage, Box<dyn Error>> {
    //     let mut buf: Vec<u8> = vec![];

    //     to_buf!(1u32, buf); // Len.
    //     to_buf!(1u8 as u8, buf); // Unchoke.
    //     assert_eq!(5, buf.len());

    //     log::info!("Unchoke init with: {:?}", self.ip);
    //     self.stream.write(&buf[..]).expect("Cannot send unchoke");
    //     log::info!("Unchoke sent");

    //     let mut response_buf: [u8; 1024] = [0; 1024];
    //     let response_len = stream.read(&mut response_buf)?;

    //     log::info!("Unchoke reponse: {} bytes", response_len);
    //     dbg!(&response_buf[..32]);

    //     if response_len == 0 {
    //         return Ok(PeerMessage::KeepAlive);
    //     }

    //     let peer_msg_code: PeerMessageCode = response_buf[..]
    //         .try_into()
    //         .map_err(|_| "No bytes for peer message code")?;
    //     match peer_msg_code {
    //         _ => unimplemented!("Unchoke response is not handled yet: {:?}", &peer_msg_code),
    //     }
    // }

    // pub fn interested(&mut self) -> Result<PeerMessage, Box<dyn Error>> {
    //     let mut stream = TcpStream::connect(self.ip.socket_addr())?;

    //     stream.set_ttl(3).expect("Cannot set TCP IP TTL");

    //     let mut buf: Vec<u8> = vec![];

    //     to_buf!(1u32, buf); // Len.
    //     to_buf!(2u8 as u8, buf); // Interested.
    //     assert_eq!(5, buf.len());

    //     log::info!("Interested init with: {:?}", self.ip);
    //     stream.write(&buf[..]).expect("Cannot send interested");
    //     log::info!("Interested sent");

    //     let mut response_buf: [u8; 1024] = [0; 1024];
    //     let response_len = stream.read(&mut response_buf)?;

    //     log::info!("Interested reponse: {} bytes", response_len);
    //     dbg!(&response_buf[..32]);

    //     if response_len == 0 {
    //         return Ok(PeerMessage::KeepAlive);
    //     }

    //     let peer_msg_code: PeerMessageCode = response_buf[..]
    //         .try_into()
    //         .map_err(|_| "No bytes for peer message code")?;
    //     match peer_msg_code {
    //         _ => unimplemented!(
    //             "Interested response is not handled yet: {:?}",
    //             &peer_msg_code
    //         ),
    //     }
    // }
}

pub enum PeerMessage {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have,
    Bitfield,
    Request,
    Piece,
    Cancel,
    Port,
}
