use std::error::Error;

use crate::byte_reader::*;
use crate::stable_hash_map::*;
use crate::IP;

#[derive(Debug)]
pub enum BenType {
    Str(String),
    Int(i64),
    List(Vec<BenType>),
    Dict(StableHashMap<String, BenType>),
    Pieces(Vec<Vec<u8>>),
    Peers(Vec<IP>),
}

impl BenType {
    pub fn read_into(reader: &mut ByteReader) -> Result<Self, Box<dyn Error>> {
        match reader.peek().expect("Has type sig char") {
            b'd' => Ok(BenType::Dict(BenType::read_dictionary(reader)?)),
            b'i' => Ok(BenType::Int(BenType::read_int(reader)?)),
            b'l' => Ok(BenType::List(BenType::read_list(reader)?)),
            _ => Ok(BenType::Str(BenType::read_string(reader)?)),
        }
    }

    fn read_dictionary(
        reader: &mut ByteReader,
    ) -> Result<StableHashMap<String, BenType>, Box<dyn Error>> {
        assert_eq!(b'd', reader.next().expect("Starts with d"));

        let mut out = StableHashMap::new();

        loop {
            if b'e' == reader.peek().expect("Ends with e") {
                assert_eq!(b'e', reader.next().expect("Ends with e"));
                break;
            }

            let key = BenType::read_string(reader)?;

            let value = match key.as_str() {
                "pieces" => BenType::read_pieces(reader)?,
                "peers" => BenType::read_peers(reader)?,
                _ => BenType::read_into(reader)?,
            };

            out.insert(key, value);
        }

        Ok(out)
    }

    fn read_list(reader: &mut ByteReader) -> Result<Vec<BenType>, Box<dyn Error>> {
        assert_eq!(b'l', reader.next().expect("Starts with l"));

        let mut list = vec![];

        loop {
            if b'e' == reader.peek().expect("Ends with e") {
                assert_eq!(b'e', reader.next().expect("Ends with e"));
                break;
            }

            let val = BenType::read_into(reader)?;

            list.push(val);
        }

        Ok(list)
    }

    fn read_string(reader: &mut ByteReader) -> Result<String, Box<dyn Error>> {
        let len_raw = reader.take_while(|b| b != b':');
        let len = usize::from_str_radix(std::str::from_utf8(len_raw)?, 10)?;

        assert_eq!(Some(b':'), reader.next());

        let bytes = reader.take_n(len);
        Ok(std::str::from_utf8(bytes)?.to_owned())
    }

    fn read_pieces(reader: &mut ByteReader) -> Result<BenType, Box<dyn Error>> {
        let len_raw = reader.take_while(|b| b != b':');
        let len = usize::from_str_radix(std::str::from_utf8(len_raw)?, 10)?;

        assert_eq!(Some(b':'), reader.next());

        Ok(BenType::Pieces(
            reader
                .take_n(len)
                .chunks(20)
                .map(|chunk| chunk.to_vec())
                .collect(),
        ))
    }

    fn read_peers(reader: &mut ByteReader) -> Result<BenType, Box<dyn Error>> {
        let len_raw = reader.take_while(|b| b != b':');
        let len = usize::from_str_radix(std::str::from_utf8(len_raw)?, 10)?;

        assert_eq!(Some(b':'), reader.next());

        Ok(BenType::Peers(
            reader
                .take_n(len)
                .chunks(6)
                .map(|chunk| IP {
                    address: chunk[0..4].try_into().unwrap(),
                    port: ((chunk[4] as u16) << 8) | chunk[5] as u16,
                })
                .collect(),
        ))
    }

    fn read_int(reader: &mut ByteReader) -> Result<i64, Box<dyn Error>> {
        assert_eq!(Some(b'i'), reader.next());

        let int_raw = reader.take_while(|b| b != b'e');
        let int = i64::from_str_radix(std::str::from_utf8(int_raw)?, 10)?;

        assert_eq!(Some(b'e'), reader.next());

        Ok(int)
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = vec![];

        match &self {
            BenType::Dict(dict) => {
                out.push(b'd');

                for (k, v) in dict.iter() {
                    let mut len_bytes: Vec<u8> = k.len().to_string().bytes().collect();
                    out.append(&mut len_bytes);
                    out.push(b':');
                    out.append(&mut k.bytes().collect());
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
            BenType::Peers(peers) => {
                let len = peers.len() * 6;
                out.append(&mut len.to_string().bytes().collect());
                out.push(b':');
                for peer in peers {
                    out.append(&mut peer.address.to_vec());
                    out.push((peer.port >> 8) as u8);
                    out.push((peer.port & 0xFF) as u8);
                }
            }
        }

        out
    }

    pub fn try_into_dict(self) -> Option<StableHashMap<String, BenType>> {
        match self {
            BenType::Dict(dict) => Some(dict),
            _ => None,
        }
    }

    pub fn try_into_pieces(self) -> Option<Vec<Vec<u8>>> {
        match self {
            BenType::Pieces(pieces) => Some(pieces),
            _ => None,
        }
    }

    pub fn try_into_str(self) -> Option<String> {
        match self {
            BenType::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn try_into_int(self) -> Option<i64> {
        match self {
            BenType::Int(i) => Some(i),
            _ => None,
        }
    }

    pub fn try_into_peers(self) -> Option<Vec<IP>> {
        match self {
            BenType::Peers(peers) => Some(peers),
            _ => None,
        }
    }

    pub fn try_into_list(self) -> Option<Vec<BenType>> {
        match self {
            BenType::List(list) => Some(list),
            _ => None,
        }
    }
}
