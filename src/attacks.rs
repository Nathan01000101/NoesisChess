use crate::types::{Bitboard, Side};

const FILES: [u64; 8] = [
    0x0101010101010101, // A
    0x0202020202020202, // B
    0x0404040404040404, // C
    0x0808080808080808, // D
    0x1010101010101010, // E
    0x2020202020202020, // F
    0x4040404040404040, // G
    0x8080808080808080, // H
];

const RANKS: [u64; 8] = [
    0x00000000000000FF, // 1
    0x000000000000FF00, // 2
    0x0000000000FF0000, // 3
    0x00000000FF000000, // 4
    0x000000FF00000000, // 5
    0x0000FF0000000000, // 6
    0x00FF000000000000, // 7
    0xFF00000000000000, // 8
];

const DIAGONAL_DIRS: [(i16, i16); 4] = [(-1, 1), (1, 1), (1, -1), (-1, -1)];
const DIAGONAL_MOVES: [[Bitboard; 4]; 64] = build_diagonal_attacks();
pub const BISHOP_MAGIC_TABLE: [u64; 64] = [0b0000000000000100000100000000001000001010000000100010001000000001, 0b0100000000001000000010000000000010000000101000100000010100001000, 0b0100000000010010000010000000010001000001000000100001000000000000, 0b1001100000000100000001000000000010000000100010100000000011001010, 0b0000000000000100000001000110000010001000000100100010000000000000, 0b0100100000001000010010000000101000010000000000000000000010000000, 0b0000000000000100000000001001100000001000010010100000000010000000, 0b0000000000001000110001000100010000001000101000000001000000000100, 0b0000110000010000001000000001010000010000000000001000000100000101, 0b0000100010010000101000001000100000001001000001000100011010001000, 0b0010000001001010000100001010000010000010000000001100000000000100, 0b1000001000000001000001000000010100000010000000000000000001100000, 0b0100100000110100000000100000011000010000000000000100000000000000, 0b0001001100000000000010010000010100100000000100000001000000000000, 0b0000000000000000000001000000011000001000010000110000100000000010, 0b0100010011010101001000010000010000000010101001000010010010000001, 0b1001000100000100001110000100000001101000000001100000110000000000, 0b0000001000001100001000000001000000000001000010000000000100100000, 0b0000000000001000000000000100000100000100010000010000001000000100, 0b0000100000000100110000100000010000000100000000101000000011001000, 0b0000000010000001100000000000010000000000101000000000000010000000, 0b0000001000010001000000000111001000000001000100001000001000000000, 0b0000001000100001000000000000010000000001000010001000101000000000, 0b0000100000010000010000110000000101101100000001000000010001010000, 0b0000110001001000000010001000000001100000001000010000001100000100, 0b0110100000000100000100001000000000100000000000100001001110000000, 0b1000000000000100000100000000100010000001000000000100000110000000, 0b0000000001100110000000000010000000001000000001001000000000100000, 0b0100000000001001000000010000000000000000001100000100000000000000, 0b0000000010001000000000000100000010100010000000110100001000000000, 0b0000100000001100000000010000100000001000010000010011000000000000, 0b0001000000100100000000101101000000000011000000100100001000000000, 0b0000010000100011001000001000100000000000100100100001000000000100, 0b0000000001010010001100000000001000000000100001000000100001000011, 0b1100000011000001000000001000001000010100010000000100010000000000, 0b0010000000000000010000000000100010100000010000100000001000000001, 0b0001000001000000000001100000011000000000000001010010000010000001, 0b1000000000010000000000100000100010100000000001100000000010001000, 0b0100000001010000001010001000000100001000000001011000010000000000, 0b0010000010001010000000100000001000000000000000010100000011000100, 0b0000000000000110000100000001000000001001000000001100011000000000, 0b0010000000001000100001100001000000100010000000100001000000000000, 0b0000000000000001001000000010010000101000000000000001000000000000, 0b0000000001000010000000000010000000011000000001000010000100000000, 0b1100100000001010000010000100000100000000010000100000010000000000, 0b0000000100110001010001010010000100000001000000000000011000000000, 0b0000000000010100001001000000100010010010001000100000110000000000, 0b0000000000000001000100110100001100000010000000001000000100001000, 0b0000010000000000010001000000001000001000010000010000000000000000, 0b0100000000000010001000100000100000000010000010000000010011000000, 0b0100000000000000000010000010000010000100000100000001000000000000, 0b0000000011000000010000000000001000000101000011000000100000000001, 0b1000000100000101000101100000010001010000010001000000000000000010, 0b0100110000000000001000000010110000001101000000100000000000000000, 0b1100000000001000001000011000000100000010000110100000000000000000, 0b0001010000010011000010000010010100000011000000100000000000000000, 0b0010000000100000010000010110001000001000000010100100000010000000, 0b1010000000000000000000100000011000000000100001001000010010000000, 0b0000000000000000000001000000010011100111000010000001100000010000, 0b0000000000000100000000000000000000000000010000100000001000000000, 0b0010000100000100000000100001000001010010000111100000001000000000, 0b0000000000000000100000101010000000010100001100000011000010001000, 0b0000010001001000000001000101100000000010000001000000010000000000, 0b0000010000001000000000100011100000011000000000010000000100010101];
pub const BISHOP_MASKS: [u64; 64] = build_bishop_masks();
pub const BISHOP_SHIFTS: [u32; 64] = build_bishop_shifts();
pub const DIAGONAL_VALID_MOVES: [[Bitboard; 512]; 64] = calculate_valid_diagonal_attacks();

