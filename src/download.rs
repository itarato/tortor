use crate::defs::*;

const PIECE_MASK_SIZE: usize = 8;

#[derive(Debug)]
pub struct Piece {
    // Fragments.
    completed: Vec<u8>,
    requested: Vec<u8>,
}

impl Piece {
    fn new(size: usize) -> Self {
        let fragment_count =
            (size as f64 / (FRAGMENT_SIZE * PIECE_MASK_SIZE) as f64).ceil() as usize;
        Self {
            completed: vec![0; fragment_count],
            requested: vec![0; fragment_count],
        }
    }

    pub fn is_requested(&self, idx: usize) -> bool {
        let (unit, subunit) = Self::index_and_mask(idx);
        (self.completed[unit] & subunit) > 0
    }

    pub fn is_completed(&self, idx: usize) -> bool {
        let (unit, subunit) = Self::index_and_mask(idx);
        (self.completed[unit] & subunit) > 0
    }

    pub fn mark_requested(&mut self, idx: usize) {
        assert!(!self.is_requested(idx));
        assert!(!self.is_completed(idx));

        let (unit, subunit) = Self::index_and_mask(idx);
        self.requested[unit] |= subunit;
    }

    pub fn mark_complete(&mut self, idx: usize) {
        assert!(self.is_requested(idx));
        assert!(!self.is_completed(idx));

        let (unit, subunit) = Self::index_and_mask(idx);
        self.completed[unit] |= subunit;
    }

    fn index_and_mask(idx: usize) -> (usize, u8) {
        (idx >> 3, 1 << (idx & 0b111))
    }

    fn next_not_requested_fragment(&self) -> Option<usize> {
        for (i, mask) in self.requested.iter().enumerate() {
            for offs in 0..PIECE_MASK_SIZE {
                let check = 1 << offs;
                if mask & check == 0 {
                    return Some(i * PIECE_MASK_SIZE + offs);
                }
            }
        }

        None
    }
}

#[derive(Debug)]
pub struct Download {
    pub pieces: Vec<Piece>,
}

impl Download {
    pub fn new(torrent_size: usize, piece_size: usize) -> Self {
        let mut pieces = vec![];

        let piece_count = torrent_size / piece_size;
        for _ in 0..piece_count {
            pieces.push(Piece::new(piece_size));
        }

        let last_piece_size = torrent_size % piece_size;
        if last_piece_size > 0 {
            pieces.push(Piece::new(last_piece_size));
        }

        Self { pieces }
    }

    pub fn next_not_requested_fragment(&self) -> Option<(usize, usize)> {
        for (i, piece) in self.pieces.iter().enumerate() {
            if let Some(fragment_index) = piece.next_not_requested_fragment() {
                return Some((i, fragment_index));
            }
        }

        None
    }
}
