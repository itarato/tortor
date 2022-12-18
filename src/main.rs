use clap::Parser;
use std::{
    error::Error,
    fs::File,
    io::Read,
    collections::HashMap,
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short = 'f')]
    filename: String,
}

const BUF_READER_CHUNK_SIZE: usize = 1024;

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
            .and_then(|s| self.read_n(1).map(|_| s))
    }

    fn peek(&mut self) -> Result<u8, Box<dyn Error>> {
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

    fn read_until(&mut self, pred: fn(u8) -> bool) -> Result<String, Box<dyn Error>> {
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

    fn is_end_of_stream(&self) -> bool {
        self.cursor >= self.readlen
    }

    fn read_n(&mut self, n: usize) -> Result<String, Box<dyn Error>> {
        let mut n = n as i32;
        let mut out = String::new();

        while n > 0 || self.is_complete {
            let slice_len = i32::min(n as i32, (self.readlen - self.cursor) as i32);

            if slice_len == 0 {
                self.fill_buffer()?;
                continue;
            }

            out.push_str(
                String::from_utf8(
                    self.buffer[self.cursor as usize..self.cursor + slice_len as usize].into(),
                )?
                .as_str(),
            );
            self.cursor += slice_len as usize;

            n -= slice_len;
        }

        if n > 0 {
            return Err("not enough bytes".into());
        }

        Ok(out)
    }
    

    fn read_bytes_n(&mut self, n: usize) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut n = n as i32;
        let mut out = vec![];

        while n > 0 || self.is_complete {
            let slice_len = i32::min(n as i32, (self.readlen - self.cursor) as i32);

            if slice_len == 0 {
                self.fill_buffer()?;
                continue;
            }

            out.append(
                &mut self.buffer[self.cursor as usize..self.cursor + slice_len as usize].to_vec()
            );
            self.cursor += slice_len as usize;

            n -= slice_len;
        }

        if n > 0 {
            return Err("not enough bytes".into());
        }

        Ok(out)
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
    Dict(HashMap<String, BenType>),
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

    fn read_dictionary(reader: &mut BufReader) -> Result<HashMap<String, BenType>, Box<dyn Error>> {
        assert_eq!("d".to_owned(), reader.read_n(1)?);

        let mut out = HashMap::new();

        loop {
            if reader.peek()? == b'e' {
                assert_eq!("e".to_owned(), reader.read_n(1)?);
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
        assert_eq!("l".to_owned(), reader.read_n(1)?);

        let mut list = vec![];

        loop {
            if reader.peek()? == b'e' {
                assert_eq!("e".to_owned(), reader.read_n(1)?);
                break;
            }

            let val = BenType::read_into(reader)?;

            list.push(val);
        }

        Ok(list)
    }

    fn read_string(reader: &mut BufReader) -> Result<String, Box<dyn Error>> {
        let len_raw = reader.read_until_and_swallow_last(|b| {
            b != ':' as u8
        })?;
        
        let len = usize::from_str_radix(&len_raw, 10)?;
        let bytes = reader.read_bytes_n(len)?;
        Ok(String::from_utf8(bytes).unwrap_or("error".to_owned()))
    }

    fn read_pieces(reader: &mut BufReader) -> Result<BenType, Box<dyn Error>> {
        let len_raw = reader.read_until_and_swallow_last(|b| {
            b != ':' as u8
        })?;
        let len = usize::from_str_radix(&len_raw, 10)?;

        Ok(BenType::Pieces(reader.read_bytes_n(len)?.chunks(20).map(|chunk| chunk.to_vec()).collect()))
    }

    fn read_int(reader: &mut BufReader) -> Result<i32, Box<dyn Error>> {
        assert_eq!("i".to_owned(), reader.read_n(1)?);

        let int_raw = reader.read_until_and_swallow_last(|b| b != b'e')?;

        Ok(i32::from_str_radix(int_raw.as_str(), 10)?)
    }
}

#[derive(Debug)]
struct Torrent {}

impl Torrent {
    fn new(filename: String) -> Result<Torrent, Box<dyn Error>> {
        let mut buf_reader = BufReader::new(filename)?;
        let ben = BenType::read_into(&mut buf_reader)?;

        Ok(Torrent {})
    }
}

fn main() {
    let args = Args::parse();
    let torrent = Torrent::new(args.filename);

    dbg!(torrent);
}