const STRAIGHT_DIRS: [(i16, i16); 4] = [(1, 0), (-1, 0), (0, -1), (0, 1)];
const STRAIGHT_MOVES: [[Bitboard; 4]; 64] = build_straight_attacks();
pub const ROOK_MAGIC_TABLE: [u64; 64] = [0b0010000010000000000000000010001000011001110000000000000010000000, 0b0000001011000000000000000010000000000010010000000001000000000000, 0b0000100110000000001000000000000010000001000010000001000000000000, 0b0000000010000000000001100001000000000001100000000000100000000000, 0b0000000110000000000000101000000001010100000000000000100000000000, 0b0010001000000000000000100000000010010000010010000000100100000100, 0b0000000010000000000101010000000000000010000000000000000010000000, 0b0000001000000000000000000100000000100010100001000000001000000101, 0b0000101000100000100000000000000001000000000000000010000010000000, 0b0001000010100010010000000001000000000000010000001110000000000001, 0b0000010000000110100000000001000000000000100010000010000000000000, 0b0010000001110001000000000000100000100001000000001101000000000000, 0b1000100100000101000000000000100000000001000000000001000000001100, 0b0100100000000001000000000000011000001000000001000000000100000000, 0b0000000000000110000000000000010010000001010000100000000000001000, 0b0000000010001001000000000000100010000000110000010000000000000010, 0b0000011010000000100000001000000000000100001000000100000000001000, 0b0000000100000000100000001000000000100000000000010100000000000001, 0b0100000000000010000000100000000000100000010001001000000000010100, 0b0101000000000010000000100000000000011000000100000100000001100000, 0b0000000001000001000000010000000000000100000010000000000010010000, 0b0001000000000010100000001000000000000110000000000100010000000000, 0b0000000100000000000111000000000010000001000010000101000000100010, 0b0000000000010000010100100000000000000000100001110000000001000100, 0b0000000000010100010000000000000010000000000000011000000000100100, 0b0000000011010000001000000000000001000000000100000000010001000000, 0b0000000000000000001000100000001000000000000100000100000010000000, 0b0101000100000000000100000000000100000000001010010000000000100001, 0b0000000000000001000000100001000100000000000001000000100000000000, 0b1000000000000001000000100000000010000000000001000000000010000000, 0b0000001000000001000010000010010000000000000100000000001000100001, 0b0100000000000000000000010001011000000000001000001000000001000100, 0b0000000101000000001000000100000000000000100000000000001010000000, 0b1000000100010000000000000010000000000000010000000000000001000010, 0b0001000000000000010000100010000000000001000000000101000100000000, 0b0001000000000000000110000010000100000011000000000001000000000000, 0b0000001010000001010010000000000010000001100000000000010000000000, 0b0000001000001100100000000110010000000000100000000000001000000001, 0b1000000000000010100000010000001000000100000000000000100011010000, 0b0010100000000000101000000100010000000110000000000000000010000001, 0b0011000001000000000000000010000010000000010000001000000000000000, 0b1000001000010000000001000100000000100000000000000100000000000000, 0b0000000000000000001000000000001100000000100100010000000001000001, 0b0000000001000000000100000000000000100011000000010000000000001000, 0b0000001000001000000000001001000100000000010001010000000000001000, 0b0000000000000110000000000001000000001001010000100000000000001100, 0b0000000000001000000100100000000000000001000000001000000010000000, 0b0000000000000001000000000000100110100000110000010000000000000010, 0b0000010000000000010000000010000010000001000000100000001000000000, 0b0000000000000000010000000000000100100000000000001000000010000000, 0b0000000000000011000000000010000000000100010000000001000100000000, 0b0000000001000000000100000000000000001000000000001000010010000000, 0b0000000000011010000000000101000000100001100010000000011000000000, 0b0000000000100001000100000100000000000100001000000000100000000001, 0b1000000001000000000100100000000100001000000100001000010000000000, 0b0000000110000001000001000100010010000100000000110000001000000000, 0b0000000000000000010001001000000000000001000000000001000010100001, 0b0100100000000000010000000000000101111000011000001000001100000001, 0b0000000100000000100000000100000000100010000000000010101010010010, 0b0000000000000110000000000010000000010100000100000000100001000010, 0b0000000000000001000000000000100000000000000101000011000001011011, 0b1010010000000001000000000000001001000100000000000000100000000101, 0b0000001000000001000000010001000000000000110010000000001000000100, 0b0010001000000001000000000000100001000010000000001000000000101001];
pub const ROOK_MASKS: [u64; 64] = build_rook_masks();
pub const ROOK_SHIFTS: [u32; 64] = build_rook_shifts();
// I know im on a watchlist somewhere for this.
#[allow(long_running_const_eval)]
pub const STRAIGHT_VALID_MOVES: [[Bitboard; 4096]; 64] = calculate_valid_straight_attacks();

