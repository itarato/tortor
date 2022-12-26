mod ben_type;
mod byte_reader;
mod stable_hash_map;

use byte_reader::ByteReader;
use clap::Parser;
use rand::Rng;
use sha1::{Digest, Sha1};
use simple_logger::SimpleLogger;
use std::{error::Error, fs::File, io::Read, net::SocketAddr};
use tokio::net::UdpSocket;

use crate::ben_type::*;

macro_rules! to_buf {
    ($e: expr, $i: ident) => {
        for b in $e.to_be_bytes() {
            $i.push(b);
        }
    };
}

macro_rules! vec_to_buf {
    ($e: expr, $i: ident) => {
        for b in $e {
            $i.push(*b);
        }
    };
}

static LOCAL_SOCKET_ADDR: &'static str = "0.0.0.0:6881";
static LOCAL_PORT: u16 = 6881;
static PEER_ID: &'static str = "M0-0-1--IT2022------";
static CONNECT_MAGIC_NUMBER: u64 = 0x41727101980;
static ACTION_CONNECT: u32 = 0;
static ACTION_ANNOUNCE: u32 = 1;
static ACTION_SCRAPE: u32 = 2;
static ACTION_ERROR: u32 = 3;
static ANNOUNCE_EVENT_NONE: u32 = 0;
static ANNOUNCE_EVENT_COMPLETED: u32 = 1;
static ANNOUNCE_EVENT_STARTED: u32 = 2;
static ANNOUNCE_EVENT_STOPPED: u32 = 3;
static MAX_PEERS: i32 = 8;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short = 'f')]
    filename: String,
}

fn action_code(buf: &[u8]) -> u32 {
    u32::from_be_bytes(buf[0..4].try_into().expect("Has 4 bytes"))
}

#[derive(Debug)]
struct AnnounceInfo {
    pieces: Vec<Vec<u8>>,
    pieces_len: usize,
    len: usize,
    name: String,
}

#[derive(Debug)]
struct ConnectResponse {
    transaction_id: u32,
    connection_id: u64,
}

impl From<&[u8]> for ConnectResponse {
    fn from(value: &[u8]) -> Self {
        Self {
            transaction_id: u32::from_be_bytes(value[4..8].try_into().expect("Can obtain: 4..8")),
            connection_id: u64::from_be_bytes(value[8..16].try_into().expect("Can obtain: 8..16")),
        }
    }
}

impl ConnectResponse {
    fn is_valid(&self, expected_transaction_id: u32) -> bool {
        self.transaction_id == expected_transaction_id
    }
}

#[derive(Debug)]
struct AnnounceResponse {
    transaction_id: u32,
    interval: u32,
    leechers: u32,
    seeders: u32,
}

impl From<&[u8]> for AnnounceResponse {
    fn from(value: &[u8]) -> Self {
        Self {
            transaction_id: u32::from_be_bytes(value[4..8].try_into().expect("Failed bytes: 4..8")),
            interval: u32::from_be_bytes(value[8..12].try_into().expect("Failed bytes: 8..12")),
            leechers: u32::from_be_bytes(value[12..16].try_into().expect("Failed bytes: 12..16")),
            seeders: u32::from_be_bytes(value[16..20].try_into().expect("Failed bytes: 16..20")),
        }
    }
}

#[derive(Debug)]
struct ErrorResponse {
    transaction_id: u32,
    msg: String,
}

impl From<&[u8]> for ErrorResponse {
    fn from(value: &[u8]) -> Self {
        Self {
            transaction_id: u32::from_be_bytes(value[4..8].try_into().expect("No 4..8 bytes")),
            msg: String::from_utf8(value[8..].to_vec()).expect("Failed decoding error message"),
        }
    }
}

#[derive(Debug)]
struct Tracker {
    announce: String,
    info: AnnounceInfo,
    info_hash: Vec<u8>,
}

impl Tracker {
    fn new(filename: String) -> Result<Tracker, Box<dyn Error>> {
        let mut bytes: Vec<u8> = vec![];
        let mut file = File::open(filename)?;
        file.read_to_end(&mut bytes)?;

        let mut byte_reader = ByteReader::new(bytes);

        let ben = BenType::read_into(&mut byte_reader)?;
        let mut ben_map = ben.try_into_dict().unwrap();

        let mut hasher = Sha1::new();
        hasher.update(ben_map["info"].serialize());

        let info_hash = hasher.finalize().to_vec();

        let mut info = ben_map
            .remove(&"info".to_owned())
            .unwrap()
            .try_into_dict()
            .unwrap();

        return Ok(Tracker {
            announce: ben_map
                .remove(&"announce".to_owned())
                .unwrap()
                .try_into_str()
                .unwrap(),
            info: AnnounceInfo {
                pieces: info
                    .remove(&"pieces".to_owned())
                    .unwrap()
                    .try_into_pieces()
                    .unwrap(),
                pieces_len: info
                    .remove(&"piece length".to_owned())
                    .unwrap()
                    .try_into_int()
                    .unwrap() as usize,
                len: info
                    .remove(&"length".to_owned())
                    .unwrap()
                    .try_into_int()
                    .unwrap() as usize,
                name: info
                    .remove(&"name".to_owned())
                    .unwrap()
                    .try_into_str()
                    .unwrap(),
            },
            info_hash,
        });
    }

