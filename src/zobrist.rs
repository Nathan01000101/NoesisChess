use crate::{
    board::{castle_rook_squares, ep_victim_square, get_piece},
    types::{
        self, BLACK_LONG, BLACK_SHORT, Board, Move, PieceType, Undo, WHITE_LONG, WHITE_SHORT,
        WHITE_TO_MOVE,
    },
};

pub struct ZobristTable {
    pieces: [[[u64; 64]; 2]; 6],
    pub black_to_move: u64,
    pub en_passant_file: [u64; 8],
    castling: [u64; 4], // WK, WQ, BK, BQ
}

impl ZobristTable {
    pub fn new() -> Self {
        // "RNG" but will keep same for determinism tests
        let mut rng: u64 = 0x0EE5_150E_5150_ABCD;

        let mut pieces = [[[0u64; 64]; 2]; 6];
        for pt in 0..6 {
            for color in 0..2 {
                for sq in 0..64 {
                    pieces[pt][color][sq] = splitmix64(&mut rng);
                }
            }
        }

        let mut en_passant_file = [0u64; 8];
        for f in 0..8 {
            en_passant_file[f] = splitmix64(&mut rng);
        }

        let mut castling = [0u64; 4];
        for i in 0..4 {
            castling[i] = splitmix64(&mut rng);
        }

        ZobristTable {
            pieces,
            black_to_move: splitmix64(&mut rng),
            en_passant_file,
            castling,
        }
    }

    pub fn hash(&self, board: &Board) -> u64 {
        let mut h: u64 = 0;
        for sq in 0..64 {
            if board.is_piece(sq) {
                let p = get_piece(&board, sq);
                h ^= self.pieces[p.piece_type as usize][p.color as usize][sq as usize];
            }
        }
        if board.state & WHITE_TO_MOVE == 0 {
            h ^= self.black_to_move;
        }

        if let Some(target) = board.en_passant_target {
            h ^= self.en_passant_file[(target % 8) as usize];
        }

        if board.state & WHITE_SHORT != 0 {
            h ^= self.castling[0];
        }
        if board.state & WHITE_LONG != 0 {
            h ^= self.castling[1];
        }
        if board.state & BLACK_SHORT != 0 {
            h ^= self.castling[2];
        }
        if board.state & BLACK_LONG != 0 {
            h ^= self.castling[3];
        }

        h
    }

    pub fn update_hash(&self, hash: u64, board: &Board, mv: Move, undo: &Undo) -> u64 {
        let mut h = hash;
        let from = mv.get_from();
        let to = mv.get_to();
        let move_type = mv.get_flags() & 0b11;

        let after = get_piece(board, to);
        let side = after.color;
        let before_type = if move_type == types::PROMOTION_MOVE {
            PieceType::Pawn
        } else {
            after.piece_type
        };

        h ^= self.pieces[before_type as usize][side as usize][from as usize];
        h ^= self.pieces[after.piece_type as usize][side as usize][to as usize];

        // captured piece (normal, promo-capture, or en passant)
        if let Some(captured) = undo.captured_piece {
            let cap_sq = if move_type == types::EN_PASSANT_MOVE {
                ep_victim_square(from, to)
            } else {
                to
            };
            h ^=
                self.pieces[captured.piece_type as usize][captured.color as usize][cap_sq as usize];
        }

        // castling also moves the rook
        if move_type == types::CASTLE_MOVE {
            let (rook_from, rook_to) = castle_rook_squares(from, to);
            h ^= self.pieces[PieceType::Rook as usize][side as usize][rook_from as usize];
            h ^= self.pieces[PieceType::Rook as usize][side as usize][rook_to as usize];
        }

        // side to move always flips
        h ^= self.black_to_move;

        // en passant file: remove old, add new
        if let Some(prev_ep) = undo.previous_en_passant_target {
            h ^= self.en_passant_file[(prev_ep % 8) as usize];
        }
        if let Some(new_ep) = board.en_passant_target {
            h ^= self.en_passant_file[(new_ep % 8) as usize];
        }

        // castling rights: toggle any that changed
        let (prev, cur) = (undo.previous_state, board.state);
        if (prev & WHITE_SHORT) != (cur & WHITE_SHORT) {
            h ^= self.castling[0];
        }
        if (prev & WHITE_LONG) != (cur & WHITE_LONG) {
            h ^= self.castling[1];
        }
        if (prev & BLACK_SHORT) != (cur & BLACK_SHORT) {
            h ^= self.castling[2];
        }
        if (prev & BLACK_LONG) != (cur & BLACK_LONG) {
            h ^= self.castling[3];
        }

        h
    }
}

// 64 bit hashing
// hashing is reproducable and deterministic
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