const KNIGHT_OFFSETS: [(i16, i16); 8] = [
    (-2, -1), (-2, 1), (-1, -2), (-1, 2),
    (1, -2), (1, 2), (2, -1), (2, 1),
];
pub const KNIGHT_MOVES: [Bitboard; 64] = build_knight_attacks();

pub const PAWN_MOVES: [[Bitboard; 64]; 2] = build_pawn_attacks();
pub const PAWN_PASSED_MASKS: [[u64; 64]; 2] = build_pawn_forward_masks();
pub const PAWN_DOUBLED_MASKS: [[u64; 64]; 2] = build_pawn_doubled_masks();
pub const PAWN_ISOLATED_MASKS: [u64; 64] = build_pawn_isolated_masks();

const KING_OFFSETS: [(i16, i16); 8] = [
    (-1, -1), (-1, 0), (-1, 1), (0, -1),
    (0, 1), (1, -1), (1, 0), (1, 1),
];
pub const KING_MOVES: [Bitboard; 64] = build_king_attacks();

enum DiagonalDirections{
    SouthEast = 0, NorthEast = 1, NorthWest = 2, SouthWest = 3
}

enum Directions{
    North = 0, South = 1, West = 2, East = 3
}


pub const fn in_bounds_row_col(row: i16, col:i16) -> bool{
    row >= 0 && row < 8 && col >= 0 && col < 8
}


const fn get_subset(i: u64, mask: u64) -> u64{
    let mut result = 0u64;
    let mut bits = mask;
    let mut counter = 0;

    while bits != 0{
        let bit = bits.trailing_zeros();
        if i & (1 << counter) != 0{
            result |= 1 << bit;
        }
        bits ^= 1 << bit;
        counter += 1;
    }

    result
}

