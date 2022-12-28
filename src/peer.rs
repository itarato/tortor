use std::error::Error;
use std::io::{Read, Write};
use std::{net::TcpStream, sync::Arc};

use num_traits::{FromPrimitive, ToPrimitive};

use crate::defs::*;
use crate::macros::*;
use crate::IP;

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
}

impl Peer {
    pub fn new(ip: IP, info_hash: Arc<Vec<u8>>) -> Self {
        Self {
            ip,
            info_hash,
            is_interested: false,
            is_choked: true,
        }
    }

    pub fn exec(&mut self) {
        let handshake_res = self.handshake();

        match handshake_res {
            Ok(PeerMessage::KeepAlive) => {
                if self.is_choked {
                    let choke_res = self.unchoke();
                } else {
                    unimplemented!("Handshake is keep-alive but peer is already unchoked.")
                }
            }
            Err(err) => {
                log::error!("Handshake error: {:?}", err);
            }
            _ => unimplemented!("Handshake response not handled yet"),
        };
    }

    pub fn handshake(&mut self) -> Result<PeerMessage, Box<dyn Error>> {
        let mut stream = TcpStream::connect(self.ip.socket_addr())?;

        stream.set_ttl(3).expect("Cannot set TCP IP TTL");

        let mut buf: Vec<u8> = vec![];
        let pstr = b"BitTorrent protocol";

        to_buf!(pstr.len() as u8, buf);
        vec_to_buf!(pstr, buf);
        vec_to_buf!(&[0u8; 8][..], buf);
        vec_to_buf!(&self.info_hash[..], buf);
        vec_to_buf!(&PEER_ID[..], buf);
        assert_eq!(49 + pstr.len(), buf.len());

        log::info!("Handshake init with: {:?}", self.ip);
        stream.write(&buf[..]).expect("Cannot send handshake");
        log::info!("Handshake sent");

        let mut response_buf: [u8; 1024] = [0; 1024];
        let response_len = stream.read(&mut response_buf)?;

        log::info!("Handshake reponse: {} bytes", response_len);
        dbg!(&response_buf[..32]);

        if response_len == 0 {
            return Ok(PeerMessage::KeepAlive);
        }

        let peer_msg_code: PeerMessageCode = response_buf[..]
            .try_into()
            .map_err(|_| "No bytes for peer message code")?;
        match peer_msg_code {
            _ => unimplemented!(
                "Handshake response is not handled yet: {:?}",
                &peer_msg_code
            ),
        }
    }

    pub fn unchoke(&mut self) -> Result<PeerMessage, Box<dyn Error>> {
        let mut stream = TcpStream::connect(self.ip.socket_addr())?;

        stream.set_ttl(3).expect("Cannot set TCP IP TTL");

        let mut buf: Vec<u8> = vec![];

        to_buf!(1u32, buf); // Len.
        to_buf!(1u8 as u8, buf); // Unchoke.
        assert_eq!(5, buf.len());

        log::info!("Unchoke init with: {:?}", self.ip);
        stream.write(&buf[..]).expect("Cannot send handshake");
        log::info!("Unchoke sent");

        let mut response_buf: [u8; 1024] = [0; 1024];
        let response_len = stream.read(&mut response_buf)?;

        log::info!("Unchoke reponse: {} bytes", response_len);
        dbg!(&response_buf[..32]);

        if response_len == 0 {
            return Ok(PeerMessage::KeepAlive);
        }

        let peer_msg_code: PeerMessageCode = response_buf[..]
            .try_into()
            .map_err(|_| "No bytes for peer message code")?;
        match peer_msg_code {
            _ => unimplemented!(
                "Handshake response is not handled yet: {:?}",
                &peer_msg_code
            ),
        }
    }
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
