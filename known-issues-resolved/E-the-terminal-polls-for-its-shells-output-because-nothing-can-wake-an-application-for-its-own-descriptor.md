### [E] The terminal polls for its shell's output, because nothing can wake an application for its own descriptor -- 2026-09-24
**Status:** FIXED 2026-09-25 (lane E, on lane F's waker)

**What changed.** Lane F added a waker to the window loop (`App::wants_waker`,
`attach_waker`, `on_wake`) and made a clock that speeds up take effect at once.
The terminal and tmux ask for the waker and hand it to each link
(`termchild::Link::set_waker`); the link's reader thread wakes the window after
every chunk the shell writes, and a waiter thread wakes it when the shell
finishes -- which the end of the output does not announce when a background
job still holds the terminal open. The waiter blocks in the new
`libcall::pty::wait_exited` (`waitid` with `WNOWAIT`), which leaves the child
for the window's thread to collect, so the process id stays reserved while the
window might still signal it; for a link already dropped -- a closed tmux pane
-- the waiter collects it, so closed panes no longer leave finished processes
behind for as long as tmux runs. A terminal at a prompt now asks for no clock at
all; it asks only a link that cannot wake it, and while draining output a wake
left over. The text below is the entry as it was.

**In short:** The terminal cannot be told that its shell has written
something; it has to look. It looks every 16 ms while the shell is talking and
every 50 ms once it has been quiet for two seconds. So a terminal sitting at a
prompt wakes the machine twenty times a second to find nothing, and the first
key pressed after a pause waits up to 50 ms for its echo.

**Why.** `oswindow`'s event loop blocks on the compositor's socket and wakes
an application for two things only: the compositor's events and the clock the
application asks for (`App::tick_interval`). The shell's output arrives on the
pseudo-terminal's master, which the loop does not know about. The terminal's
reader thread drains the master continuously, so the shell is never held up,
but what it reads reaches the screen only on the next tick.

**Where.** `apps/terminal/src/main.rs`: `ACTIVE_POLL_MS`, `IDLE_POLL_MS`,
`ACTIVE_WINDOW_MS` and `TerminalState::tick_interval`. The 50 ms echo delay
has a second cause in `gui/window/src/app.rs` `sync_clock`, which never
shortens an armed deadline, so switching from the idle to the busy interval
takes effect only after the idle one fires.

**The proper fix** is lane F's: a waker the reader thread can call (ask 1 of
the request), or at least letting a shorter interval replace a longer armed
one (ask 2). Then the terminal stops asking for a clock on the child's behalf,
and this entry closes.
