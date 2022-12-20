use clap::Parser;
use sha1::{Digest, Sha1};
use std::{
    borrow::Borrow,
    collections::HashMap,
    error::Error,
    fs::File,
    hash::Hash,
    io::{Read, Write},
    ops::Index,
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short = 'f')]
    filename: String,
}

const BUF_READER_CHUNK_SIZE: usize = 1024;

#[derive(Debug)]
struct StableHashMap<K, V> {
    map: HashMap<K, V>,
    keys: Vec<K>,
}

impl<K: Eq + Hash, V> StableHashMap<K, V> {
    fn new() -> Self {
        StableHashMap {
            map: HashMap::new(),
            keys: vec![],
        }
    }

    fn iter<'a>(&'a self) -> StableHashMapIter<'a, K, V> {
        StableHashMapIter {
            stable_hash_map: self,
            index: 0,
        }
    }

    fn insert(&mut self, key: K, value: V)
    where
        K: Clone,
    {
        self.map.insert(key.clone(), value);
        self.keys.push(key);
    }
}

impl<K, Q: ?Sized, V> Index<&Q> for StableHashMap<K, V>
where
    K: Eq + Hash + Borrow<Q>,
    Q: Eq + Hash,
{
    type Output = V;

    fn index(&self, k: &Q) -> &Self::Output {
        self.map.get(&k).unwrap()
    }
}

struct StableHashMapIter<'a, K, V> {
    stable_hash_map: &'a StableHashMap<K, V>,
    index: usize,
}

impl<'a, K: Eq + Hash, V> Iterator for StableHashMapIter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.stable_hash_map.keys.len() {
            return None;
        }

        self.index += 1;

        let key = self
            .stable_hash_map
            .keys
            .get(self.index as usize - 1)
            .unwrap();
        let val = self.stable_hash_map.map.get(key).unwrap();

        Some((key, val))
    }
}

#[derive(Debug)]
struct BufReader {
    file: File,
    buffer: [u8; BUF_READER_CHUNK_SIZE],
    readlen: usize,
    cursor: usize,
    is_complete: bool,
}

impl BufReader {
    fn new(filename: String) -> Result<BufReader, Box<dyn Error>> {
        Ok(BufReader {
            file: File::open(filename)?,
            buffer: [0; BUF_READER_CHUNK_SIZE],
            readlen: 0,
            cursor: 0,
            is_complete: false,
        })
    }

    fn read_until_and_swallow_last(
        &mut self,
        pred: fn(u8) -> bool,
    ) -> Result<String, Box<dyn Error>> {
        self.read_until(pred)
            .and_then(|s| self.read_bytes_n(1).map(|_| s))
    }

    pub fn peek(&mut self) -> Result<u8, Box<dyn Error>> {
        loop {
            if self.is_complete {
                return Err("End of stream".into());
            }

            if self.is_end_of_stream() {
                self.fill_buffer()?;
                continue;
            }

            return Ok(self.buffer[self.cursor]);
        }
    }

    pub fn read_until(&mut self, pred: fn(u8) -> bool) -> Result<String, Box<dyn Error>> {
        let mut out = String::new();

        loop {
            if self.is_complete {
                break;
            }

            if self.is_end_of_stream() {
                self.fill_buffer()?;
                continue;
            }

            if !pred(self.buffer[self.cursor as usize]) {
                break;
            }

            out.push(self.buffer[self.cursor as usize].into());
            self.cursor += 1;
        }

        Ok(out)
    }

    pub fn read_bytes_n(&mut self, n: usize) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut n = n as i32;
        let mut out = vec![];

        while n > 0 || self.is_complete {
            let slice_len = i32::min(n as i32, (self.readlen - self.cursor) as i32);

            if slice_len == 0 {
                self.fill_buffer()?;
                continue;
            }

            out.append(
                &mut self.buffer[self.cursor as usize..self.cursor + slice_len as usize].to_vec(),
            );
            self.cursor += slice_len as usize;

            n -= slice_len;
        }

        if n > 0 {
            return Err("not enough bytes".into());
        }

        Ok(out)
    }

    fn is_end_of_stream(&self) -> bool {
        self.cursor >= self.readlen
    }

    fn fill_buffer(&mut self) -> Result<(), Box<dyn Error>> {
        if self.is_complete {
            return Err("reader is already complete".into());
        }

        if self.cursor < self.readlen {
            return Err("buffer is not yet consumed".into());
        }

        self.file
            .read(&mut self.buffer)
            .and_then(|size| {
                self.readlen = size;
                self.cursor = 0;

                self.is_complete = size == 0;

                Ok(())
            })
            .map_err(|err| err.into())
    }
}

#[derive(Debug)]
enum BenType {
    Str(String),
    Int(i32),
    List(Vec<BenType>),
    Dict(StableHashMap<String, BenType>),
    Pieces(Vec<Vec<u8>>),
}

