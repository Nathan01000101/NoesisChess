use crate::board::get_piece;

use crate::board::{castle_rook_squares, ep_victim_square};
use crate::types::{
    Board, CASTLE_MOVE, EN_PASSANT_MOVE, Move, PROMOTION_MOVE, PieceType, Side, Undo,
};
const HIDDEN_SIZE: usize = 256;
const SCALE: i32 = 400;
const QA: i16 = 255;
const QB: i16 = 64;

pub static NNUE: Network = unsafe { std::mem::transmute(*include_bytes!("../nets/quantised.bin")) };

#[inline]
/// Square Clipped ReLU - Activation Function.
/// Note that this takes the i16s in the accumulator to i32s.
/// Range is 0.0 .. 1.0 (in other words, 0 to QA*QA quantized).
fn screlu(x: i16) -> i32 {
    let y = i32::from(x).clamp(0, i32::from(QA));
    y * y
}

/// This is the quantised format that bullet outputs.
#[repr(C)]
pub struct Network {
    /// Column-Major `HIDDEN_SIZE x 768` matrix.
    /// Values have quantization of QA.
    feature_weights: [Accumulator; 768],
    /// Vector with dimension `HIDDEN_SIZE`.
    /// Values have quantization of QA.
    feature_bias: Accumulator,
    /// Column-Major `1 x (2 * HIDDEN_SIZE)`
    /// matrix, we use it like this to make the
    /// code nicer in `Network::evaluate`.
    /// Values have quantization of QB.
    output_weights: [i16; 2 * HIDDEN_SIZE],
    /// Scalar output bias.
    /// Value has quantization of QA * QB.
    output_bias: i16,
}

// the SIMD path eats 16 i16s per step, so the hidden layer must divide evenly
const _: () = assert!(HIDDEN_SIZE % 16 == 0);

impl Network {
    /// Calculates the output of the network, starting from the already
    /// calculated hidden layer (done efficiently during makemoves).
    pub fn evaluate(&self, us: &Accumulator, them: &Accumulator) -> i32 {
        // sum of screlu(acc) * weight over both perspectives, at QA * QA * QB
        let mut output = self.screlu_dot(us, them);

        // Reduce quantization from QA * QA * QB to QA * QB.
        output /= i32::from(QA);

        // Add bias.
        output += i32::from(self.output_bias);

        // Apply eval scale.
        output *= SCALE;

        // Remove quantisation altogether.
        output /= i32::from(QA) * i32::from(QB);

        output
    }

    #[inline]
    fn screlu_dot(&self, us: &Accumulator, them: &Accumulator) -> i32 {
        #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
        {
            // SAFETY: avx2 is enabled at compile time (target-cpu=x86-64-v3)
            unsafe { screlu_dot_avx2(&us.vals, &them.vals, &self.output_weights) }
        }
    }
}

/// Reference version: screlu(x) * w, accumulated in i32.
/// Used as the fallback on non-AVX2 builds, and as the ground truth in tests.
#[allow(dead_code)]
fn screlu_dot_scalar(
    us: &[i16; HIDDEN_SIZE],
    them: &[i16; HIDDEN_SIZE],
    weights: &[i16; 2 * HIDDEN_SIZE],
) -> i32 {
    let mut output: i32 = 0;
    for (&input, &weight) in us.iter().zip(&weights[..HIDDEN_SIZE]) {
        output = output.wrapping_add(screlu(input) * i32::from(weight));
    }
    for (&input, &weight) in them.iter().zip(&weights[HIDDEN_SIZE..]) {
        output = output.wrapping_add(screlu(input) * i32::from(weight));
    }
    output
}

/// Lizard SCReLU: computes clamp(x)^2 * w as (clamp(x) * w) * clamp(x).
#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
#[inline]
unsafe fn screlu_dot_avx2(
    us: &[i16; HIDDEN_SIZE],
    them: &[i16; HIDDEN_SIZE],
    weights: &[i16; 2 * HIDDEN_SIZE],
) -> i32 {
    use std::arch::x86_64::*;
    const LANES: usize = 16; // i16s per 256-bit register

    unsafe {
        let zero = _mm256_setzero_si256();
        let qa = _mm256_set1_epi16(QA);
        let mut sum = _mm256_setzero_si256(); // 8 x i32

        for (acc, w) in [
            (us, &weights[..HIDDEN_SIZE]),
            (them, &weights[HIDDEN_SIZE..]),
        ] {
            let mut i = 0;
            while i < HIDDEN_SIZE {
                let x = _mm256_loadu_si256(acc.as_ptr().add(i) as *const __m256i);
                let wv = _mm256_loadu_si256(w.as_ptr().add(i) as *const __m256i);

                let y = _mm256_min_epi16(_mm256_max_epi16(x, zero), qa); // clamp(x, 0, QA)
                let yw = _mm256_mullo_epi16(y, wv); // y * w, fits in i16
                let prod = _mm256_madd_epi16(yw, y); // (y * w) * y, pairs summed into i32

                sum = _mm256_add_epi32(sum, prod);
                i += LANES;
            }
        }

        // horizontal sum of the 8 i32 lanes
        let lo = _mm256_castsi256_si128(sum);
        let hi = _mm256_extracti128_si256::<1>(sum);
        let s = _mm_add_epi32(lo, hi); // 4 lanes
        let s = _mm_add_epi32(s, _mm_shuffle_epi32::<0b01_00_11_10>(s)); // 2 lanes
        let s = _mm_add_epi32(s, _mm_shuffle_epi32::<0b10_11_00_01>(s)); // 1 lane
        _mm_cvtsi128_si32(s)
    }
}

