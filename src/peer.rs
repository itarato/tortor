use std::io::{Write, Read};
use std::{sync::Arc, net::TcpStream};

use num_traits::FromPrimitive;

use crate::IP;
use crate::macros::*;
use crate::defs::*;

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
      let b = u32::from_be_bytes(value[0..4].try_into().expect("Has 4 bytes"));
      FromPrimitive::from_u32(b).ok_or(())
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

  pub fn exec(&mut self) -> Result<(), ()> {
    let handshake_res = self.handshake()?;

    match handshake_res {
      PeerMessage::KeepAlive => {
        if self.is_choked {
          unimplemented!("Do this")
        } else {
          unimplemented!("Handshake is keep-alive but peer is already unchoked.")
        }
      }
      _ => unimplemented!("Handshake response not handled yet")
    };

    Ok(())
  }

  pub fn handshake(&mut self) -> Result<PeerMessage, ()> {
    let mut stream = TcpStream::connect(self.ip.socket_addr())
        .expect("Cannot reserve TCP stream");

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
    let response_len = stream
        .read(&mut response_buf)
        .expect("Failed getting handshake response");

    log::info!("Handshake reponse: {} bytes", response_len);
    dbg!(&response_buf[..32]);

    if response_len == 0 {
      return Ok(PeerMessage::KeepAlive);
    }

    let peer_msg_code: PeerMessageCode = response_buf[..].try_into()?;
    match peer_msg_code {
      _ => unimplemented!("Handshake response is not handled yet: {:?}", &peer_msg_code)
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