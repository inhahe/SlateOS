//! The terminal's window.
//!
//! Everything a terminal *is* -- the emulator, the link to the shell, the
//! scrollback, the drawing -- is the `terminal` library beside this file,
//! which `apps/tmux` shares. This is the program that gives one of them a
//! window and a shell.

use oswindow::app;
use std::process::ExitCode;
use terminal::{TerminalConfig, TerminalState, child};

fn main() -> ExitCode {
    let mut terminal = TerminalState::new(TerminalConfig::default());
    // Started before the window, and so before any thread of this process
    // exists: see `libcall::pty` on why a fork wants as few threads beside it
    // as possible.
    terminal.start_with(child::spawn_shell);
    app::launch("terminal", &mut terminal)
}
