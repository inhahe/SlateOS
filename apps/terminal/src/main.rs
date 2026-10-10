//! The terminal's window.
//!
//! Everything a terminal *is* -- the emulator, the link to the shell, the
//! scrollback, the drawing -- is the `terminal` library beside this file,
//! which `apps/tmux` shares. This is the program that gives one of them a
//! window and a shell -- or, with `-e PROGRAM ARG...`, that program instead,
//! which is how the desktop starts a program whose entry says `Terminal=true`
//! ([`terminal::CommandLine`]).

use oswindow::app;
use std::process::ExitCode;
use terminal::{CommandLine, TerminalConfig, TerminalState, child};

fn main() -> ExitCode {
    // Read here and handed on to `launch_with`, not `launch`: `launch` refuses
    // every argument it does not take itself, so `-e` and its program never
    // reached the window.
    let line = match CommandLine::parse(std::env::args_os().skip(1).collect()) {
        Ok(line) => line,
        Err(why) => {
            eprintln!("terminal: {why}");
            return ExitCode::from(2);
        }
    };
    let mut terminal = TerminalState::new(TerminalConfig::default());
    // Started before the window, and so before any thread of this process
    // exists: see `libcall::pty` on why a fork wants as few threads beside it
    // as possible.
    match &line.command {
        Some((program, args)) => {
            terminal.start_with(|size| child::spawn_command(program, args, size));
        }
        None => terminal.start_with(child::spawn_shell),
    }
    app::launch_with("terminal", line.display.as_deref(), &mut terminal)
}
