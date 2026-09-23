use std::time::Duration;

pub const WHITE_SHORT:  u8 = 0b00001;
pub const WHITE_LONG:   u8 = 0b00010;
pub const BLACK_SHORT:  u8 = 0b00100;
pub const BLACK_LONG:   u8 = 0b01000;
pub const WHITE_TO_MOVE: u8 = 0b10000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bitboard(pub u64);

impl Bitboard {
    pub const EMPTY: Self = Self(0);

    pub const fn set(&mut self, square: u8){
        self.0 |= 1 << square;
    }

    pub fn get(&self, square:u8) -> bool{
        (self.0 & (1 << square)) != 0
    }
}

pub struct MoveBuf {
    pub data: [u8; 32],
    pub len: usize,
}
impl MoveBuf {
    pub fn new() -> Self { Self { data: [0; 32], len: 0 } }
    
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
    pub data: [(u8, u8); 218],
    pub len: usize,
}
impl MoveListBuf {
    pub fn new() -> Self { Self { data: [(0, 0); 218], len: 0 } }

    #[inline(always)]
    pub fn push(&mut self, from: u8, to: u8) {
        debug_assert!(self.len < self.data.len(), "MoveListBuf overflow");
        unsafe {
            *self.data.get_unchecked_mut(self.len) = (from, to);
        }
        self.len += 1;
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Piece {
    pub piece_type: PieceType,
    pub color: Side
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PieceType {
    Pawn = 5, Knight = 4, Bishop = 3, Rook = 2, Queen = 1, King = 0
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Side {
    White = 0, Black = 1
}


#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Board{
    // bitboards[color][piecetype]
    // 0 -> white   1 -> black
    // 0 -> king    1 -> queen  2 -> rook   3 -> bishop 4 -> knight 5 -> pawn
    pub bitboards: [[Bitboard; 6]; 2],
    pub mailbox: [Option<Piece>; 64], // solely used so get_piece can be instant LU 
    pub occupied: u64,
    pub moves: u8,
    pub en_passant_target: Option<u8>,
    pub state: u8,
    pub half_moves: u16 // used for 50 move rule; is reset after a capture or pawn move
    // 5th bit -> white to move,  4th bit -> black long castle right, 3rd -> black short castle right
    // 2nd bit -> white long castle right, 1st bit -> white short castle right
}

impl Board{
    pub fn is_piece(&self, square: u8) -> bool { self.occupied & (1 << square) != 0}
}


#[derive(Clone, Copy, PartialEq)]
pub struct Move{
    pub from: u8,
    pub to: u8,

}

#[derive(Clone, Copy, PartialEq)]
pub struct Undo{
    pub last_move: Move, // (from, to)
    pub moving_piece_before: Piece,
    pub captured_piece: Option<Piece>, // what piece occupied the square before moving
    pub previous_en_passant_target: Option<u8>, // stores where the en_passant target was before moving
    pub previous_state: u8
}

pub struct MoveContext{
    pub board: Board,
    pub wtime: Duration,
    pub btime: Duration,
    pub winc: Duration,
    pub binc: Duration,
    pub moves_to_go: i32
}