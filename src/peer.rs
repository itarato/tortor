use std::error::Error;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use num_traits::{FromPrimitive, ToPrimitive};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::defs::*;
use crate::macros::*;
use crate::IP;

const HANDSHAKE_PSTR: &'static [u8; 19] = b"BitTorrent protocol";
const MSG_BUF_SIZE: usize = 1024;

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
    ip: IP,
    info_hash: Arc<Vec<u8>>,
    is_interested: bool,
    is_choked: bool,
    stream: TcpStream,
}

impl Peer {
    pub async fn try_new(ip: IP, info_hash: Arc<Vec<u8>>) -> Result<Self, io::Error> {
        TcpStream::connect(ip.socket_addr())
            .await
            .map(|stream| Self {
                ip,
                info_hash,
                is_interested: false,
                is_choked: true,
                stream,
            })
    }

    pub async fn exec(&mut self) {
        let listener = TcpListener::bind(
            self.stream
                .local_addr()
                .expect("Cannot obtain the local stream addr"),
        )
        .await
        .expect("Cannot establish local TCP listener");

        log::info!("Peer listens on {:?}", self.stream.local_addr());

        match self.handshake().await {
            Ok(_) => loop {
                match listener.accept().await {
                    Ok((stream_res, addr)) => self.handle_incoming(stream_res, addr).await,
                    Err(err) => {
                        log::error!("TcpListener error: {:?}", err);
                        break;
                    }
                }
                break;
            },
            Err(err) => {
                log::error!("Handshake error: {:?}", err);
            }
        };

        log::info!("Peer closing");
    }

    async fn handle_incoming(&mut self, mut stream: TcpStream, addr: SocketAddr) {
        let mut buf: [u8; MSG_BUF_SIZE] = [0; MSG_BUF_SIZE];

        match stream.read(&mut buf).await {
            Ok(size) => {
                log::info!("Incoming peer msg: {} bytes", size);
                dbg!(&buf[..32]);
            }
            Err(err) => {
                log::error!("Incoming peer stream failure: {:?}", err);
            }
        };
    }

    pub async fn handshake(&mut self) -> Result<(), io::Error> {
        let mut buf: Vec<u8> = vec![];

        to_buf!(HANDSHAKE_PSTR.len() as u8, buf);
        vec_to_buf!(HANDSHAKE_PSTR, buf);
        vec_to_buf!(&[0u8; 8][..], buf);
        vec_to_buf!(&self.info_hash[..], buf);
        vec_to_buf!(&PEER_ID[..], buf);
        assert_eq!(49 + HANDSHAKE_PSTR.len(), buf.len());

        log::info!("Handshake init with: {:?}", self.ip);
        self.stream
            .write(&buf[..])
            .await
            .expect("Cannot send handshake");
        log::info!("Handshake sent");

        // self.stream.set_read_timeout(Some(Duration::new(3, 0)))?;

        let mut resp_buf: Vec<u8> = vec![];

        log::info!("Handshake waiting for response");
        self.stream.read(&mut resp_buf).await?;

        log::info!("Got handshake response");
        dbg!(resp_buf);

        Ok(())
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
