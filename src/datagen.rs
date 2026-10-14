use std::{
    fs::{self, OpenOptions},
    io::{self, BufWriter, Write},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    board::{get_piece, make_move, new_board, to_fen},
    movegen::{get_all_moves, is_in_check},
    search::{Limits, MATE_THRESHOLD, Searcher},
    types::{Board, EN_PASSANT_MOVE, Move, MoveListBuf, PROMOTION_MOVE, PieceType, Side},
    zobrist::ZobristTable,
};

// output: one file per thread per run in DATA_PATH, each line is
//   fen | score in cp, white pov | result 1.0 / 0.5 / 0.0, 1 => white win, 0.5 => draw, 0 => black win

// data gen params
const THREADS: u8 = 12;
const TARGET_POSITIONS: u64 = 100_000_000; // total across ALL threads, fast threads just contribute more
const NODE_LIMIT: u64 = 5_000; // per move
const MAX_DEPTH: i32 = 16;
const TT_MB: usize = 16;

// openings: play random moves, then throw the opening away if a search says it's too lopsided.
const RANDOM_OPENING_PLIES: u32 = 8;
const RANDOM_OPENING_EVAL_CUTOFF: i32 = 400;

// counted in plies from the start position, random ones included
const MIN_RECORD_PLIES: u32 = 10;

// adjudication, all based on search scores (white pov)
const ADJUDICATE_EVAL: i32 = 1800;
const ADJUDICATE_PLIES: u32 = 4;
const DRAW_ADJUDICATE_EVAL: i32 = 10;
const DRAW_ADJUDICATE_PLIES: u32 = 10;
const DRAW_ADJUDICATE_MIN_PLY: u32 = 80;
const MAX_GAME_PLIES: u32 = 500;

const DATA_PATH: &str = "data/";
const WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;

// every search in datagen: node limit only, no clock, no UCI spam
const LIMITS: Limits = Limits {
    time_ms: 0,
    max_nodes: NODE_LIMIT,
    max_depth: MAX_DEPTH,
    uci_info: false,
};

#[derive(Clone, Copy)]
enum Outcome {
    WhiteWin,
    BlackWin,
    Draw,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Outcome::WhiteWin => "1.0",
            Outcome::Draw => "0.5",
            Outcome::BlackWin => "0.0",
        }
    }

    // the side that just got checkmated lost
    fn mated(side: Side) -> Outcome {
        if side == Side::White {
            Outcome::BlackWin
        } else {
            Outcome::WhiteWin
        }
    }
}

struct DataEntry {
    fen: String,
    score: i32, // cp, white pov
}

impl DataEntry {
    fn to_line(&self, outcome: Outcome) -> String {
        format!("{} | {} | {}", self.fen, self.score, outcome.as_str())
    }
}

// one finished game
struct GameRecord {
    entries: Vec<DataEntry>,
    outcome: Outcome,
    rerolls: u64, // openings thrown away before finding a usable one
}