const fn ray_march_bitboard(origin: u8, direction: (i16, i16), occupied: u64) -> u64{
    let mut bb = match direction {
        (-1,  1)    => DIAGONAL_MOVES[origin as usize][DiagonalDirections::SouthEast as usize],
        ( 1,  1)    => DIAGONAL_MOVES[origin as usize][DiagonalDirections::NorthEast as usize],
        ( 1, -1)    => DIAGONAL_MOVES[origin as usize][DiagonalDirections::NorthWest as usize],
        (-1, -1)    => DIAGONAL_MOVES[origin as usize][DiagonalDirections::SouthWest as usize],
        ( 1,  0)    => STRAIGHT_MOVES[origin as usize][Directions::North as usize],
        (-1,  0)    => STRAIGHT_MOVES[origin as usize][Directions::South as usize],
        ( 0, -1)    => STRAIGHT_MOVES[origin as usize][Directions::West as usize],
        ( 0,  1)    => STRAIGHT_MOVES[origin as usize][Directions::East as usize],
        _=> panic!("Error, tried to ray march an invalid direction ")
    };
    let delta = direction.0 * 8 + direction.1;
    let mut valid = 0u64;

    
    while bb.0 != 0{
        let bit_position = if delta > 0 { bb.0.trailing_zeros()} else {63 - bb.0.leading_zeros()};

        if occupied & (1 << bit_position) != 0{
            valid |= 1 << bit_position;
            return valid;
        }
            
        valid |= 1 << bit_position;
        bb.0 ^= 1 << bit_position;
    }
    
    valid
}


// STRAIGHT MOVE LOGIC
const fn get_straight_mask(square: u8) -> u64{
    let row = (square / 8) as i16;
    let col = (square - (row as u8) * 8) as i16;
    let mut mask = 0u64;

    let mut i = 0;
    while i < 4{
        let mut step = 1;
        let dir = STRAIGHT_DIRS[i];
        loop {
            let r = row + dir.0 * step;
            let c = col + dir.1 * step;

            if !in_bounds_row_col(r, c) {
                break; // walked off board
            }
            if !in_bounds_row_col(r + dir.0, c + dir.1) {
                break; // this square IS the edge
            }

            mask |= 1 << (r * 8 + c);
            step += 1;
        }
        i += 1;
    }

    mask
}

const fn build_rook_masks() -> [u64; 64] {
    let mut masks = [0u64; 64];
    let mut sq = 0;
    while sq < 64 {
        masks[sq] = get_straight_mask(sq as u8);
        sq += 1;
    }
    masks
}

const fn build_rook_shifts() -> [u32; 64] {
    let mut shifts = [0u32; 64];
    let mut sq = 0;
    while sq < 64 {
        shifts[sq] = 64 - ROOK_MASKS[sq].count_ones();
        sq += 1;
    }
    shifts
}

const fn compute_rook_attacks(square: u8, occupancy: u64) -> u64{
    let mut bb = 0u64;
    let mut i = 0;
    while i < 4{
        bb |= ray_march_bitboard(square, STRAIGHT_DIRS[i], occupancy);
        i += 1;
    }
    bb    
}

const fn build_straight_attacks() -> [[Bitboard; 4]; 64]{
    let mut bitboards = [[Bitboard::EMPTY; 4]; 64];
    let mut i = 0;
    while i < 64{
        let mut j = 0;
        let row = i / 8;
        let col = i - row * 8;
        while j < 4{
            let mut stopped = false;
            let mut step = 1;

            while !stopped{
                let new_r = row + STRAIGHT_DIRS[j].0 * step;
                let new_c = col + STRAIGHT_DIRS[j].1 * step;

                if in_bounds_row_col(new_r, new_c){
                    bitboards[i as usize][j as usize].set((new_r * 8 + new_c) as u8);
                }else{
                    stopped = true;
                }

                step += 1;
            }

            j += 1;
        }

        i += 1;
    }

    bitboards
}

const fn calculate_valid_straight_attacks() -> [[Bitboard; 4096]; 64]{
    let mut attacks = [[Bitboard::EMPTY; 4096]; 64];

    // for each square
    let mut square = 0;
    while square < 64{
        
        // for each blocker config.
        let mut i = 0;
        let mask = get_straight_mask(square as u8);
        let size = mask.count_ones();
        let shift = 64 - size;
        let num_subsets = 1u64 << size;
        while i < num_subsets{
            let occupancy = get_subset(i as u64, mask);
            let square_attacks = compute_rook_attacks(square as u8, occupancy);
            let key = ((occupancy.wrapping_mul(ROOK_MAGIC_TABLE[square])) >> shift) as usize;
            
            attacks[square][key] = Bitboard(square_attacks);
            i += 1
        }
        
        square += 1;
    }

    attacks
}


