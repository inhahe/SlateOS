## 1069. Curses' signal handlers wait for a curses call in progress, instead of running inside it

**Date:** 2026-10-09
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** a full-screen program has to put the terminal back the way it
found it when the user presses Ctrl-Z (suspend) or Ctrl-C (interrupt), and
ncurses does that from inside its signal handlers -- code that runs the
moment the signal arrives, in the middle of whatever the program was doing.
If the program was itself half way through a curses call at that moment,
upstream's handler works on the screen while that call has it half changed.
Our port (`userspace/curses`, for `watch` and `slabtop`) instead lets the
interrupted curses call finish first and then does the handler's work: the
user sees the same suspend or exit, a few microseconds later, without the
chance of a garbled screen or a crash.

### What it is

The process's screen sits behind a flag that a curses call takes with one
atomic swap and gives back when it returns (`signals::with`). The handlers
for `SIGTSTP`, `SIGINT` and `SIGTERM`, and `end_and_exit` -- what a
program's own handler calls, as `watch`'s does -- first note what they were
asked to do, then try to take the flag:

| when the signal arrives | upstream's handler | ours |
|---|---|---|
| outside any curses call | does its work now | does its work now -- the same |
| inside a curses call | does its work now, on a screen half changed | leaves it noted; the call does it as it returns |
| inside `doupdate` | `SIGTSTP` is ignored there (both) | the same |

Everything else is upstream's: the handlers are installed only where the
program left the default action, `SIGTSTP` is ignored for the length of an
update, a second interrupt during the clean-up exits at once, and the
suspend sequence -- modes saved, `endwin`, the stop, input thrown away,
modes read again, the repaint -- is `handle_SIGTSTP`'s, step for step.
`scripts/curses-diff.sh` raises each signal against both libraries on a
pseudo-terminal and finds the same bytes.

### Why

* **Upstream's version is a data race by construction.** `endwin` and
  `doupdate` read and write the very structures the interrupted call was
  changing; in C that is undefined behaviour that usually works, and in Rust
  it would be two live mutable references to one screen, which the language
  does not permit. The flag is what makes the port sound at all.
* **Nothing a user can see is lost.** A curses call takes microseconds and
  does not block on input (this crate has no `getch`), so "after the call"
  is, to a person, "now". The output is upstream's because the work is
  upstream's.
* **It is cheaper than the alternative designs.** A helper thread that did
  the work in ordinary context would change which thread signals interrupt
  and would need its own answers for job control (a stopped thread, the
  terminal's `SIGTTOU`); blocking the signals during every curses call would
  cost two system calls per call.

### Against

* **What upstream does still happens in one case:** a signal that arrives
  while the program is outside curses runs `endwin` inside the handler, as
  upstream's does, and `endwin` allocates (a few small vectors). If the
  program was inside the memory allocator at that instant, the handler can
  deadlock in it. Upstream accepts the same hazard in its own words ("Much
  of this is unsafe from a signal handler. But we'll _try_ to clean up the
  screen and terminal settings on the way out."). The output buffer is
  reserved whole when the screen is made, so writing to the terminal is not
  among the allocations. Tracked as
  `known-issues.md` -> `TD-B-CURSES-SIGNAL-WORK-ALLOCATES-IN-A-HANDLER`.
* A signal in the middle of a curses call takes effect a call later than
  upstream's -- after, say, an `addstr` finishes rather than half way
  through it. No harness or program has been found that can tell.

### Where it lives

`userspace/curses/src/signals.rs`: `Global`, `with_slot`, `with`, `post`,
`release`, `perform`, the handlers and `suspend`. `Screen::doupdate`
(`screen.rs`) brackets the update with `signals::tstp`.

### How to reverse

Make `post` call `perform` directly instead of deferring when the flag is
held -- which reintroduces the aliasing above, and so cannot be done without
giving up `&mut Screen` for raw pointers throughout.
