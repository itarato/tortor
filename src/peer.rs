use std::io::{Write, Read};
use std::{sync::Arc, net::TcpStream};

use crate::IP;
use crate::macros::*;
use crate::defs::*;

enum PeerMessage {
  Choke,
  Unchoke,
  Interested,
  NotInterested,
  Have,
  Bitfield,
  Request,
  Piece,
  Cancel,
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

  }

  pub fn handshake(&mut self) {
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
  }
}