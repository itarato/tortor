mod ben_type;
mod byte_reader;
mod stable_hash_map;

use byte_reader::ByteReader;
use clap::Parser;
use sha1::{Digest, Sha1};
use std::{
    error::Error,
    fs::File,
    io::{Read, Write},
};

use crate::ben_type::*;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short = 'f')]
    filename: String,
}

#[derive(Debug)]
struct TorrentInfo {
    pieces: Vec<Vec<u8>>,
    pieces_len: usize,
    len: usize,
    name: String,
}

#[derive(Debug)]
struct Torrent {
    announce: String,
    info: TorrentInfo,
    info_hash: Vec<u8>,
}

impl Torrent {
    fn new(filename: String) -> Result<Torrent, Box<dyn Error>> {
        let mut bytes: Vec<u8> = vec![];
        let mut file = File::open(filename)?;
        file.read_to_end(&mut bytes)?;

        let mut byte_reader = ByteReader::new(bytes);

        let ben = BenType::read_into(&mut byte_reader)?;

        let mut out_file = File::create("./sample.torrent").expect("Can create test out file");
        out_file
            .write_all(ben.serialize().as_slice())
            .expect("Can write test output");

        let ben_map = ben.try_into_dict().unwrap();

        let mut hasher = Sha1::new();
        hasher.update(ben_map["info"].serialize());

        let info_hash = hasher.finalize().to_vec();

        let info = ben_map["info"].try_into_dict().unwrap();

        return Ok(Torrent {
            announce: ben_map["announce"].try_into_str().unwrap().clone(),
            info: TorrentInfo {
                pieces: info["pieces"].try_into_pieces().unwrap().clone(),
                pieces_len: info["piece length"].try_into_int().unwrap() as usize,
                len: info["length"].try_into_int().unwrap() as usize,
                name: info["name"].try_into_str().unwrap().clone(),
            },
            info_hash,
        });
    }

    fn announce_url(&self) -> String {
        format!(
            "{}?info_hash={}&left={}&peer_id=imjusttryingtomakeit&port=6881&uploaded=0&downloaded=0&compact=1",
            self.announce,
            self
                .info_hash
                .iter()
                .map(|b| format!("%{:02x}", b))
                .collect::<String>(),
            self.info.len,
        )
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let torrent = Torrent::new(args.filename).expect("Torrent can be created");

    println!(
        "Name: {}\nURL: {}",
        torrent.info.name,
        torrent.announce_url()
    );

    let resp = reqwest::get(torrent.announce_url()).await?;

    let resp_body = resp.bytes().await?.to_vec();

    Ok(())
}