// splitmix64 rng
pub struct Rng(pub u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

pub fn start_data_gen() {
    if let Err(e) = fs::create_dir_all(DATA_PATH) {
        eprintln!("couldn't create {}: {}", DATA_PATH, e);
        return;
    }

    let run_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos() as u64;
    let start = Instant::now();

    // shared across threads: every worker adds to it, everyone stops once it hits the target
    let total_written = Arc::new(AtomicU64::new(0));

    let handles: Vec<_> = (0..THREADS)
        .map(|t| {
            let total_written = Arc::clone(&total_written);
            thread::Builder::new()
                .name(format!("datagen-{}", t))
                .stack_size(WORKER_STACK_BYTES)
                .spawn(move || worker(t, run_id, &total_written))
                .expect("failed to spawn datagen thread")
        })
        .collect();

    for (t, handle) in handles.into_iter().enumerate() {
        match handle.join() {
            Ok(Ok(n)) => println!("[datagen {}] finished, wrote {} positions", t, n),
            Ok(Err(e)) => eprintln!("[datagen {}] io error: {}", t, e),
            Err(_) => eprintln!("[datagen {}] panicked", t),
        }
    }

    println!(
        "datagen done: {} positions in {:.1}s",
        total_written.load(Ordering::Relaxed),
        start.elapsed().as_secs_f64()
    );
}

fn worker(thread_id: u8, run_id: u64, total_written: &AtomicU64) -> io::Result<u64> {
    let mut rng = Rng(run_id ^ (thread_id as u64 + 1).wrapping_mul(0xD1B5_4A32_D192_ED03));
    let zobrist = ZobristTable::new();
    let mut searcher = Searcher::new(TT_MB); // one per thread, reused for every game

    // run id + thread id -> unique file, nothing gets overwritten
    let path = format!("{}data_{}_t{}.txt", DATA_PATH, run_id, thread_id);
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    let mut writer = BufWriter::new(file);

    let mut written: u64 = 0; // by this thread
    let mut games: u64 = 0;
    let mut rerolls: u64 = 0;
    while total_written.load(Ordering::Relaxed) < TARGET_POSITIONS {
        let game = play_game(&mut rng, &zobrist, &mut searcher);
        for entry in &game.entries {
            writeln!(writer, "{}", entry.to_line(game.outcome))?;
        }
        // flush every game so a crash only loses the game in progress
        writer.flush()?;

        let n = game.entries.len() as u64;
        written += n;
        let total = total_written.fetch_add(n, Ordering::Relaxed) + n;
        games += 1;
        rerolls += game.rerolls;
        if games % 100 == 0 {
            println!(
                "[datagen {}] {} games, {} positions, {:.1}% done overall, {:.2} rerolls/game",
                thread_id,
                games,
                written,
                total as f64 / TARGET_POSITIONS as f64 * 100.0,
                rerolls as f64 / games as f64
            );
        }
    }

    Ok(written)
}

fn play_game(rng: &mut Rng, zobrist: &ZobristTable, searcher: &mut Searcher) -> GameRecord {
    // fresh TT + history so every game is independent
    searcher.clear();
    let mut rerolls: u64 = 0;

    // keep rolling openings until one is still going and roughly balanced
    let (mut board, mut hashes) = loop {
        let Some((board, hashes)) = random_opening(rng, zobrist) else {
            rerolls += 1; // game ended during the random plies
            continue;
        };
        match searcher.search(&board, &hashes, &LIMITS) {
            Some(r) if r.depth >= 1 && r.score.abs() < RANDOM_OPENING_EVAL_CUTOFF => {
                break (board, hashes);
            }
            _ => {
                rerolls += 1; // no moves, search didn't finish, or lopsided
                continue;
            }
        }
    };

    let mut game_ply = RANDOM_OPENING_PLIES;
    let mut recorded: Vec<DataEntry> = Vec::new();
    let mut white_streak: u32 = 0;
    let mut black_streak: u32 = 0;
    let mut draw_streak: u32 = 0;

    let outcome = loop {
        let side = board.side_to_move();
        let hash = *hashes.last().expect("game history is empty");

        // draws by rule (half_moves = 50 move clock).
        // this ends the actual game; the search doesn't
        if board.half_moves >= 100
            || repetitions(&hashes, hash) >= 3
            || insufficient_material(&board)
            || game_ply >= MAX_GAME_PLIES
        {
            break Outcome::Draw;
        }

        // None -> no legal moves
        let Some(result) = searcher.search(&board, &hashes, &LIMITS) else {
            break if is_in_check(&board, side) {
                Outcome::mated(side)
            } else {
                Outcome::Draw
            };
        };

        if result.depth >= 1 {
            let white_score = if side == Side::White {
                result.score
            } else {
                -result.score
            };

            // proven mate, no point playing it out, and mate scores are garbage labels
            if white_score.abs() >= MATE_THRESHOLD {
                break if white_score > 0 {
                    Outcome::WhiteWin
                } else {
                    Outcome::BlackWin
                };
            }

            // record the position *before* the move, with its own search score
            if game_ply >= MIN_RECORD_PLIES
                && !is_in_check(&board, side)
                && !is_tactical(&board, result.result)
            {
                recorded.push(DataEntry {
                    fen: to_fen(&board),
                    score: white_score,
                });
            }

            white_streak = if white_score >= ADJUDICATE_EVAL {
                white_streak + 1
            } else {
                0
            };
            black_streak = if white_score <= -ADJUDICATE_EVAL {
                black_streak + 1
            } else {
                0
            };
            draw_streak = if game_ply >= DRAW_ADJUDICATE_MIN_PLY
                && white_score.abs() <= DRAW_ADJUDICATE_EVAL
            {
                draw_streak + 1
            } else {
                0
            };

            if white_streak >= ADJUDICATE_PLIES {
                break Outcome::WhiteWin;
            }
            if black_streak >= ADJUDICATE_PLIES {
                break Outcome::BlackWin;
            }
            if draw_streak >= DRAW_ADJUDICATE_PLIES {
                break Outcome::Draw;
            }
        }

        let undo = make_move(&mut board, result.result);
        let new_hash = zobrist.update_hash(hash, &board, result.result, &undo);
        hashes.push(new_hash);
        game_ply += 1;
    };

    GameRecord {
        entries: recorded,
        outcome,
        rerolls,
    }
}

// plays RANDOM_OPENING_PLIES random legal moves from the start position.
// returns None if the game ends during the random part
fn random_opening(rng: &mut Rng, zobrist: &ZobristTable) -> Option<(Board, Vec<u64>)> {
    let mut board = new_board();
    let mut hashes = vec![zobrist.hash(&board)];

    for _ in 0..RANDOM_OPENING_PLIES {
        let side = board.side_to_move();
        let mut moves = MoveListBuf::new();
        get_all_moves(&mut board, side, &mut moves);
        if moves.len == 0 {
            return None;
        }

        let mv = moves.data[rng.below(moves.len)];
        let prev = *hashes.last().expect("game history is empty");
        let undo = make_move(&mut board, mv);
        hashes.push(zobrist.update_hash(prev, &board, mv, &undo));
    }

    Some((board, hashes))
}

fn repetitions(hashes: &[u64], hash: u64) -> usize {
    hashes.iter().filter(|&&h| h == hash).count()
}

// returns true if move is capture (including En Passant) or promotion
fn is_tactical(board: &Board, mv: Move) -> bool {
    let move_type = mv.get_flags() & 0b11;
    board.is_piece(mv.get_to()) || move_type == PROMOTION_MOVE || move_type == EN_PASSANT_MOVE
}

fn insufficient_material(board: &Board) -> bool {
    let mut minors = 0;
    for sq in 0..64 {
        if !board.is_piece(sq) {
            continue;
        }
        match get_piece(board, sq).piece_type {
            PieceType::Pawn | PieceType::Rook | PieceType::Queen => return false,
            PieceType::Knight | PieceType::Bishop => minors += 1,
            _ => {}
        }
    }
    minors <= 1
}