/// A column of the feature-weights matrix.
/// Note the `align(64)`.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct Accumulator {
    vals: [i16; HIDDEN_SIZE],
}

impl Accumulator {
    /// Initialised with bias so we can just efficiently
    /// operate on it afterwards.
    pub fn new(net: &Network) -> Self {
        net.feature_bias
    }

    /// Add a feature to an accumulator.
    pub fn add_feature(&mut self, feature_idx: usize, net: &Network) {
        for (i, d) in self
            .vals
            .iter_mut()
            .zip(&net.feature_weights[feature_idx].vals)
        {
            *i += *d
        }
    }

    /// Remove a feature from an accumulator.
    pub fn remove_feature(&mut self, feature_idx: usize, net: &Network) {
        for (i, d) in self
            .vals
            .iter_mut()
            .zip(&net.feature_weights[feature_idx].vals)
        {
            *i -= *d
        }
    }
}

#[derive(Clone, Copy)]
pub struct AccumulatorPair {
    pub white: Accumulator,
    pub black: Accumulator,
}

impl AccumulatorPair {
    pub fn new() -> Self {
        AccumulatorPair {
            white: Accumulator::new(&NNUE),
            black: Accumulator::new(&NNUE),
        }
    }

    /// Adds a piece to both perspectives.
    #[inline]
    pub fn add(&mut self, color: Side, pt: PieceType, sq: u8) {
        let (w, b) = feature_indices(color, pt, sq);
        self.white.add_feature(w, &NNUE);
        self.black.add_feature(b, &NNUE);
    }

    /// Removes a piece from both perspectives.
    #[inline]
    pub fn remove(&mut self, color: Side, pt: PieceType, sq: u8) {
        let (w, b) = feature_indices(color, pt, sq);
        self.white.remove_feature(w, &NNUE);
        self.black.remove_feature(b, &NNUE);
    }
}

/// Input index of a piece in (white's perspective, black's perspective).
/// Each side sees its own pieces in the first 384 inputs, and black sees the board flipped.
#[inline]
fn feature_indices(color: Side, pt: PieceType, sq: u8) -> (usize, usize) {
    let pt = bullet_piece_index(pt);
    let sq = sq as usize;
    let (white_offset, black_offset) = if color == Side::White {
        (0, 384)
    } else {
        (384, 0)
    };
    (
        white_offset + 64 * pt + sq,
        black_offset + 64 * pt + (sq ^ 56),
    )
}

/// Updates `acc` (already a copy of the parent's accumulators) for `mv`.
/// `board` is the position *after* make_move, exactly like update_hash.
pub fn apply_move(acc: &mut AccumulatorPair, board: &Board, mv: Move, undo: &Undo) {
    let from = mv.get_from();
    let to = mv.get_to();
    let move_type = mv.get_flags() & 0b11;

    // whatever sits on `to` now is the moved piece (already promoted if it was a promotion)
    let after = get_piece(board, to);
    let side = after.color;
    let before_type = if move_type == PROMOTION_MOVE {
        PieceType::Pawn
    } else {
        after.piece_type
    };

    // moving piece: off `from` as it was, onto `to` as it is now
    acc.remove(side, before_type, from);
    acc.add(side, after.piece_type, to);

    // captured piece (normal, promo-capture, or en passant)
    if let Some(captured) = undo.captured_piece {
        let cap_sq = if move_type == EN_PASSANT_MOVE {
            ep_victim_square(from, to)
        } else {
            to
        };
        acc.remove(captured.color, captured.piece_type, cap_sq);
    }

    // castling also moves the rook
    if move_type == CASTLE_MOVE {
        let (rook_from, rook_to) = castle_rook_squares(from, to);
        acc.remove(side, PieceType::Rook, rook_from);
        acc.add(side, PieceType::Rook, rook_to);
    }
} // static eval in centipawns from the side to move's point of view.
pub fn evaluate(accumulators: &AccumulatorPair, side_to_move: Side) -> i32 {
    if side_to_move == Side::White {
        NNUE.evaluate(&accumulators.white, &accumulators.black)
    } else {
        NNUE.evaluate(&accumulators.black, &accumulators.white)
    }
}

// returns a pair of accumulators (white + black) with all features of the board
pub fn get_accumulator_pair(board: &Board) -> AccumulatorPair {
    let mut white = Accumulator::new(&NNUE);
    let mut black = Accumulator::new(&NNUE);
    for sq in 0..64u8 {
        if !board.is_piece(sq) {
            continue;
        }
        let piece = get_piece(board, sq);
        let pt = bullet_piece_index(piece.piece_type);
        let sq = sq as usize;

        // each perspective sees its own pieces in the first 384 inputs,
        // and black's perspective sees the board flipped vertically
        let (white_offset, black_offset) = if piece.color == Side::White {
            (0, 384)
        } else {
            (384, 0)
        };

        white.add_feature(white_offset + 64 * pt + sq, &NNUE);
        black.add_feature(black_offset + 64 * pt + (sq ^ 56), &NNUE);
    }
    AccumulatorPair {
        white: white,
        black: black,
    }
}

// bullet's piece order, independent of how PieceType is declared
fn bullet_piece_index(pt: PieceType) -> usize {
    match pt {
        PieceType::Pawn => 0,
        PieceType::Knight => 1,
        PieceType::Bishop => 2,
        PieceType::Rook => 3,
        PieceType::Queen => 4,
        PieceType::King => 5,
    }
}
