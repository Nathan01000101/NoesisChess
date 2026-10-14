# Noesis 0.5 Release

**by: Nathan E**

Noesis is a UCI compatible chess engine built in the **Rust** language with a strength of **~2500** elo.

## System Requirements

- AVX2 compatible CPU

## Building the program

in the repo root directory:
`cargo b -r`

## Using with a Chess GUI

the **working directory** must contain **Noesis.exe**

## Supported UCI commands

- `uci`
- `isready`
- `quit`
- `ucinewgame`
- `position <fen/startpos> moves <moves>`
- `go w/btime <time> w/binc <inc> movestogo <movestogo>`

## Running the Program

Double-clicking the executable or running without specification will start an instance of the engine, without any GUI. This is intended; if you would wish to use the built in GUI bundled with Noesis please see below.

*read further for CLI usage/examples*

##  CLI Specifications

### Launch program from command line with specification:
Noesis --white <white_player> --black <black_player> --depth <depth> --fen <fen_of_position> --gui

**Note:** The `--white` and `--black` flags only apply when the `--gui` flag is also passed 

### Available players:

- `human` - you play with the mouse
- `minimax` - default AI, searches and evaluates moves and picks the 'best' one

### Depth:

this field sets the max depth the engine will try to search. The default is 40 however it tends to only reach depth 14-20 in classical games with max 5s of thinking.

## GUI
- **Click** a piece to select it
- **Click** a destination to move
- **F** to flip the board

#### Examples For Running With GUI:
- `./Noesis --gui ` 	*This will start a game with a human playing white and Noesis with black*
- `./Noesis --white minimax --black minimax --gui` 	*This will start a game with two Noesi (plural for Noesis.. according to me)*
- `./Noesis --white minimax --black minimax --depth 7 --gui` 	*This will start a game with two Noesi with their max depth being set to 7*
- `./Noesis --fen "rnbqkbnr/pp1ppppp/8/2p5/4P3/8/PPPP1PPP/RNBQKBNR w KQkq c6 0 2" --gui` 	*This will start a game with a human playing white, Noesis playing black in a sicilian*


## About the AI (for nerds)

### Features:
- NNUE (Efficiently Updatable Neural Network)
- Quiescence Search
- Late Move Reductions
- Late Move Pruning
- Null Move Pruning
- Static Exchange Evaluation (qsearch + move ordering)
- Reverse Futility Pruning
- Iterative Deepening
- PVS
- Alpha/Beta Pruning
- Killer Moves
- History Heuristic w/ Malus
- Transposition Table w/ buckets and prefetching

The Engine's backbone is its **negamax algorithm**, used to score moves. The following is an example of a negamax algorithm with alpha/beta pruning: 

*( with move/board handling + pruning omitted for clarity purposes )*:

```rust
fn negamax(board_state, depth, alpha, beta) -> i32{
	if depth == 0{
		return evaluate();
	}

	let max_evaluation = -infinity;

	let moves = get_all_moves();

	for mv in moves{
		new_eval = -negamax(board_state, depth - 1, -beta, -alpha); // this is the recursive part

		
		max_evaluation = max_evaluation.max(new_eval);
		alpha = alpha.max(new_eval); 
		
		if beta <= alpha{
			break; // opponent will end up ahead 
		}
	}
	
	return max_evaluation;
}

```

### Board Evaluation

Starting with this release, Noesis uses a 256 neuron **NNUE** trained with [bullet](https://github.com/jw1912/bullet) for its static board evaluation. This NNUE is the second net, trained on 100M positions of training data, assembled by the first net, which was built off my HCE with 20M positions