// DIAGONAL MOVE LOGIC
const fn get_diagonal_mask(square: u8) -> u64{
    let row = (square / 8) as i16;
    let col = (square - (row as u8) * 8) as i16;
    let mut mask = 0u64;

    let mut i = 0;
    while i < 4{
        let mut step = 1;
        let dir = DIAGONAL_DIRS[i];
        loop {
            let r = row + dir.0 * step;
            let c = col + dir.1 * step;

            if !in_bounds_row_col(r, c) {
                break; // walked off board
            }
            if !in_bounds_row_col(r + dir.0, c + dir.1) {
                break; // this square IS the edge — stop before including it
            }

            mask |= 1 << (r * 8 + c);
            step += 1;
        }
        i += 1;
    }

    mask
}

const fn build_bishop_masks() -> [u64; 64] {
    let mut masks = [0u64; 64];
    let mut sq = 0;
    while sq < 64 {
        masks[sq] = get_diagonal_mask(sq as u8);
        sq += 1;
    }
    masks
}

const fn build_bishop_shifts() -> [u32; 64] {
    let mut shifts = [0u32; 64];
    let mut sq = 0;
    while sq < 64 {
        shifts[sq] = 64 - BISHOP_MASKS[sq].count_ones();
        sq += 1;
    }
    shifts
}

const fn compute_bishop_attacks(square: u8, occupancy: u64) -> u64{
    let mut bb = 0u64;
    let mut i = 0;
    while i < 4{
        bb |= ray_march_bitboard(square, DIAGONAL_DIRS[i], occupancy);
        i += 1;
    }
    bb      
}

const fn build_diagonal_attacks() -> [[Bitboard; 4]; 64]{
    let mut bitboards = [[Bitboard::EMPTY; 4]; 64];
    let mut i = 0;
    while i < 64{
        let mut j = 0;
        let row = i / 8;
        let col = i - row * 8;
        while j < 4{
            let mut stopped = false;
            let mut step = 1;

            while !stopped{
                let new_r = row + DIAGONAL_DIRS[j].0 * step;
                let new_c = col + DIAGONAL_DIRS[j].1 * step;

                if in_bounds_row_col(new_r, new_c){
                    bitboards[i as usize][j as usize].set((new_r * 8 + new_c) as u8);
                }else{
                    stopped = true;
                }

                step += 1;
            }

            j += 1;
        }

        i += 1;
    }

    bitboards
}

const fn calculate_valid_diagonal_attacks() -> [[Bitboard; 512]; 64]{
    let mut attacks = [[Bitboard::EMPTY; 512]; 64];

    // for each square
    let mut square = 0;
    while square < 64{
        
        // for each blocker config.
        let mut i = 0;
        let mask = get_diagonal_mask(square as u8);
        let size = mask.count_ones();
        let shift = 64 - size;
        let num_subsets = 1u64 << size;
        while i < num_subsets{
            let occupancy = get_subset(i as u64, mask);
            let square_attacks = compute_bishop_attacks(square as u8, occupancy);
            let key = ((occupancy.wrapping_mul(BISHOP_MAGIC_TABLE[square])) >> shift) as usize;
            
            attacks[square][key] = Bitboard(square_attacks);
            i += 1
        }
        
        square += 1;
    }

    attacks
}


const fn build_pawn_attacks() -> [[Bitboard; 64]; 2]{
    let mut bitboards = [[Bitboard::EMPTY; 64]; 2];    
    let mut i = 8;
    
    //white pawns
    while i < 56{

        if i % 8 != 0{
            bitboards[0][i].0 |= 1 << (i + 7);
        }
        if i % 8 != 7{
            bitboards[0][i].0 |= 1 << (i + 9);
        }

        i += 1;
    }
    
    i = 8;

    //black pawns
    while i < 56{

        if i % 8 != 0{
            bitboards[1][i].0 |= 1 << (i - 9);
        }
        if i % 8 != 7{
            bitboards[1][i].0 |= 1 << (i - 7);
        }
        i += 1;
    }

    bitboards
}

const fn build_pawn_forward_masks() -> [[u64; 64]; 2]{
    let mut bitboards = [[0u64; 64];2];

    let mut square = 0;
    while square < 64{
        bitboards[0][square] = get_pawn_forward_mask(square as u8, Side::White as usize);
        bitboards[1][square] = get_pawn_forward_mask(square as u8, Side::Black as usize);
        square += 1;
    }

    bitboards
}