    fn announce_addr(&self) -> SocketAddr {
        url::Url::parse(self.announce.as_str())
            .expect("URL is not parsable")
            .socket_addrs(|| Some(6881))
            .expect("Cannot extract socket addr")
            .into_iter()
            .next()
            .expect("No socket addr")
    }

    async fn connect(&self, socket: &UdpSocket) -> ConnectResponse {
        socket
            .connect(self.announce_addr())
            .await
            .expect("Can connect to peer");

        let mut buf: Vec<u8> = vec![];

        let mut rng = rand::thread_rng();
        let transaction_id = rng.gen::<u32>();

        to_buf!(CONNECT_MAGIC_NUMBER, buf);
        to_buf!(ACTION_CONNECT, buf);
        to_buf!(transaction_id, buf);

        log::info!("Connection request start");
        socket
            .send_to(&buf, self.announce_addr())
            .await
            .expect("Can send connect payload");
        log::info!("Connection request end");

        let mut response_buf: [u8; 64] = [0; 64];
        log::info!("Connection response listen");
        socket
            .recv(&mut response_buf)
            .await
            .expect("Can receive connection response");
        log::info!("Connection response received");

        let action_code = action_code(&response_buf[..]);
        assert_eq!(ACTION_CONNECT, action_code);

        let resp: ConnectResponse = response_buf[..].into();

        assert!(resp.is_valid(transaction_id));

        resp
    }

    async fn announce(&self, socket: &UdpSocket, connection_id: u64) -> AnnounceResponse {
        socket
            .connect(self.announce_addr())
            .await
            .expect("Can connect to peer");

        let mut buf: Vec<u8> = vec![];

        let mut rng = rand::thread_rng();
        let transaction_id = rng.gen::<u32>();
        let key = rng.gen::<u32>();

        to_buf!(connection_id, buf);
        to_buf!(ACTION_ANNOUNCE, buf);
        to_buf!(transaction_id, buf);
        vec_to_buf!(&self.info_hash, buf);
        vec_to_buf!(&PEER_ID.bytes().collect::<Vec<u8>>(), buf);
        to_buf!(0u64, buf); // FIXME: Set downloaded to a real value.
        to_buf!(self.info.len as u64, buf);
        to_buf!(0u64, buf); // FIXME: Set uploaded to a real value.
        to_buf!(ANNOUNCE_EVENT_NONE, buf);
        to_buf!(0u32, buf); // IP address.
        to_buf!(key, buf); // FIXME: What is a key?
        to_buf!(MAX_PEERS, buf); // Numwant.
        to_buf!(LOCAL_PORT, buf);

        assert_eq!(98, buf.len());

        log::info!("Announce request start");
        socket
            .send_to(&buf, self.announce_addr())
            .await
            .expect("Failed announcing");
        log::info!("Announce request end");

        let mut response_buf: [u8; 2048] = [0; 2048];

        log::info!("Announce response listen");
        socket
            .recv(&mut response_buf)
            .await
            .expect("Failed getting announce response");
        log::info!("Announce response received");

        let resp_action = action_code(&response_buf[..]);
        if resp_action == ACTION_ANNOUNCE {
            let resp: AnnounceResponse = response_buf[..].into();
            assert_eq!(transaction_id, resp.transaction_id);
            dbg!(&resp);

            return resp;
        } else if resp_action == ACTION_ERROR {
            let err: ErrorResponse = response_buf[..].into();
            assert_eq!(transaction_id, err.transaction_id);
            dbg!(err);
        } else {
            log::error!("Unexpected announce response action: {}", resp_action);
            dbg!(response_buf);
        }
        panic!("Failed having announcement success response")
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    SimpleLogger::new()
        .init()
        .expect("Failed initializing logger");

    let args = Args::parse();
    let tracker = Tracker::new(args.filename).expect("Torrent can be created");

    let socket = UdpSocket::bind(LOCAL_SOCKET_ADDR)
        .await
        .expect("Handshake connection established");

    let connection_response = tracker.connect(&socket).await;
    let announce_response = tracker
        .announce(&socket, connection_response.connection_id)
        .await;

    Ok(())
}
