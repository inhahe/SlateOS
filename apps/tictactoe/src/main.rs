//! Tic-Tac-Toe game with AI opponent for SlateOS.
//!
//! Features:
//! - 3x3 grid with X and O
//! - Perfect AI using minimax (unbeatable)
//! - Arrow key + Enter placement
//! - Mouse click placement
//! - Score tracking
//! - Player goes first (X), AI is O
//! - Catppuccin Mocha theme

#![allow(dead_code)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::fn_params_excessive_bools)]
#![allow(clippy::needless_range_loop)]
#![allow(unused_imports)]

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ── Catppuccin Mocha ────────────────────────────────────────────────
const COL_BASE: Color = Color::from_hex(0x1E1E2E);
const COL_MANTLE: Color = Color::from_hex(0x181825);
const COL_SURFACE0: Color = Color::from_hex(0x313244);
const COL_SURFACE1: Color = Color::from_hex(0x45475A);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COL_BLUE: Color = Color::from_hex(0x89B4FA);
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
const COL_RED: Color = Color::from_hex(0xF38BA8);
const COL_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COL_PEACH: Color = Color::from_hex(0xFAB387);
const COL_LAVENDER: Color = Color::from_hex(0xB4BEFE);
const COL_OVERLAY0: Color = Color::from_hex(0x6C7086);
const COL_TEAL: Color = Color::from_hex(0x94E2D5);
const COL_MAUVE: Color = Color::from_hex(0xCBA6F7);

// ── Cell/Mark ───────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    X,
    O,
}

impl Mark {
    fn other(self) -> Self {
        match self {
            Self::X => Self::O,
            Self::O => Self::X,
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            Self::X => "X",
            Self::O => "O",
        }
    }
}

// ── Board ───────────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy)]
struct Board {
    cells: [Option<Mark>; 9],
}

impl Board {
    fn new() -> Self {
        Self { cells: [None; 9] }
    }

    fn get(&self, row: usize, col: usize) -> Option<Mark> {
        self.cells[row * 3 + col]
    }

    fn set(&mut self, row: usize, col: usize, mark: Mark) {
        self.cells[row * 3 + col] = Some(mark);
    }

    fn is_empty(&self, row: usize, col: usize) -> bool {
        self.cells[row * 3 + col].is_none()
    }

    fn is_full(&self) -> bool {
        self.cells.iter().all(|c| c.is_some())
    }

    fn available_moves(&self) -> Vec<(usize, usize)> {
        let mut moves = Vec::new();
        for r in 0..3 {
            for c in 0..3 {
                if self.is_empty(r, c) {
                    moves.push((r, c));
                }
            }
        }
        moves
    }

    /// Check winner. Returns Some(Mark) if there's a winner, None otherwise.
    fn winner(&self) -> Option<Mark> {
        const LINES: [[usize; 3]; 8] = [
            [0, 1, 2],
            [3, 4, 5],
            [6, 7, 8], // rows
            [0, 3, 6],
            [1, 4, 7],
            [2, 5, 8], // cols
            [0, 4, 8],
            [2, 4, 6], // diagonals
        ];

        for line in &LINES {
            if let Some(mark) = self.cells[line[0]]
                && self.cells[line[1]] == Some(mark) && self.cells[line[2]] == Some(mark) {
                    return Some(mark);
                }
        }
        None
    }

    /// Return the winning line indices, if any
    fn winning_line(&self) -> Option<[usize; 3]> {
        const LINES: [[usize; 3]; 8] = [
            [0, 1, 2],
            [3, 4, 5],
            [6, 7, 8],
            [0, 3, 6],
            [1, 4, 7],
            [2, 5, 8],
            [0, 4, 8],
            [2, 4, 6],
        ];

        for line in &LINES {
            if let Some(mark) = self.cells[line[0]]
                && self.cells[line[1]] == Some(mark) && self.cells[line[2]] == Some(mark) {
                    return Some(*line);
                }
        }
        None
    }

    fn is_game_over(&self) -> bool {
        self.winner().is_some() || self.is_full()
    }
}