const fn build_pawn_doubled_masks() -> [[u64; 64]; 2]{
    let mut bitboards = [[0u64; 64];2];

    let mut square = 0;
    while square < 64{
        bitboards[0][square] = get_pawn_doubled_mask(square as u8, Side::White as usize);
        bitboards[1][square] = get_pawn_doubled_mask(square as u8, Side::Black as usize);
        square += 1;
    }

    bitboards
}

const fn build_pawn_isolated_masks() -> [u64; 64]{
    let mut bitboards = [0u64; 64];

    let mut square = 0;
    while square < 64{
        bitboards[square] = get_pawn_isolated_mask(square as u8);
        bitboards[square] = get_pawn_isolated_mask(square as u8);
        square += 1;
    }

    bitboards
}

const fn build_king_attacks() -> [Bitboard; 64]{
    let mut bitboards = [Bitboard::EMPTY; 64];    
    let mut i = 0;
    while i < 64{
        let r = i / 8;
        let c = i - r*8;

        let mut offset_counter = 0;
        while offset_counter < KING_OFFSETS.len(){
            let new_r = r + KING_OFFSETS[offset_counter].0;
            let new_c = c + KING_OFFSETS[offset_counter].1;

            if in_bounds_row_col(new_r, new_c){
                bitboards[i as usize].set((new_r*8 + new_c) as u8);
            }
            offset_counter += 1;
        }
        i += 1;
    }
    bitboards
}

const fn build_knight_attacks() -> [Bitboard; 64]{
    let mut bitboards = [Bitboard::EMPTY; 64];    
    let mut i = 0;
    while i < 64{
        let r = i / 8;
        let c = i - r*8;

        let mut offset_counter = 0;
        while offset_counter < KNIGHT_OFFSETS.len(){
            let new_r = r + KNIGHT_OFFSETS[offset_counter].0;
            let new_c = c + KNIGHT_OFFSETS[offset_counter].1;

            if in_bounds_row_col(new_r, new_c){
                bitboards[i as usize].set((new_r*8 + new_c) as u8);
            }
            offset_counter += 1;
        }
        i += 1;
    }
    bitboards
}


// DOES NOT DISCRIMINATE AGAINST FRIENDLY PIECES.
pub fn get_bishop_moves(blockers: u64, square: u8) -> u64{
    let mask = BISHOP_MASKS[square as usize];
    let shift = BISHOP_SHIFTS[square as usize];
    let key = (((blockers & mask).wrapping_mul(BISHOP_MAGIC_TABLE[square as usize])) >> shift) as usize;
    DIAGONAL_VALID_MOVES[square as usize][key].0
}

// DOES NOT DISCRIMINATE AGAINST FRIENDLY PIECES.
pub fn get_rook_moves(blockers: u64, square: u8) -> u64{
    let mask = ROOK_MASKS[square as usize];
    let shift = ROOK_SHIFTS[square as usize];
    let key = (((blockers & mask).wrapping_mul(ROOK_MAGIC_TABLE[square as usize])) >> shift) as usize;
    STRAIGHT_VALID_MOVES[square as usize][key].0
}

// returns a bitboard with all the squares in front of the pawn and adjacent columns.
const fn get_pawn_forward_mask(square: u8, side: usize) -> u64{
    let file = (square % 8) as usize;
    let mut mask = FILES[file] 
        | if file != 7 {FILES[file+1]} else {0} 
        | if file != 0 {FILES[file-1]} else {0};
    //crop below pawn
    let mut rank = (square / 8) as usize;
    loop{
        mask &= !RANKS[rank]; // mask cannot include this rank
        if rank == 0 || rank == 7 {break;}
        if side == 0 {rank -= 1;} else {rank += 1;}
    }
    mask
}

// returns a bitboard with all the squares in directly front of the pawn.
const fn get_pawn_doubled_mask(square: u8, side: usize) -> u64{
    let mut mask = FILES[(square % 8) as usize];
    //crop below pawn
    let mut rank = (square / 8) as usize;
    loop{
        mask &= !RANKS[rank]; // mask cannot include this rank
        if rank == 0 || rank == 7 {break;}
        if side == 0 {rank -= 1;} else {rank += 1;}
    }
    mask
}

// returns a bitboard with all the squares in adjacent files of square
const fn get_pawn_isolated_mask(square: u8) -> u64{
    return if square % 8 != 7 {FILES[(square % 8 + 1) as usize]} else {0}
    | if square % 8 != 0 {FILES[(square % 8 - 1) as usize]} else {0}
}