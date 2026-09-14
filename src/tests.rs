

#[cfg(test)]
mod make_unmake_tests {
    use crate::types::{MoveListBuf, WHITE_TO_MOVE, Board, Side};
    use crate::board::{new_board, make_move, undo_move};
    use crate::movegen::get_all_moves;

    /// Walks the move tree to `depth`, asserting board equality after every make/unmake.
    /// Returns the node count (a perft result) — useful for comparing against known values.
    fn perft_with_undo_check(board: &mut Board, depth: usize) -> u64 {
        if depth == 0 {
            return 1;
        }

        let side = if board.state & WHITE_TO_MOVE != 0 { Side::White } else { Side::Black };
        let mut move_list: MoveListBuf = MoveListBuf::new();
        get_all_moves(board, side, &mut move_list);
        let mut nodes = 0u64;

        for i in 0..move_list.len {
            // snapshot the board BEFORE making the move
            let before = *board;

            let undo = make_move(board, move_list.data[i].0, move_list.data[i].1);
            nodes += perft_with_undo_check(board, depth - 1);
            undo_move(board, undo);

            // board must be byte-identical to the snapshot
            assert_eq!(
                *board, before,
                "Board not restored after make/unmake of move {:?} -> {:?} at depth {}",
                move_list.data[i].0, move_list.data[i].1, depth
            );
        }

        nodes
    }

    #[test]
    fn roundtrip_from_starting_position_depth_1() {
        let mut board = new_board();  // adjust to your constructor
        let nodes = perft_with_undo_check(&mut board, 1);
        // Starting position has 20 legal moves
        assert_eq!(nodes, 20, "Starting position should have 20 legal moves");
    }

    #[test]
    fn roundtrip_from_starting_position_depth_2() {
        let mut board = new_board();
        let nodes = perft_with_undo_check(&mut board, 2);
        assert_eq!(nodes, 400, "Depth 2 from start should be 400 nodes");
    }

    #[test]
    fn roundtrip_from_starting_position_depth_3() {
        let mut board = new_board();
        let nodes = perft_with_undo_check(&mut board, 3);
        assert_eq!(nodes, 8902, "Depth 3 from start should be 8902 nodes");
    }
    
    #[test]
    fn roundtrip_from_starting_position_depth_4() {
        let mut board = new_board();
        let nodes = perft_with_undo_check(&mut board, 4);
        assert_eq!(nodes, 197_281, "Depth 4 from start should be 197,281 nodes");
    }

    #[test]
    fn roundtrip_from_starting_position_depth_5() {
        let mut board = new_board();
        let nodes = perft_with_undo_check(&mut board, 5);
        assert_eq!(nodes, 4_865_609, "Depth 5 from start should be 4,865,609 nodes");
    }

    #[test]
    fn roundtrip_from_starting_position_depth_6() {
        let mut board = new_board();
        let nodes = perft_with_undo_check(&mut board, 6);
        assert_eq!(nodes, 119_060_324, "Depth 6 from start should be 119,060,324 nodes");
    }

    #[test]
    fn roundtrip_from_starting_position_depth_7() {
        let mut board = new_board();
        let nodes = perft_with_undo_check(&mut board, 7);
        assert_eq!(nodes, 3_195_901_860, "Depth 7 from start should be 3,195,901,860 nodes");
    }

    fn divide_perft(board: &mut Board, depth: usize) {
        let side = if board.state & WHITE_TO_MOVE != 0 { Side::White } else { Side::Black };
        let mut move_list: MoveListBuf = MoveListBuf::new();
        get_all_moves(board, side, &mut move_list);
        let mut total = 0u64;
        for i in 0..move_list.len {
            let undo = make_move(board, move_list.data[i].0, move_list.data[i].1);
            let nodes = perft_with_undo_check(board, depth - 1);
            undo_move(board, undo);
            println!("{:?} -> {:?}: {}", move_list.data[i].0, move_list.data[i].1, nodes);
            total += nodes;
        }
        println!("total: {}", total);
    }

    #[test]
    fn divide_depth_5() {
        let mut board = new_board();
        divide_perft(&mut board, 5);
    }
}

#[cfg(test)]
mod fen_tests {
    use crate::board::{from_fen, to_fen};

    fn round_trip(fen: &str) {
        let board = from_fen(fen);
        let out = to_fen(&board);

        let orig: Vec<&str> = fen.split(' ').collect();
        let got: Vec<&str> = out.split(' ').collect();

        assert_eq!(orig[0], got[0], "placement mismatch for {fen}");
        assert_eq!(orig[1], got[1], "side to move mismatch for {fen}");
        assert_eq!(orig[2], got[2], "castling rights mismatch for {fen}");
        assert_eq!(orig[3], got[3], "en passant mismatch for {fen}");
    }

    #[test]
    fn round_trip_start_position() {
        round_trip("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
    }

    #[test]
    fn round_trip_sparse_board() {
        // fully-empty ranks in the middle, trailing empty squares on the last rank
        round_trip("4k3/8/8/8/8/8/8/4K3 w - - 0 1");
    }

    #[test]
    fn round_trip_en_passant_white_to_move() {
        // classic example: black just played ...d5, ep target d6
        round_trip("rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 3");
    }

    #[test]
    fn round_trip_en_passant_black_to_move() {
        // white just played e4, ep target e3
        round_trip("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1");
    }

    #[test]
    fn round_trip_no_castling_rights_with_rooks_present() {
        // rooks and kings on their home squares, but rights explicitly withheld —
        // makes sure rights come from board.state, not inferred from piece positions
        round_trip("r3k2r/8/8/8/8/8/8/R3K2R w - - 0 1");
    }

    #[test]
    fn round_trip_partial_castling_rights() {
        round_trip("r3k2r/8/8/8/8/8/8/R3K2R w Kq - 0 1");
    }
}