// ── Minimax AI ──────────────────────────────────────────────────────
fn minimax(board: &Board, is_maximizing: bool, ai_mark: Mark, depth: i32) -> i32 {
    // Depth-aware terminal scoring: a win reached sooner scores higher and a loss
    // reached later scores higher, so the AI prefers an immediate win over an
    // equally-winning-but-slower line (and delays an unavoidable loss). Without
    // this, an immediate win and a delayed win both score 10 and iteration order
    // could pick a blocking move instead of taking the win.
    if let Some(winner) = board.winner() {
        return if winner == ai_mark {
            10 - depth
        } else {
            depth - 10
        };
    }
    if board.is_full() {
        return 0;
    }

    let current_mark = if is_maximizing {
        ai_mark
    } else {
        ai_mark.other()
    };
    let moves = board.available_moves();

    if is_maximizing {
        let mut best = i32::MIN;
        for (r, c) in moves {
            let mut b = *board;
            b.set(r, c, current_mark);
            let score = minimax(&b, false, ai_mark, depth + 1);
            if score > best {
                best = score;
            }
        }
        best
    } else {
        let mut best = i32::MAX;
        for (r, c) in moves {
            let mut b = *board;
            b.set(r, c, current_mark);
            let score = minimax(&b, true, ai_mark, depth + 1);
            if score < best {
                best = score;
            }
        }
        best
    }
}

fn best_move(board: &Board, ai_mark: Mark) -> Option<(usize, usize)> {
    let moves = board.available_moves();
    if moves.is_empty() {
        return None;
    }

    let mut best_score = i32::MIN;
    let mut best = moves[0];

    for (r, c) in moves {
        let mut b = *board;
        b.set(r, c, ai_mark);
        let score = minimax(&b, false, ai_mark, 1);
        if score > best_score {
            best_score = score;
            best = (r, c);
        }
    }
    Some(best)
}

// ── Game state ──────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GameState {
    Playing,
    Won(Mark),
    Draw,
}

// ── App ─────────────────────────────────────────────────────────────
struct TicTacToeApp {
    board: Board,
    state: GameState,
    current_turn: Mark,
    cursor_row: usize,
    cursor_col: usize,
    // Score
    player_wins: u32,
    ai_wins: u32,
    draws: u32,
    // Winning line for highlight
    win_line: Option<[usize; 3]>,
    // Last rendered window dimensions. Stored so mouse hit-testing uses the same
    // (centered) grid layout that render() draws — otherwise clicks map to the
    // wrong cells. Defaults to the common 800x600 window before the first render.
    win_width: f32,
    win_height: f32,
}

impl TicTacToeApp {
    fn new() -> Self {
        Self {
            board: Board::new(),
            state: GameState::Playing,
            current_turn: Mark::X, // Player is X, goes first
            cursor_row: 1,
            cursor_col: 1,
            player_wins: 0,
            ai_wins: 0,
            draws: 0,
            win_line: None,
            win_width: 800.0,
            win_height: 600.0,
        }
    }

    /// Top-left origin and cell size of the 3x3 grid for a given window width.
    /// Shared by `render` and mouse hit-testing so clicks map to the cells the
    /// player actually sees (the grid is horizontally centered).
    fn grid_layout(width: f32) -> (f32, f32, f32) {
        let cell_sz = 100.0_f32;
        let grid_x = width / 2.0 - 150.0;
        let grid_y = 100.0_f32;
        (grid_x, grid_y, cell_sz)
    }

    fn reset(&mut self) {
        self.board = Board::new();
        self.state = GameState::Playing;
        self.current_turn = Mark::X;
        self.win_line = None;
    }

    fn place_mark(&mut self, row: usize, col: usize) {
        if self.state != GameState::Playing {
            return;
        }
        if !self.board.is_empty(row, col) {
            return;
        }
        if self.current_turn != Mark::X {
            return; // Not player's turn
        }

        self.board.set(row, col, Mark::X);
        self.check_game_over();

        if self.state == GameState::Playing {
            self.current_turn = Mark::O;
            self.ai_move();
        }
    }

    fn ai_move(&mut self) {
        if self.state != GameState::Playing || self.current_turn != Mark::O {
            return;
        }
        if let Some((r, c)) = best_move(&self.board, Mark::O) {
            self.board.set(r, c, Mark::O);
            self.current_turn = Mark::X;
            self.check_game_over();
        }
    }

