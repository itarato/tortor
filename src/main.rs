use clap::Parser;
use std::{
    error::Error,
    fs::File,
    io::{prelude, Read},
    slice,
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
    cursor: i32,
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
        self.cursor >= self.readlen as i32
    }

    fn read_n(&mut self, n: usize) -> Result<String, Box<dyn Error>> {
        let mut n = n as i32;
        let mut out = String::new();

        while n > 0 || self.is_complete {
            let slice_len = i32::min(n as i32, (self.readlen as i32) - self.cursor);

            if slice_len == 0 {
                self.fill_buffer()?;
                continue;
            }

            out.push_str(
                String::from_utf8(
                    self.buffer[self.cursor as usize..(self.cursor + slice_len) as usize].into(),
                )?
                .as_str(),
            );
            self.cursor += slice_len;

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

        if self.cursor < self.readlen as i32 {
            return Err("buffer is not yet consumed".into());
        }

        self.file
            .read(&mut self.buffer)
            .and_then(|size| {
                dbg!(size);

                self.readlen = size;
                self.cursor = 0;

                self.is_complete = size == 0;

                Ok(())
            })
            .map_err(|err| err.into())
    }
}

struct Torrent {}

impl Torrent {
    fn new(filename: String) -> Result<Torrent, Box<dyn Error>> {
        let mut buf_reader = BufReader::new(filename)?;

        dbg!(buf_reader.read_until_and_swallow_last(|byte| byte != ':' as u8));

        dbg!(buf_reader.read_n(16));

        Ok(Torrent {})
    }
}

fn main() {
    let args = Args::parse();
    let torrent = Torrent::new(args.filename);
}