impl BenType {
    fn read_into(reader: &mut BufReader) -> Result<Self, Box<dyn Error>> {
        match reader.peek()? {
            b'd' => Ok(BenType::Dict(BenType::read_dictionary(reader)?)),
            b'i' => Ok(BenType::Int(BenType::read_int(reader)?)),
            b'l' => Ok(BenType::List(BenType::read_list(reader)?)),
            _ => Ok(BenType::Str(BenType::read_string(reader)?)),
        }
    }

    fn read_dictionary(
        reader: &mut BufReader,
    ) -> Result<StableHashMap<String, BenType>, Box<dyn Error>> {
        assert_eq!(vec![b'd'], reader.read_bytes_n(1)?);

        let mut out = StableHashMap::new();

        loop {
            if reader.peek()? == b'e' {
                assert_eq!(vec![b'e'], reader.read_bytes_n(1)?);
                break;
            }

            let key = BenType::read_string(reader)?;
            let value = match key.as_str() {
                "pieces" => BenType::read_pieces(reader)?,
                _ => BenType::read_into(reader)?,
            };

            out.insert(key, value);
        }

        Ok(out)
    }

    fn read_list(reader: &mut BufReader) -> Result<Vec<BenType>, Box<dyn Error>> {
        assert_eq!(vec![b'l'], reader.read_bytes_n(1)?);

        let mut list = vec![];

        loop {
            if reader.peek()? == b'e' {
                assert_eq!(vec![b'e'], reader.read_bytes_n(1)?);
                break;
            }

            let val = BenType::read_into(reader)?;

            list.push(val);
        }

        Ok(list)
    }

    fn read_string(reader: &mut BufReader) -> Result<String, Box<dyn Error>> {
        let len_raw = reader.read_until_and_swallow_last(|b| b != ':' as u8)?;

        let len = usize::from_str_radix(&len_raw, 10)?;
        let bytes = reader.read_bytes_n(len)?;
        Ok(String::from_utf8(bytes).unwrap_or("error".to_owned()))
    }

    fn read_pieces(reader: &mut BufReader) -> Result<BenType, Box<dyn Error>> {
        let len_raw = reader.read_until_and_swallow_last(|b| b != ':' as u8)?;
        let len = usize::from_str_radix(&len_raw, 10)?;

        Ok(BenType::Pieces(
            reader
                .read_bytes_n(len)?
                .chunks(20)
                .map(|chunk| chunk.to_vec())
                .collect(),
        ))
    }

    fn read_int(reader: &mut BufReader) -> Result<i32, Box<dyn Error>> {
        assert_eq!(vec![b'i'], reader.read_bytes_n(1)?);

        let int_raw = reader.read_until_and_swallow_last(|b| b != b'e')?;

        Ok(i32::from_str_radix(int_raw.as_str(), 10)?)
    }

    fn serialize(&self) -> Vec<u8> {
        let mut out = vec![];

        match &self {
            BenType::Dict(dict) => {
                out.push(b'd');

                for (k, v) in dict.iter() {
                    let mut len_bytes: Vec<u8> = k.len().to_string().bytes().collect();
                    out.append(&mut len_bytes);
                    out.push(b':');
                    out.append(&mut k.bytes().collect());

                    dbg!(k);

                    out.append(&mut v.serialize());
                }

                out.push(b'e');
            }
            BenType::Int(int) => {
                out.push(b'i');
                out.append(&mut int.to_string().bytes().collect());
                out.push(b'e');
            }
            BenType::List(list) => {
                out.push(b'l');
                list.iter()
                    .for_each(|elem| out.append(&mut elem.serialize()));
                out.push(b'e');
            }
            BenType::Pieces(pieces) => {
                let len = pieces.iter().map(|slice| slice.len()).sum::<usize>();
                out.append(&mut len.to_string().bytes().collect());
                out.push(b':');
                pieces.iter().for_each(|piece| {
                    out.append(&mut piece.clone());
                });
            }
            BenType::Str(str) => {
                let mut len_bytes: Vec<u8> = str.len().to_string().bytes().collect();
                out.append(&mut len_bytes);
                out.push(b':');
                out.append(&mut str.bytes().collect());
            }
        }

        out
    }

    fn try_into_dict(&self) -> Option<&StableHashMap<String, BenType>> {
        match self {
            BenType::Dict(dict) => Some(dict),
            _ => None,
        }
    }

    fn try_into_pieces(&self) -> Option<&Vec<Vec<u8>>> {
        match self {
            BenType::Pieces(pieces) => Some(pieces),
            _ => None,
        }
    }

    fn try_into_str(&self) -> Option<&String> {
        match self {
            BenType::Str(s) => Some(s),
            _ => None,
        }
    }

    fn try_into_int(&self) -> Option<i32> {
        match self {
            BenType::Int(i) => Some(*i),
            _ => None,
        }
    }
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
        let mut buf_reader = BufReader::new(filename)?;
        let ben = BenType::read_into(&mut buf_reader)?;

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
