use std::time::Duration;

pub const WHITE_SHORT: u8 = 0b00001;
pub const WHITE_LONG: u8 = 0b00010;
pub const BLACK_SHORT: u8 = 0b00100;
pub const BLACK_LONG: u8 = 0b01000;
pub const WHITE_TO_MOVE: u8 = 0b10000;

// move flags
pub const PROMOTION_QUEEN: u8 = 0b0000;
pub const PROMOTION_ROOK: u8 = 0b0100;
pub const PROMOTION_BISHOP: u8 = 0b1000;
pub const PROMOTION_KNIGHT: u8 = 0b1100;
pub const NORMAL_MOVE: u8 = 0b0000;
pub const PROMOTION_MOVE: u8 = 0b0001;
pub const EN_PASSANT_MOVE: u8 = 0b0010;
pub const CASTLE_MOVE: u8 = 0b0011;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bitboard(pub u64);

impl Bitboard {
    pub const EMPTY: Self = Self(0);

    pub const fn set(&mut self, square: u8) {
        self.0 |= 1 << square;
    }

    pub fn get(&self, square: u8) -> bool {
        (self.0 & (1 << square)) != 0
    }
}

// log(64) = 6 .. meaning we only need 5 bits of info to represent a square
// bits 0-5:   from
// bits 6-11:  to
// bits 12-15  flags (normal, promo, en passant, castle)
#[derive(Clone, Copy, PartialEq)]
pub struct Move(pub u16);

impl Move {
    pub const NULL: Self = Self(0);

    pub const fn new(from: u8, to: u8, flags: u8) -> Move {
        Move(from as u16 | (to as u16) << 6 | (flags as u16) << 12)
    }

    pub fn get_from(self) -> u8 {
        (self.0 & 0x3F) as u8
    } // __XXXXXX

    pub fn set_from(mut self, sq: u8) {
        self.0 ^= 0x3F;
        self.0 |= sq as u16;
    }

    pub fn get_to(self) -> u8 {
        ((self.0 >> 6) & 0x3F) as u8
    } // __XXXXXX

    pub fn set_to(mut self, sq: u8) {
        self.0 ^= 0x3F << 6;
        self.0 |= (sq as u16) << 6;
    }

    pub fn get_flags(self) -> u8 {
        (self.0 >> 12) as u8
    } // ____XXXX

    pub fn set_flags(mut self, flags: u8) {
        self.0 ^= 0xF << 12;
        self.0 |= (flags as u16) << 12;
    }
}

pub struct MoveBuf {
    pub data: [u8; 32],
    pub len: usize,
}
impl MoveBuf {
    pub fn new() -> Self {
        Self {
            data: [0; 32],
            len: 0,
        }
    }

    #[inline(always)]
    pub fn push(&mut self, sq: u8) {
        debug_assert!(self.len < self.data.len(), "MoveBuf overflow");
        // The things we do for performance.
        // Technically len should never exceed 32, if this panics it is most certainly just a smoking gun
        // and not the root problem
        unsafe {
            *self.data.get_unchecked_mut(self.len) = sq;
        }
        self.len += 1;
    }
}

pub struct MoveListBuf {
    pub data: [Move; 218],
    pub len: usize,
}
impl MoveListBuf {
    pub fn new() -> Self {
        Self {
            data: [Move::NULL; 218],
            len: 0,
        }
    }

    #[inline(always)]
    pub fn push(&mut self, mv: Move) {
        debug_assert!(self.len < self.data.len(), "MoveListBuf overflow");
        unsafe {
            *self.data.get_unchecked_mut(self.len) = mv;
        }
        self.len += 1;
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Piece {
    pub piece_type: PieceType,
    pub color: Side,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PieceType {
    Pawn = 5,
    Knight = 4,
    Bishop = 3,
    Rook = 2,
    Queen = 1,
    King = 0,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Side {
    White = 0,
    Black = 1,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Board {
    // bitboards[color][piecetype]
    // 0 -> white   1 -> black
    // 0 -> king    1 -> queen  2 -> rook   3 -> bishop 4 -> knight 5 -> pawn
    pub bitboards: [[Bitboard; 6]; 2],
    pub mailbox: [Option<Piece>; 64], // solely used so get_piece can be instant LU
    pub occupied: u64,
    pub moves: u8,
    pub en_passant_target: Option<u8>,
    pub state: u8,
    pub half_moves: u16, // used for 50 move rule; is reset after a capture or pawn move
                         // 5th bit -> white to move,  4th bit -> black long castle right, 3rd -> black short castle right
                         // 2nd bit -> white long castle right, 1st bit -> white short castle right
}

impl Board {
    pub fn is_piece(&self, square: u8) -> bool {
        self.occupied & (1 << square) != 0
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct Undo {
    pub captured_piece: Option<Piece>, // what piece occupied the square before moving
    pub previous_en_passant_target: Option<u8>, // stores where the en_passant target was before moving
    pub previous_state: u8,
}

pub struct MoveContext {
    pub board: Board,
    pub wtime: Duration,
    pub btime: Duration,
    pub winc: Duration,
    pub binc: Duration,
    pub moves_to_go: i32,
}
