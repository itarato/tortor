use crate::defs::*;

const PIECE_MASK_SIZE: usize = 8;

#[derive(Debug)]
pub struct Piece {
    // Fragments.
    completed: Vec<u8>,
    requested: Vec<u8>,
    last_fragment_idx: usize,
}

impl Piece {
    fn new(size: usize, fragment_size: usize) -> Self {
        let mask_count = (size as f64 / (fragment_size * PIECE_MASK_SIZE) as f64).ceil() as usize;
        let last_fragment_idx = (size as f64 / fragment_size as f64).ceil() as usize - 1;

        Self {
            completed: vec![0; mask_count],
            requested: vec![0; mask_count],
            last_fragment_idx,
        }
    }

    pub fn is_requested(&self, idx: usize) -> bool {
        assert!(idx <= self.last_fragment_idx);

        let (unit, subunit) = Self::index_and_mask(idx);
        (self.completed[unit] & subunit) > 0
    }

    pub fn is_completed(&self, idx: usize) -> bool {
        assert!(idx <= self.last_fragment_idx);

        let (unit, subunit) = Self::index_and_mask(idx);
        (self.completed[unit] & subunit) > 0
    }

    pub fn mark_requested(&mut self, idx: usize) {
        assert!(!self.is_requested(idx));
        assert!(!self.is_completed(idx));
        assert!(idx <= self.last_fragment_idx);

        let (unit, subunit) = Self::index_and_mask(idx);
        self.requested[unit] |= subunit;
    }

    pub fn mark_complete(&mut self, idx: usize) {
        assert!(self.is_requested(idx));
        assert!(!self.is_completed(idx));
        assert!(idx <= self.last_fragment_idx);

        let (unit, subunit) = Self::index_and_mask(idx);
        self.completed[unit] |= subunit;
    }

    fn index_and_mask(idx: usize) -> (usize, u8) {
        (idx >> 3, 1 << (idx & 0b111))
    }

    fn next_not_requested_fragment(&self) -> Option<usize> {
        for (i, mask) in self.requested.iter().enumerate() {
            for offs in 0..PIECE_MASK_SIZE {
                let frag_index = i * PIECE_MASK_SIZE + offs;
                if frag_index > self.last_fragment_idx {
                    return None;
                }

                let check = 1 << offs;
                if mask & check == 0 {
                    return Some(frag_index);
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
    pub fn new(torrent_size: usize, piece_size: usize, fragment_size: usize) -> Self {
        let mut pieces = vec![];

        let piece_count = torrent_size / piece_size;
        for _ in 0..piece_count {
            pieces.push(Piece::new(piece_size, fragment_size));
        }

        let last_piece_size = torrent_size % piece_size;
        if last_piece_size > 0 {
            pieces.push(Piece::new(last_piece_size, fragment_size));
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

mod test {
    use super::*;

    #[test]
    fn test_base() {
        // * Piece#1: [xx______]
        // * Piece#2: [xx______]
        // * Piece#3: [x_______]
        let mut dl = Download::new(73, 32, 16);
        assert_eq!(3, dl.pieces.len());

        assert_eq!(Some((0, 0)), dl.next_not_requested_fragment());
        dl.pieces[0].mark_requested(0);
        assert_eq!(Some((0, 1)), dl.next_not_requested_fragment());
        dl.pieces[0].mark_requested(1);

        assert_eq!(Some((1, 0)), dl.next_not_requested_fragment());
        dl.pieces[1].mark_requested(0);
        assert_eq!(Some((1, 1)), dl.next_not_requested_fragment());
        dl.pieces[1].mark_requested(1);
        assert_eq!(Some((2, 0)), dl.next_not_requested_fragment());

        dl.pieces[2].mark_requested(0);
        assert_eq!(None, dl.next_not_requested_fragment());
    }
}
