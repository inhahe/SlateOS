### TD-OILS-SELECT-COLS. `select`'s menu never queries the real window size — OPEN (low priority, gated on a terminal-size syscall) — 2026-07-27

**Where:** `userspace/oils/src/interp.rs` — `Shell::menu_columns`, the sole
input to `select_menu`'s column arithmetic.

**What:** bash's `default_columns()` resolves the menu width in three steps:
`$COLUMNS` if it reads (via `atoi`) as a positive number; otherwise the real
window size — but *only* when `shopt -s checkwinsize` is set **and** the shell
is attached to a terminal, in which case bash issues a `TIOCGWINSZ` and also
publishes the result back into `$COLUMNS`/`$LINES`; otherwise 80.

osh implements the first and third steps exactly (including bash's `atoi`
quirks: leading whitespace and a sign are skipped, the first non-digit ends the
number, so `40x` is 40 and `abc` is 0 → 80). It never performs the middle step,
so with `$COLUMNS` unset osh always lays the menu out for 80 columns.

**Impact:** none for scripts — the divergence needs an *interactive* shell on a
terminal with `checkwinsize` on and `$COLUMNS` unset, and even then it only
changes menu cosmetics. Every non-interactive invocation (including the whole
differential corpus, which pipes) already gets bash's own 80. It is recorded
because the comment in `menu_columns` promises it.

**Proper fix:** two prerequisites, both outside oils. (1) A terminal-size query
on the target — SlateOS has no `TIOCGWINSZ` equivalent wired through to
userspace yet; on the Windows dev host it would be
`GetConsoleScreenBufferInfo`. (2) `shopt -s checkwinsize` support plus the
"publish back into `$COLUMNS`/`$LINES` after each command" hook, which bash
also uses for `help` and readline. Once both exist, `menu_columns` gains a
middle arm and TD-OILS-HELP-LAYOUT's width argument changes too.
