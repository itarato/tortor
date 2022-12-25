mod ben_type;
mod byte_reader;
mod stable_hash_map;

use byte_reader::ByteReader;
use clap::Parser;
use sha1::{Digest, Sha1};
use std::{
    collections::VecDeque,
    error::Error,
    fs::File,
    io::{Read, Write},
    net::SocketAddr,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
};

use crate::ben_type::*;

static PEER_ID: &'static str = "M0-0-0--IT2022------";

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short = 'f')]
    filename: String,
}

#[derive(Debug)]
struct AnnounceInfo {
    pieces: Vec<Vec<u8>>,
    pieces_len: usize,
    len: usize,
    name: String,
}

#[derive(Debug)]
struct Announce {
    announce: String,
    // announce_list: Vec<String>,
    info: AnnounceInfo,
    info_hash: Vec<u8>,
}

impl Announce {
    fn new(filename: String) -> Result<Announce, Box<dyn Error>> {
        let mut bytes: Vec<u8> = vec![];
        let mut file = File::open(filename)?;
        file.read_to_end(&mut bytes)?;

        let mut byte_reader = ByteReader::new(bytes);

        let ben = BenType::read_into(&mut byte_reader)?;

        let mut out_file = File::create("./sample.torrent").expect("Can create test out file");
        out_file
            .write_all(ben.serialize().as_slice())
            .expect("Can write test output");

        let mut ben_map = ben.try_into_dict().unwrap();

        let mut hasher = Sha1::new();
        hasher.update(ben_map["info"].serialize());

        let info_hash = hasher.finalize().to_vec();

        let mut info = ben_map
            .remove(&"info".to_owned())
            .unwrap()
            .try_into_dict()
            .unwrap();

        return Ok(Announce {
            announce: ben_map
                .remove(&"announce".to_owned())
                .unwrap()
                .try_into_str()
                .unwrap(),
            // announce_list: ben_map
            //     .remove(&"announce-list".to_owned())
            //     .unwrap()
            //     .try_into_list()
            //     .unwrap()
            //     .into_iter()
            //     .flat_map(|list| {
            //         list.try_into_list()
            //             .unwrap()
            //             .into_iter()
            //             .map(|e| e.try_into_str().unwrap())
            //     })
            //     .collect(),
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

    fn announce_url(&self) -> String {
        format!(
            "{}?info_hash={}&left={}&peer_id={}&port=6881&uploaded=0&downloaded=0&compact=1&numwant=10",
            self.announce,
            self.info_hash
                .iter()
                .map(|b| format!("%{:02x}", b))
                .collect::<String>(),
            self.info.len,
            PEER_ID,
        )
    }

    fn handshake_body(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(0x13);

        for b in b"BitTorrent protocol" {
            out.push(*b);
        }

        for _ in 0..8 {
            out.push(0);
        }

        self.info_hash.iter().for_each(|b| out.push(*b));

        PEER_ID.bytes().for_each(|b| out.push(b));

        out
    }
}

#[derive(Debug)]
struct AnnounceResponse {
    // interval: i64,
    // complete: i64,
    // incomplete: i64,
    peers: Vec<IP>,
}

impl AnnounceResponse {
    fn ip_string(&self, n: usize) -> Option<String> {
        if n >= self.peers.len() {
            None
        } else {
            let ip_str = self.peers[n].address.map(|byte| byte.to_string()).join(".");
            let port = self.peers[n].port;
            Some(format!("{}:{}", ip_str, port))
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let torrent = Announce::new(args.filename).expect("Torrent can be created");

    println!(
        "Name: {}\nURL: {}",
        torrent.info.name,
        torrent.announce_url(),
        // torrent.announce_list
    );

    // let resp = reqwest::get(torrent.announce_url()).await?;

    // let resp_body = resp.bytes().await?.to_vec();
    // let mut resp_ben = BenType::read_into(&mut ByteReader::new(resp_body))?
    //     .try_into_dict()
    //     .unwrap();

    // let announce_response = AnnounceResponse {
    //     // interval: resp_ben
    //     //     .remove(&"interval".to_owned())
    //     //     .unwrap()
    //     //     .try_into_int()
    //     //     .unwrap(),
    //     // complete: resp_ben
    //     //     .remove(&"complete".to_owned())
    //     //     .unwrap()
    //     //     .try_into_int()
    //     //     .unwrap(),
    //     // incomplete: resp_ben
    //     //     .remove(&"incomplete".to_owned())
    //     //     .unwrap()
    //     //     .try_into_int()
    //     //     .unwrap(),
    //     peers: resp_ben
    //         .remove(&"peers".to_owned())
    //         .unwrap()
    //         .try_into_peers()
    //         .unwrap(),
    // };

    // if announce_response.peers.is_empty() {
    //     println!("No peers found");
    //     return Ok(());
    // }

    // dbg!(&announce_response);
    // dbg!(&announce_response.ip_string(0));

    let stream = UdpSocket::bind("0.0.0.0:6881")
        .await
        .expect("Handshake connection established");

    // stream.set_ttl(3).expect("Can set TTL");
    // let addr = torrent
    //     .announce
    //     .parse::<SocketAddr>()
    //     .expect("Can make connection address");
    stream
        .connect("bttracker.debian.org:6969")
        .await
        .expect("Can connect to peer");

    let mut connect_buf: Vec<u8> = vec![];
    for b in 0x41727101980u64.to_be_bytes() {
        connect_buf.push(b);
    }
    let action = 0u32;
    for b in action.to_be_bytes() {
        connect_buf.push(b);
    }
    let transaction_id = 0x12345678u32;
    for b in transaction_id.to_be_bytes() {
        connect_buf.push(b);
    }

    println!("Start connection payload");
    stream
        .send_to(&connect_buf, "bttracker.debian.org:6969")
        .await
        .expect("Can send connect payload");
    println!("Sent connection payload");

    let mut connect_response_buf: [u8; 16] = [0; 16];
    println!("Start connection payload");
    stream
        .recv_from(&mut connect_response_buf)
        .await
        .expect("Can receive connection response");
    println!("Got connection payload");

    dbg!(connect_response_buf);

    // // stream
    // //     .write_all_buf(&mut torrent.handshake_body())
    // //     .await
    // //     .expect("Handshake payload sent");

    // stream
    //     .send(&torrent.handshake_body())
    //     .await
    //     .expect("Can send UDP payload.");

    // let mut buf: [u8; 1] = [0; 1];

    // // let handshake_resp = stream
    // //     .read_exact(&mut buf)
    // //     .await
    // //     .expect("Handshare response first byte received");
    // // dbg!(handshake_resp);

    // stream.recv(&mut buf).await.expect("Got response");
    // dbg!(buf);

    Ok(())
}