    fn check_game_over(&mut self) {
        if let Some(winner) = self.board.winner() {
            self.state = GameState::Won(winner);
            self.win_line = self.board.winning_line();
            match winner {
                Mark::X => self.player_wins += 1,
                Mark::O => self.ai_wins += 1,
            }
        } else if self.board.is_full() {
            self.state = GameState::Draw;
            self.draws += 1;
        }
    }

    fn event(&mut self, event: &Event) {
        match event {
            Event::Key(KeyEvent { key, .. }) => match key {
                Key::Up
                    if self.cursor_row > 0 => {
                        self.cursor_row -= 1;
                    }
                Key::Down
                    if self.cursor_row < 2 => {
                        self.cursor_row += 1;
                    }
                Key::Left
                    if self.cursor_col > 0 => {
                        self.cursor_col -= 1;
                    }
                Key::Right
                    if self.cursor_col < 2 => {
                        self.cursor_col += 1;
                    }
                Key::Enter | Key::Space => {
                    if self.state == GameState::Playing {
                        self.place_mark(self.cursor_row, self.cursor_col);
                    } else {
                        self.reset();
                    }
                }
                Key::N => self.reset(),
                _ => {}
            },
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Press(MouseButton::Left),
                x,
                y,
                ..
            }) => {
                // Map click to grid cell using the same layout render() draws.
                let (grid_x, grid_y, cell_sz) = Self::grid_layout(self.win_width);

                let col = ((*x - grid_x) / cell_sz) as i32;
                let row = ((*y - grid_y) / cell_sz) as i32;

                if (0..3).contains(&row) && (0..3).contains(&col) {
                    self.cursor_row = row as usize;
                    self.cursor_col = col as usize;
                    if self.state == GameState::Playing {
                        self.place_mark(row as usize, col as usize);
                    }
                }
            }
            _ => {}
        }
    }

    fn render(&mut self, width: f32, height: f32) -> Vec<RenderCommand> {
        // Remember the window size so mouse hit-testing matches this layout.
        self.win_width = width;
        self.win_height = height;
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: COL_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: width / 2.0 - 70.0,
            y: 20.0,
            text: "Tic-Tac-Toe".to_string(),
            font_size: 24.0,
            color: COL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Status
        let status = match self.state {
            GameState::Playing => {
                if self.current_turn == Mark::X {
                    "Your turn (X)".to_string()
                } else {
                    "AI thinking...".to_string()
                }
            }
            GameState::Won(Mark::X) => "You win!".to_string(),
            GameState::Won(Mark::O) => "AI wins!".to_string(),
            GameState::Draw => "Draw!".to_string(),
        };
        let status_color = match self.state {
            GameState::Playing => COL_SUBTEXT0,
            GameState::Won(Mark::X) => COL_GREEN,
            GameState::Won(Mark::O) => COL_RED,
            GameState::Draw => COL_YELLOW,
        };
        cmds.push(RenderCommand::Text {
            x: width / 2.0 - 60.0,
            y: 55.0,
            text: status,
            font_size: 16.0,
            color: status_color,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Grid
        let (grid_x, grid_y, cell_sz) = Self::grid_layout(width);

        // Draw cells
        for row in 0..3 {
            for col in 0..3 {
                let cx = grid_x + col as f32 * cell_sz;
                let cy = grid_y + row as f32 * cell_sz;
                let idx = row * 3 + col;

                let is_cursor = row == self.cursor_row && col == self.cursor_col;
                let is_win_cell = self.win_line.is_some_and(|line| line.contains(&idx));

                // Cell background
                let bg = if is_win_cell {
                    COL_SURFACE1
                } else if is_cursor && self.state == GameState::Playing {
                    COL_SURFACE0
                } else {
                    COL_MANTLE
                };

                cmds.push(RenderCommand::FillRect {
                    x: cx + 2.0,
                    y: cy + 2.0,
                    width: cell_sz - 4.0,
                    height: cell_sz - 4.0,
                    color: bg,
                    corner_radii: CornerRadii::all(8.0),
                });

                // Mark
                if let Some(mark) = self.board.get(row, col) {
                    let (text, color) = match mark {
                        Mark::X => ("X", COL_BLUE),
                        Mark::O => ("O", COL_RED),
                    };
                    cmds.push(RenderCommand::Text {
                        x: cx + cell_sz / 2.0 - 15.0,
                        y: cy + cell_sz / 2.0 - 18.0,
                        text: text.to_string(),
                        font_size: 40.0,
                        color,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                    });
                }
            }
        }

        // Cursor highlight
        if self.state == GameState::Playing {
            let cx = grid_x + self.cursor_col as f32 * cell_sz;
            let cy = grid_y + self.cursor_row as f32 * cell_sz;
            cmds.push(RenderCommand::StrokeRect {
                x: cx + 2.0,
                y: cy + 2.0,
                width: cell_sz - 4.0,
                height: cell_sz - 4.0,
                color: COL_LAVENDER,
                line_width: 3.0,
                corner_radii: CornerRadii::all(8.0),
            });
        }

        // Score
        let score_y = grid_y + 3.0 * cell_sz + 30.0;
        cmds.push(RenderCommand::Text {
            x: grid_x,
            y: score_y,
            text: format!("You (X): {}", self.player_wins),
            font_size: 16.0,
            color: COL_BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: grid_x + 140.0,
            y: score_y,
            text: format!("AI (O): {}", self.ai_wins),
            font_size: 16.0,
            color: COL_RED,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: grid_x + 260.0,
            y: score_y,
            text: format!("Draws: {}", self.draws),
            font_size: 16.0,
            color: COL_YELLOW,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Help
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: height - 24.0,
            text: "Arrows=Move  Enter/Space=Place  N=New Game  Click=Place".to_string(),
            font_size: 11.0,
            color: COL_OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds
    }
}

fn main() {
    let _app = TicTacToeApp::new();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_board_new() {
        let b = Board::new();
        for r in 0..3 {
            for c in 0..3 {
                assert!(b.is_empty(r, c));
            }
        }
    }

    #[test]
    fn test_board_set_get() {
        let mut b = Board::new();
        b.set(0, 0, Mark::X);
        assert_eq!(b.get(0, 0), Some(Mark::X));
        assert!(b.is_empty(0, 1));
    }

    #[test]
    fn test_board_is_full() {
        let mut b = Board::new();
        assert!(!b.is_full());
        for r in 0..3 {
            for c in 0..3 {
                b.set(r, c, if (r + c) % 2 == 0 { Mark::X } else { Mark::O });
            }
        }
        assert!(b.is_full());
    }

    #[test]
    fn test_available_moves() {
        let mut b = Board::new();
        assert_eq!(b.available_moves().len(), 9);
        b.set(0, 0, Mark::X);
        assert_eq!(b.available_moves().len(), 8);
    }

    #[test]
    fn test_winner_row() {
        let mut b = Board::new();
        b.set(0, 0, Mark::X);
        b.set(0, 1, Mark::X);
        b.set(0, 2, Mark::X);
        assert_eq!(b.winner(), Some(Mark::X));
    }

    #[test]
    fn test_winner_col() {
        let mut b = Board::new();
        b.set(0, 1, Mark::O);
        b.set(1, 1, Mark::O);
        b.set(2, 1, Mark::O);
        assert_eq!(b.winner(), Some(Mark::O));
    }

    #[test]
    fn test_winner_diagonal() {
        let mut b = Board::new();
        b.set(0, 0, Mark::X);
        b.set(1, 1, Mark::X);
        b.set(2, 2, Mark::X);
        assert_eq!(b.winner(), Some(Mark::X));
    }

    #[test]
    fn test_winner_anti_diagonal() {
        let mut b = Board::new();
        b.set(0, 2, Mark::O);
        b.set(1, 1, Mark::O);
        b.set(2, 0, Mark::O);
        assert_eq!(b.winner(), Some(Mark::O));
    }

    #[test]
    fn test_no_winner() {
        let b = Board::new();
        assert_eq!(b.winner(), None);
    }

    #[test]
    fn test_winning_line() {
        let mut b = Board::new();
        b.set(0, 0, Mark::X);
        b.set(0, 1, Mark::X);
        b.set(0, 2, Mark::X);
        assert_eq!(b.winning_line(), Some([0, 1, 2]));
    }

    #[test]
    fn test_is_game_over_win() {
        let mut b = Board::new();
        b.set(0, 0, Mark::X);
        b.set(0, 1, Mark::X);
        b.set(0, 2, Mark::X);
        assert!(b.is_game_over());
    }

    #[test]
    fn test_is_game_over_draw() {
        let mut b = Board::new();
        // X O X
        // X X O
        // O X O
        b.set(0, 0, Mark::X);
        b.set(0, 1, Mark::O);
        b.set(0, 2, Mark::X);
        b.set(1, 0, Mark::X);
        b.set(1, 1, Mark::X);
        b.set(1, 2, Mark::O);
        b.set(2, 0, Mark::O);
        b.set(2, 1, Mark::X);
        b.set(2, 2, Mark::O);
        assert!(b.is_game_over());
        assert_eq!(b.winner(), None);
    }

    #[test]
    fn test_mark_other() {
        assert_eq!(Mark::X.other(), Mark::O);
        assert_eq!(Mark::O.other(), Mark::X);
    }

    #[test]
    fn test_mark_symbol() {
        assert_eq!(Mark::X.symbol(), "X");
        assert_eq!(Mark::O.symbol(), "O");
    }

    #[test]
    fn test_minimax_ai_wins() {
        let mut b = Board::new();
        // AI (O) has two in a row, should win
        b.set(0, 0, Mark::O);
        b.set(0, 1, Mark::O);
        // X elsewhere
        b.set(1, 0, Mark::X);
        b.set(1, 1, Mark::X);
        // AI should pick (0,2) to win
        let mv = best_move(&b, Mark::O);
        assert_eq!(mv, Some((0, 2)));
    }

    #[test]
    fn test_minimax_blocks_player() {
        let mut b = Board::new();
        // X has two in a row
        b.set(0, 0, Mark::X);
        b.set(0, 1, Mark::X);
        // O must block at (0,2)
        b.set(1, 1, Mark::O);
        let mv = best_move(&b, Mark::O);
        assert_eq!(mv, Some((0, 2)));
    }

    #[test]
    fn test_minimax_empty_board() {
        let b = Board::new();
        let mv = best_move(&b, Mark::O);
        assert!(mv.is_some());
    }

    #[test]
    fn test_minimax_prefers_win_over_block() {
        let mut b = Board::new();
        // O can win
        b.set(1, 0, Mark::O);
        b.set(1, 1, Mark::O);
        // X threatens
        b.set(0, 0, Mark::X);
        b.set(0, 1, Mark::X);
        // O should take the win at (1,2)
        let mv = best_move(&b, Mark::O);
        assert_eq!(mv, Some((1, 2)));
    }

    #[test]
    fn test_minimax_full_board() {
        let mut b = Board::new();
        b.set(0, 0, Mark::X);
        b.set(0, 1, Mark::O);
        b.set(0, 2, Mark::X);
        b.set(1, 0, Mark::X);
        b.set(1, 1, Mark::X);
        b.set(1, 2, Mark::O);
        b.set(2, 0, Mark::O);
        b.set(2, 1, Mark::X);
        b.set(2, 2, Mark::O);
        let mv = best_move(&b, Mark::O);
        assert_eq!(mv, None);
    }

    #[test]
    fn test_app_new() {
        let app = TicTacToeApp::new();
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.current_turn, Mark::X);
        assert_eq!(app.player_wins, 0);
    }

    #[test]
    fn test_app_place_mark() {
        let mut app = TicTacToeApp::new();
        app.place_mark(1, 1);
        assert_eq!(app.board.get(1, 1), Some(Mark::X));
        // AI should have moved too
        let ai_cells = (0..3)
            .flat_map(|r| (0..3).map(move |c| (r, c)))
            .filter(|&(r, c)| app.board.get(r, c) == Some(Mark::O))
            .count();
        assert_eq!(ai_cells, 1);
    }

    #[test]
    fn test_place_on_occupied() {
        let mut app = TicTacToeApp::new();
        app.place_mark(1, 1);
        let board_after = app.board;
        // Try to place on same cell
        app.current_turn = Mark::X;
        app.place_mark(1, 1);
        // Board should be unchanged
        assert_eq!(app.board.cells, board_after.cells);
    }

    #[test]
    fn test_reset() {
        let mut app = TicTacToeApp::new();
        app.place_mark(0, 0);
        app.player_wins = 3;
        app.reset();
        assert_eq!(app.state, GameState::Playing);
        assert!(app.board.available_moves().len() == 9);
        // Score preserved
        assert_eq!(app.player_wins, 3);
    }

    #[test]
    fn test_enter_key_places() {
        let mut app = TicTacToeApp::new();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.event(&Event::Key(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.board.get(0, 0), Some(Mark::X));
    }

    #[test]
    fn test_arrow_keys() {
        let mut app = TicTacToeApp::new();
        app.cursor_row = 1;
        app.cursor_col = 1;
        app.event(&Event::Key(KeyEvent {
            key: Key::Up,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.cursor_row, 0);

        app.event(&Event::Key(KeyEvent {
            key: Key::Down,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.cursor_row, 1);
    }

    #[test]
    fn test_cursor_bounds() {
        let mut app = TicTacToeApp::new();
        app.cursor_row = 0;
        app.event(&Event::Key(KeyEvent {
            key: Key::Up,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.cursor_row, 0);

        app.cursor_col = 2;
        app.event(&Event::Key(KeyEvent {
            key: Key::Right,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.cursor_col, 2);
    }

    #[test]
    fn test_n_key_resets() {
        let mut app = TicTacToeApp::new();
        app.place_mark(0, 0);
        app.event(&Event::Key(KeyEvent {
            key: Key::N,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.board.available_moves().len(), 9);
    }

    #[test]
    fn test_enter_after_game_over_resets() {
        let mut app = TicTacToeApp::new();
        app.state = GameState::Draw;
        app.event(&Event::Key(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_render_no_panic() {
        let mut app = TicTacToeApp::new();
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_marks() {
        let mut app = TicTacToeApp::new();
        app.place_mark(0, 0);
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_game_over() {
        let mut app = TicTacToeApp::new();
        app.state = GameState::Won(Mark::X);
        app.win_line = Some([0, 1, 2]);
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_draw() {
        let mut app = TicTacToeApp::new();
        app.state = GameState::Draw;
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ai_never_loses_as_o() {
        // Play all possible first moves as X, AI should never lose
        for r in 0..3 {
            for c in 0..3 {
                let mut app = TicTacToeApp::new();
                app.place_mark(r, c);
                // Keep playing until game over
                let mut safety = 0;
                while app.state == GameState::Playing && safety < 20 {
                    safety += 1;
                    let moves = app.board.available_moves();
                    if !moves.is_empty() && app.current_turn == Mark::X {
                        let (mr, mc) = moves[0];
                        app.place_mark(mr, mc);
                    }
                }
                // AI should never lose
                assert_ne!(
                    app.state,
                    GameState::Won(Mark::X),
                    "AI lost when X started at ({r},{c})"
                );
            }
        }
    }

    #[test]
    fn test_main_no_panic() {
        main();
    }

    #[test]
    fn test_place_when_not_x_turn() {
        let mut app = TicTacToeApp::new();
        app.current_turn = Mark::O;
        app.place_mark(0, 0);
        // Should not place — not player's turn
        assert!(app.board.is_empty(0, 0));
    }

    #[test]
    fn test_place_when_game_over() {
        let mut app = TicTacToeApp::new();
        app.state = GameState::Draw;
        app.place_mark(0, 0);
        assert!(app.board.is_empty(0, 0));
    }

    #[test]
    fn test_winning_row_2() {
        let mut b = Board::new();
        b.set(1, 0, Mark::X);
        b.set(1, 1, Mark::X);
        b.set(1, 2, Mark::X);
        assert_eq!(b.winner(), Some(Mark::X));
        assert_eq!(b.winning_line(), Some([3, 4, 5]));
    }

    #[test]
    fn test_winning_row_3() {
        let mut b = Board::new();
        b.set(2, 0, Mark::O);
        b.set(2, 1, Mark::O);
        b.set(2, 2, Mark::O);
        assert_eq!(b.winner(), Some(Mark::O));
        assert_eq!(b.winning_line(), Some([6, 7, 8]));
    }

    #[test]
    fn test_mouse_click() {
        let mut app = TicTacToeApp::new();
        let grid_x = 800.0 / 2.0 - 150.0;
        let grid_y = 100.0;
        // Click center cell (1,1)
        app.event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Press(MouseButton::Left),
            x: grid_x + 150.0,
            y: grid_y + 150.0,
        }));
        assert_eq!(app.board.get(1, 1), Some(Mark::X));
    }
}
