# Noesis 0.3 Release

**by: Nathan E**

Noesis is a UCI compatible chess engine built in the rust language with a strength of **roughly ~1800** elo.


## Running the Program

Double-clicking the executable or running without specification will start an instance of the engine, without any GUI. This is intended; if you would wish to use the built in GUI bundled with Noesis please see below.

*read further for CLI usage/examples*

##  CLI Specifications

### Launch program from command line with specification:
	Noesis --white <white_player> --black <black_player> --depth <depth> --fen <fen_of_position> --gui

### Available players:

#### WARNING: the `--gui` flag has to be passed if assigning players, it won't be set automatically
- `human` - you play with the mouse
- `minimax` - default AI, searches and evaluates moves and picks the 'best' one
- `random` - plays a random legal move

### Depth:

this field sets the max depth the engine will try to search. The default is 12 and it tends to not reach 12 in most games. The average time to reach a depth of 10 is around 8-10s, if you are looking for faster, less deep searches use this field and use the following for rough reference, keep in mind times **will** vary:

**depth 1-4:** *~1-2ms*

**depth 5-7:** *~10-1000ms*

**depth 8:** *~0.8s-2s*

**depth 9:** *~1s-4s*

**depth 10:** *~5-10s*

**depth 11- :** *~10s-*


### Fen Positions

fen position notation is used to store a position of a board as well as the board state, the following is 
an example of a fen position, specifically the starting position of a chess board:

	rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1

## GUI
- **Click** a piece to select it
- **Click** a destination to move
- **F** to flip the board

#### Examples For Running With GUI:
- `./Noesis --gui ` 	*This will start a game with a human playing white and Noesis with black*
- `./Noesis --white minimax --black minimax --gui` 	*This will start a game with two Noesi (plural for Noesis.. according to me)*
- `./Noesis --white minimax --black minimax --depth 7 --gui` 	*This will start a game with two Noesi with their max depth being set to 7*
- `./Noesis --fen "rnbqkbnr/pp1ppppp/8/2p5/4P3/8/PPPP1PPP/RNBQKBNR w KQkq c6 0 2" --gui` 	*This will start a game with a human playing white, Noesis playing black in a sicilian*


## About the AI

The Engine's backbone is it's **negamax algorithm**, used to score moves. The negamax algorithm looks like this in pseudo-code 

*( with move/board handling omitted for clarity purposes )*:

```rust
fn negamax(board_state) -> i32{
	if depth == 0{
		return evaluate();
	}

	let max_evaluation = -infinity;

	let moves = get_all_moves();

	for mv in moves{
		new_eval = -negamax(board_state); // this is the recursive part

		if new_eval > max_evaluation{ // did the move we just make improve our position
			max_evaluation = new_eval; // if so, this is the best eval
		}
	}
	
	return max_evaluation;
}

```

On top of this, I also use **Alpha-Beta Pruning**, which doesn't inherently lead to better moves per se however, it speeds up the search dramatically by reducing the amount of redundant, unfruitful nodes we have to calculate.


## Board Evaluation
It is pretty simple, I keep a table of values for which how good a position is for a piece. 2 tables, one for early-mid game and one for late game for each piece except knights because their positioning stays relatively the same within early-late game.. This is what the pawn table looks like:

```rust
const PAWN_TABLE: [[i32; 8]; 8] = [
    [ 0,    0,    0,    0,    0,    0,    0,    0   ],  // promotion 
    [ 90,   90,   90,   90,   90,   90,   90,   90  ],
    [ 25,   25,   50,   55,   55,   50,   25,   25  ],
    [ 10,   10,   25,   50,   50,   25,   10,   10  ],
    [ 5,    5,    25,   45,   45,   25,   5,    5   ],
    [ 5,    5,    10,   5,    5,   10,    5,    5   ],
    [ 5,    5,    5,   -10,  -10,   5,    5,    5   ],  // slight penalty for blocking center
    [ 0,    0,    0,    0,    0,    0,    0,    0   ],  // starting rank
];
```

## Move Generation
To be able to achieve a NPS of 10M, you need a well optimized move generation to rely on. Prior to this release, Noesis' move generation relied on ray marching, a slow and costly way to determine if a move can be made or not. On top of that, the previous versions also never did any pre-computing to store move info for a sliding piece like bishops, rook, or queens. Now Noesis can rely on a move generator that pre-computes all possible moves for any sliding piece that already computed every possible combination of pieces that could block its ray, leaving us with the valid moves for all pieces even before the program runs. This leaves the move generator little to do at runtime, maximizing performance.