#[derive(Debug, Clone)]
struct Piece {
    fragments: Vec<u8>,
}

impl Piece {
    fn new(size: usize) -> Self {
        Self {
            fragments: vec![0; size],
        }
    }

    fn mark_complete(&mut self, idx: usize) {
        let unit = idx >> 3;
        let subunit = 1 << (idx & 0b111);
        self.fragments[unit] |= subunit;
    }

    fn is_complete(&self, idx: usize) -> bool {
        let unit = idx >> 3;
        let subunit = 1 << (idx & 0b111);
        (self.fragments[unit] & subunit) > 0
    }
}

#[derive(Debug)]
pub struct Download {
    pieces: Vec<Piece>,
}

impl Download {
    pub fn new(size: usize, fragment_size: usize) -> Self {
        Self {
            pieces: vec![Piece::new(fragment_size); size],
        }
    }

    pub fn mark_complete(&mut self, piece_idx: usize, fragment_idx: usize) {
        self.pieces[piece_idx].mark_complete(fragment_idx);
    }

    pub fn is_complete(&self, piece_idx: usize, fragment_idx: usize) -> bool {
        self.pieces[piece_idx].is_complete(fragment_idx)
    }
}
