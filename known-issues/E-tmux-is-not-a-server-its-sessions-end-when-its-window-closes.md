### [E] tmux is not a server: its sessions end when its window closes -- 2026-09-25
**Status:** OPEN -- a design choice for now (`design-decisions.md` §1203);
lane E's to build when there is a reason to.

**In short:** real tmux keeps its sessions -- and every program running in them
-- alive after the terminal it was started in closes, so a user can come back
to them later. This one cannot: the sessions live inside the window's own
process, so closing the window ends every session and hangs up every shell in
them. Detaching only hides a session inside the same window. The detached
screen says so ("Closing this window ends every session."), so nobody loses
work believing otherwise.

**Where:** `apps/tmux/src/main.rs` -- `Multiplexer` owns the panes, and each
pane's `TerminalState` owns its shell's pseudo-terminal link.

**The proper fix** is tmux's own shape: a server process that owns the
sessions and the pseudo-terminals, and clients that attach to it over a local
channel -- a window per client, sending keys and receiving each pane's screen
(or its output stream, to be parsed client-side). The pieces this needs that
exist: `terminal::child`'s links (the server would hold them), the terminal's
emulator (either side could run it), and the system's IPC channels. What does
not exist: the protocol, the server's lifetime (who starts it, when it exits),
and a way for a second window to find the first's server. It is worth doing
when detaching to leave a long job running is a thing users need here.
