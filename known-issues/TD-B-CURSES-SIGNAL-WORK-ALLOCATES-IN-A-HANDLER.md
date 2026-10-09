## TD-B-CURSES-SIGNAL-WORK-ALLOCATES-IN-A-HANDLER (lane B, 2026-10-09) — FIXED on lane-b, pending a boot test on main

**Status:** FIXED on `lane-b` 2026-10-09, pending a boot test on `main`;
then it moves to `known-issues-resolved/`. Once a screen has been drawn, the work
a handler does -- `endwin`, and the repaint after a suspension -- allocates
and frees nothing, which
`screen::tests::the_work_a_signal_handler_does_allocates_nothing` measures
with a counting allocator in the C and UTF-8 locales (it counted 234, 4318
and 234 calls before; 0, 0 and 0 after). What it took, against the plan
below:

- `terminfo::Tparm` keeps upstream's `out_buff` and `fmt_buff` and a fixed
  twenty-slot stack, a string parameter on it held as its index rather than a
  copy, and `printf` writes digits from a stack array: `Tparm::expand` and
  `nc_expand` answer a slice of the kept buffer (the `Vec` forms copy it, for
  `tput`, `tic` and the rest). `tputs`' padding comes from a fixed run.
- `curses::term::Output` holds the sink and buffer, so that `putp_cap`,
  `tputs_cap` and `put_tiparm` write a capability straight from the entry and
  an expansion straight from `tparm` -- `Term::s`, the copier, is gone.
- `mvcur` appends expansions straight into its fixed move buffers;
  `_address_cursor` is an index (upstream points at the terminal's string
  too).
- The update borrows the lines it compares, and `Ctype::wcrtomb` writes into
  the caller's `MB_LEN_MAX` bytes; `_nc_screen_resume` restores colours by
  index.

What is left is `exit`'s, after the screen is put back: `atexit` handlers and
the standard streams' flush, as in upstream's handler.

**What.** `userspace/curses` does what ncurses' signal handlers do --
`endwin` on `SIGINT`/`SIGTERM`, the suspend-and-repaint on `SIGTSTP`, and
`end_and_exit` for a program's own handler (`watch`'s `die`) -- inside the
handler when the signal arrives while the program is *outside* any curses
call (design-decisions §1069; inside a call, the work waits for the call to
return). That work allocates: `Term::s` copies capability strings,
`tiparm` returns a fresh `Vec`, and the repaint after a suspension builds
the update's tables. `malloc` is not async-signal-safe: if the signal
interrupted the program while it was inside the allocator itself, the
handler's allocation can deadlock on the allocator's lock, and the program
hangs instead of suspending or exiting.

**Where.** `userspace/curses/src/signals.rs` (`post` -> `release` ->
`perform` -> `suspend`), and through it `Screen::endwin`,
`Screen::doupdate`, `Term::mvcur_wrap`, `update::screen_wrap`.

**How likely.** It needs the signal to land in the few instructions a
program spends inside `malloc`/`free` *outside* curses. `watch` and
`slabtop` spend nearly all their time asleep in a system call, where the
handler is safe. Not reproduced; reasoned from the code.

**Upstream has the same hazard and says so** (`lib_tstp.c`: "Much of this
is unsafe from a signal handler. But we'll _try_ to clean up the screen
and terminal settings on the way out."), though its steady state allocates
less: C's `tparm` and the capability strings use static buffers.

**What is already done.** The output buffer is reserved at its full size
(`(2 + lines) * (6 + columns)`) when the screen is made and keeps its
allocation across flushes (`Term::set_out_limit`, `Term::flush`), so
writing the escape sequences adds nothing.

**The proper fix.** Make the paths a handler can reach allocation-free in
the steady state: capability strings borrowed rather than copied (the
`Term::s` copies exist only to dodge a borrow of `self.entry` across
`self.tputs`), `tiparm` into a buffer the `Term` owns and reuses, and the
update's tables sized once per screen size. Then a test that installs a
global allocator which panics while a flag is set, sets the flag, and
raises each signal against a screen in each state -- proving no
allocation happens in the handler.

**When.** Next in lane B after the `slabtop` and `watch` ports that this
crate exists for. The refactor touches every output path of the port
(`mvcur.rs`, `update.rs`, `term.rs`, and `tparm` in `userspace/terminfo`),
and changes no byte it writes: `scripts/curses-diff.sh` (990 cases against
Ubuntu's libncursesw, on files and on terminals) checks it, and the two
programs' own terminal harnesses will check it a second time.
