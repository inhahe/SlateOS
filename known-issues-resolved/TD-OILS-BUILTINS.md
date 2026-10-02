### TD-OILS-BUILTINS. `osh` is missing the interactive-only bash builtins — ✅ **RESOLVED 2026-08-03**

**Resolution 2026-08-03.** Every builtin this entry listed now exists as a real
builtin: `history` and `fc` (see TD-OILS-MISSING-INTERACTIVE-BUILTINS), `logout`,
`suspend`, and finally `bind` including its inputrc reader (see
TD-OILS-NO-BIND-BUILTIN). `type -t` reports `builtin` for all of them and each
has differential-corpus coverage. Nothing here is gated on an interactive line
editor any more.

**What the entry got wrong:** "interactive-only" was the wrong frame. bash
answers all of these in a *non-interactive* shell — `bind -p` prints the whole
keymap, `history -s` edits a list a script can read back, `fc -s` re-runs an
entry — so each was implementable against measured bash output long before there
is a line editor. The lesson generalises: check what bash actually does in a
script before writing a builtin off as needing a terminal. (`suspend`'s actual
stop still waits on TD-OILS11 signal delivery; that is tracked there, not here.)

**Status correction 2026-07-27.** This entry used to head the list with `kill`,
`ulimit`, `complete` and `compopt` as unimplemented. They have all since landed
and `type -t` now reports `builtin` for each: `kill -l`/`-L` prints the full
Linux-x86 signal table, `kill -0`/`kill -SIG pid` works, and `ulimit -a`
reports the limit set. Only the list below is still missing.

**Where:** `userspace/oils/src/interp.rs` (`BUILTIN_NAMES`, the builtin dispatch
in `run_builtin`). `type -t <name>` reports nothing for these — except `fc`,
which resolves to the MSYS external on the host and would be `command not found`
on SlateOS.

**What:** these bash builtins are not implemented. All are interactive-only and
have no effect in a `-c`/script shell, so they are low priority until osh grows
an interactive line editor:

- **`bind`** — readline key bindings.
- **`history`** — command history list/manipulation.
- **`fc`** — history editing / re-execution (needs `history` first).
- **`logout`** — login-shell exit.
- **`suspend`** — stops the shell via SIGSTOP; additionally gated on
  job-control/signals (cf. TD-OILS13).

**Proper fix:** defer until there is an interactive REPL with a line editor and
a history store; then implement each as a real builtin (registered in
`BUILTIN_NAMES` so `type`/`command -v` report `builtin`) rather than shelling
out. `suspend` additionally waits on TD-OILS11 signal delivery.
