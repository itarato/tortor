pub struct ByteReader {
    pub bytes: Vec<u8>,
    pub pos: usize,
}

impl ByteReader {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes, pos: 0 }
    }

    pub fn peek(&self) -> Option<u8> {
        if self.finished() {
            None
        } else {
            Some(self.bytes[self.pos])
        }
    }

    pub fn take_while<'a>(&'a mut self, pred: fn(u8) -> bool) -> &'a [u8] {
        let start = self.pos;

        loop {
            if self.finished() {
                break;
            }

            if !pred(self.bytes[self.pos]) {
                break;
            }

            self.pos += 1;
        }

        &self.bytes[start..self.pos]
    }

    pub fn take_n<'a>(&'a mut self, n: usize) -> &'a [u8] {
        let start = self.pos;
        let n = n.min(self.bytes.len() - self.pos);

        self.pos += n;

        &self.bytes[start..self.pos]
    }

    pub fn next(&mut self) -> Option<u8> {
        if self.finished() {
            None
        } else {
            self.pos += 1;
            Some(self.bytes[self.pos - 1])
        }
    }

    fn finished(&self) -> bool {
        self.pos >= self.bytes.len()
    }
}

mod test {
    use super::ByteReader;

    #[test]
    fn test_base() {
        let mut reader = ByteReader::new(vec![0, 1, 2, 3, 4]);
        assert_eq!(Some(0), reader.peek());

        assert_eq!(&[0, 1], reader.take_while(|b| b != 2));

        assert_eq!(Some(2), reader.peek());
    }
}
