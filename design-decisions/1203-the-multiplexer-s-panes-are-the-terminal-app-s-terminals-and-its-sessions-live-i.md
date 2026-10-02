## 1203. The multiplexer's panes are the terminal app's terminals, and its sessions live in its window

**Date:** 2026-09-25
**Lane:** E
**Decided by:** Claude (autonomous)

**In short:** Each tmux pane is now a whole terminal -- the same code the
terminal app runs, made into a library both programs use -- with the user's
shell in it. A shell that exits normally closes its pane, as in tmux; one that
fails leaves the pane open to say how. Closing a pane or a window asks first,
because it ends the programs in it. Sessions last as long as the tmux window:
detaching hides one, but closing the window ends them all, and the window says
so. That last point is the big difference from real tmux, which keeps sessions
running after its window is gone.

### One emulator, not two

tmux had its own terminal emulator: a grid, a parser of a dozen escape
sequences and no more. With a shell in the pane it would have drawn every
full-screen program wrong -- no scroll regions, no alternate screen, no
cursor-key modes, no answers to a program's questions. `apps/terminal` has all
of those, and its link to a shell. So `apps/terminal` became a library as well
as a program (`src/lib.rs` the terminal, `src/main.rs` its window), and a pane
holds a `TerminalState`, drawn by the terminal's own `frame` under a
translation and hit-tested with the terminal's own targets. The alternatives:

| | Share the terminal (chosen) | Improve tmux's own emulator | A new crate both use |
|---|---|---|---|
| Emulators to keep correct | one | two, drifting | one |
| Work now | move `main` out, make five methods public | reimplement most of the terminal | move five thousand lines |
| Where a fix lands | both programs at once | wherever it was found | both |

The third is the second done later; nothing about the terminal is
terminal-app-specific enough to need it yet. Sharing paid at once: resizing
panes on every split found four bugs in the terminal (a shrink pushed the
prompt into history, the hidden screen lost its prompt, one saved-cursor slot
for two screens, `reset` killed the shell), and its grid moved to the
fixed-pitch face tmux had already fixed for itself. (§477's single layout walk
still holds: `relayout` sizes every pane from `Window::bounds`; the conversion
to cells is now the terminal's `resize_to_window`.)

### How a pane ends

tmux's default is that a pane closes when its program exits, whatever the exit
status; its `remain-on-exit failed` keeps a pane whose program failed. This
takes the second: the terminal app already closes its window on a clean exit
and keeps it, with the reason, on a failed one, and a pane is the same thing in
a smaller rectangle. A shell killed by a signal, or one that exits with the
status of a command that failed, leaves its pane showing why, and `prefix x`
closes it. The last pane closing ends its window, the last window its session,
and the last session the multiplexer -- the window closes, as tmux exits when
its last session does. Before, closing the last pane *detached* instead, which
left a window showing nothing that could run anything.

`prefix x` and `prefix &` ask first, with the answer typed (y) or clicked, as
tmux's default bindings do (`confirm-before`). Typed commands (`:kill-pane`,
`:kill-window`, `:kill-session`) do not ask; tmux's do not either.

### No server

Real tmux is a server that outlives its clients. This is one process with one
window: the sessions are its data and the shells its children, so they end
with it. A server needs a protocol, a lifetime and a way for a second window to
find the first -- a subsystem, not a fix -- and the value is leaving a long job
running with no window open, which nothing here needs yet. Until then the
detached screen says plainly that closing the window ends every session, and
`known-issues.md` → `[E] tmux is not a server` records what the real thing
would take.

**Where it lives:** `apps/terminal/src/lib.rs` (`TerminalState::start_with`,
`paste`, `set_focused`, `hang_up`); `apps/tmux/src/main.rs` (`Pane`,
`Multiplexer::{start_shell, tick, remove_pane, relayout, draw_pane}`).

**How to reverse:** the exit rule is one match arm in `Multiplexer::tick`
(`Response::Exit` closes the pane); the confirmation is `Confirm` and
`ask_close_*`; a server would replace `Multiplexer`'s ownership of the panes
and keep everything else.
