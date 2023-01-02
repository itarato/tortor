use std::error::Error;
use std::io;
use std::sync::Arc;

use num_traits::FromPrimitive;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;

use crate::defs::*;
use crate::macros::*;
use crate::IP;

const HANDSHAKE_PSTR: &'static [u8; 19] = b"BitTorrent protocol";
const PIECE_SIZE: usize = 1 << 14;
const MSG_BUF_SIZE: usize = PIECE_SIZE + 1024; // Some extra buffer in case multiple messages are coming in.

#[derive(Debug)]
pub struct MessageBuf {
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

    fn consume_message(&mut self, delta: usize) {
        assert!(
            delta <= self.end - self.start,
            "Consumed more than available"
        );
        self.start += delta;
    }

    fn register_read_len(&mut self, read_len: usize) {
        self.end += read_len;
    }

    pub fn ensure_read_capacity(&mut self) {
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

enum PeerState {
    // Need to initiate handshake.
    NeedHandshake,
    // Need to unchoke itself.
    Choked,
    // Ready to send out requests for pieces and receive them.
    Pulling,
    // Completed.
    Dead,
}

pub struct Peer {
    idx: usize,
    ip: IP,
    info_hash: Arc<Vec<u8>>,
}

impl Peer {
    pub fn new(idx: usize, ip: IP, info_hash: Arc<Vec<u8>>) -> Self {
        Self { idx, ip, info_hash }
    }

    pub async fn exec(&self) {
        let mut state = PeerState::NeedHandshake;

        let mut stream = match TcpStream::connect(self.ip.socket_addr()).await {
            Ok(stream) => stream,
            Err(err) => {
                log::error!("{} Peer connection failed: {:?}", self.idx, err);
                return;
            }
        };

        let mut read_buffer = MessageBuf::new_with_min_available(MSG_BUF_SIZE);

        if let Err(err) = self.handshake(&mut stream, &mut read_buffer).await {
            log::error!("Handshake error: {:?}", err);
            return;
        }

        state = PeerState::Choked;

        let (mut reader, mut writer) = stream.split();

        let writer_task = async move {
            if Peer::interested(&mut writer).await.is_err() {
                log::warn!("{} Failed sending interested message", self.idx);
                return;
            }

            loop {
                match state {
                    PeerState::NeedHandshake => panic!("Unexpected state"),
                    PeerState::Dead => break,
                    PeerState::Pulling => {
                        if Peer::request(&mut writer).await.is_err() {
                            log::error!("{} Request message error", self.idx);
                        }
                        return;
                    }
                    PeerState::Choked => tokio::task::yield_now().await,
                };
            }
        };

        let reader_task = async move {
            loop {
                log::info!("{} Listening for incoming stream", self.idx);
                // let res = self.read_one_message().await;

                // if res.is_ok() {
                //     let last_msg = self.take_last_message();
                //     log::info!("Got post-handshake message: {:?}", last_msg);

                //     match last_msg {
                //         Some(PeerMessage::Bitfield(bitfields)) => {
                //             // FIXME: use bitfields to know what is available
                //             log::info!("{} Bitfields are in", self.idx);

                //             match self.request().await {
                //                 Ok(_) => log::info!("{} Got a piece", self.idx),
                //                 Err(err) => {
                //                     log::error!("{} Error during piece fetch: {}", self.idx, err)
                //                 }
                //             }
                //         }
                //         None => {
                //             log::warn!("{} No post handshake message", self.idx);
                //         }
                //         _ => unimplemented!("Unhandled message: {:?}", last_msg),
                //     }
                // } else {
                //     log::warn!("Failed getting post-handshake message. Quitting work.");
                //     return;
                // }

                // match state {
                //     PeerState::NeedMakeHandshake => panic!("Unexpected state"),
                //     PeerState::NeedReceiveHandshake => panic!("Unexpected state"),
                //     PeerState::Dead => break,
                //     PeerState::Choked
                // }
            }
        };

        tokio::select! {
            _ = writer_task => {}
            _ = reader_task => {}
        };

        log::info!("Peer closing");
    }

    pub async fn handshake(
        &self,
        stream: &mut TcpStream,
        read_buffer: &mut MessageBuf,
    ) -> Result<(), Box<dyn Error>> {
        let mut buf: Vec<u8> = vec![];

        to_buf!(HANDSHAKE_PSTR.len() as u8, buf);
        vec_to_buf!(HANDSHAKE_PSTR, buf);
        vec_to_buf!(&[0u8; 8][..], buf);
        vec_to_buf!(&self.info_hash[..], buf);
        vec_to_buf!(&PEER_ID[..], buf);

        assert_eq!(self.handhake_len(), buf.len());

        log::info!("Handshake init with: {:?}", self.ip);
        stream.write_all(&buf[..]).await?;
        log::info!("Handshake sent");

        self.read_handshake(stream, read_buffer).await?;

        log::info!("Handshake waiting for response");
        let resp_buf = read_buffer.current();

        log::info!("Got handshake response");

        if resp_buf.len() < self.handhake_len() {
            log::warn!(
                "Handshake response is incorrect: {} != {}",
                resp_buf.len(),
                self.handhake_len()
            );
            return Err("Incorrect handshake response".into());
        }

        if resp_buf[0] != HANDSHAKE_PSTR.len() as u8 {
            log::warn!("Handshake response PSTR len is incorrect: {}", resp_buf[0]);
            return Err("Incorrect handshake response".into());
        }

        if resp_buf[1..1 + HANDSHAKE_PSTR.len()] != buf[1..1 + HANDSHAKE_PSTR.len()] {
            log::warn!("Handshake PSTR is not matching");
            return Err("Incorrect handshake response".into());
        }

        if resp_buf[28..48] != buf[28..48] {
            log::warn!("Handshake info hash is not matching");
            return Err("Incorrect handshake response".into());
        }

        read_buffer.consume_message(self.handhake_len());

        Ok(())
    }

    fn handhake_len(&self) -> usize {
        49 + HANDSHAKE_PSTR.len()
    }

    async fn read_one_message(
        &mut self,
        stream: &mut ReadHalf<'_>,
        read_buffer: &mut MessageBuf,
    ) -> Result<(), io::Error> {
        read_buffer.ensure_read_capacity();

        loop {
            if self.buffer_has_full_message(read_buffer) {
                log::info!("{} got full message", self.idx);
                break;
            }

            log::info!(
                "{} Waiting read into {} byte buf",
                self.idx,
                read_buffer.buffer().len()
            );
            let read_len = stream.read(&mut read_buffer.buffer()).await?;
            read_buffer.register_read_len(read_len);
            log::info!("{} Read {} bytes", self.idx, read_len);

            if read_buffer.current().len() == 0 && read_len == 0 {
                log::info!("{} Got Keep-Alive", self.idx);
                break;
            }
        }

        Ok(())
    }

    async fn read_handshake(
        &self,
        stream: &mut TcpStream,
        read_buffer: &mut MessageBuf,
    ) -> Result<(), io::Error> {
        read_buffer.ensure_read_capacity();

        loop {
            if read_buffer.current().len() >= self.handhake_len() {
                log::info!("{} got full handshake", self.idx);
                break;
            }

            log::info!(
                "{} Waiting read into {} byte buf",
                self.idx,
                read_buffer.buffer().len()
            );
            let read_len = stream.read(&mut read_buffer.buffer()).await?;
            read_buffer.register_read_len(read_len);
            log::info!("{} Read {} bytes", self.idx, read_len);

            if read_buffer.current().len() == 0 && read_len == 0 {
                log::info!("{} Got Keep-Alive", self.idx);
                break;
            }
        }

        Ok(())
    }

    fn buffer_has_full_message(&self, read_buffer: &mut MessageBuf) -> bool {
        let current = read_buffer.current();
        if current.len() < 4
        /* u32 len + u8 action */
        {
            log::info!("{} has {} bytes only", self.idx, current.len());
            return false;
        }

        let msg_len = u32::from_be_bytes(current[..4].try_into().unwrap()) as usize;
        log::info!(
            "{} Msg is {} bytes and we got {} already",
            self.idx,
            msg_len,
            current.len()
        );
        current.len() >= msg_len + 4
    }

    fn take_last_message(&mut self, read_buffer: &mut MessageBuf) -> Option<PeerMessage> {
        let current = read_buffer.current();

        if current.len() == 0 {
            return Some(PeerMessage::KeepAlive);
        }

        if current.len() < 5 {
            log::warn!("{} Invalid message len: {}", self.idx, current.len());
            return None;
        }

        let msg_len = u32::from_be_bytes(current[..4].try_into().unwrap()) as usize;
        let message_raw = &current[4..msg_len + 4];
        assert_eq!(msg_len, message_raw.len());

        match message_raw[0] {
            0 => {
                read_buffer.consume_message(5);
                Some(PeerMessage::Choke)
            }
            1 => {
                read_buffer.consume_message(5);
                Some(PeerMessage::Unchoke)
            }
            2 => {
                read_buffer.consume_message(5);
                Some(PeerMessage::Interested)
            }
            3 => {
                read_buffer.consume_message(5);
                Some(PeerMessage::NotInterested)
            }
            4 => {
                let piece = u32::from_be_bytes(message_raw[1..5].try_into().unwrap());
                read_buffer.consume_message(9);
                Some(PeerMessage::Have(piece))
            }
            5 => {
                let bitfields = message_raw[1..msg_len - 1].to_vec();
                Some(PeerMessage::Bitfield(bitfields))
            }
            6 => {
                let index = u32::from_be_bytes(message_raw[1..5].try_into().unwrap());
                let begin = u32::from_be_bytes(message_raw[5..9].try_into().unwrap());
                let length = u32::from_be_bytes(message_raw[9..13].try_into().unwrap());
                Some(PeerMessage::Request(index, begin, length))
            }
            7 => {
                unimplemented!("Piece message is not implemented")
                // Some(PeerMessage::Piece)
            }
            8 => {
                let index = u32::from_be_bytes(message_raw[1..5].try_into().unwrap());
                let begin = u32::from_be_bytes(message_raw[5..9].try_into().unwrap());
                let length = u32::from_be_bytes(message_raw[9..13].try_into().unwrap());
                Some(PeerMessage::Cancel(index, begin, length))
            }
            9 => {
                let port = u16::from_be_bytes(message_raw[1..3].try_into().unwrap());
                Some(PeerMessage::Port(port))
            }
            _ => unimplemented!("Message type {} not handled yet", message_raw[0]),
        }
    }

    pub async fn request(stream: &mut WriteHalf<'_>) -> Result<(), io::Error> {
        let mut buf_out: Vec<u8> = vec![];

        to_buf!(13u32, buf_out);
        to_buf!(6u8, buf_out);
        to_buf!(0u32, buf_out);
        to_buf!(0u32, buf_out);
        to_buf!((1u32 << 14) as u32, buf_out);

        log::info!("Sent: request");
        stream.write_all(&mut buf_out).await?;

        Ok(())
    }

    pub async fn interested(stream: &mut WriteHalf<'_>) -> Result<(), io::Error> {
        let mut buf_out: Vec<u8> = vec![];

        to_buf!(1u32, buf_out);
        to_buf!(2u8, buf_out);

        log::info!("Sent: interested");
        stream.write_all(&mut buf_out).await?;

        Ok(())
    }
}

#[derive(Debug)]
pub enum PeerMessage {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have(u32),              // Piece index.
    Bitfield(Vec<u8>),      // Bitfields.
    Request(u32, u32, u32), // Index, begin, length.
    Piece,
    Cancel(u32, u32, u32), // Index, begin, length.
    Port(u16),             // Port.
}
