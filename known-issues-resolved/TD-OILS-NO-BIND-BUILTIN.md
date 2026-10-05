### TD-OILS-NO-BIND-BUILTIN. `bind` was missing, then answered from constants, then could not read an inputrc — 2026-08-01 — ✅ **RESOLVED 2026-08-03**

**Where:** `userspace/oils/src/interp.rs` — `builtin_bind`;
`userspace/oils/src/bind_keys.rs`; `userspace/oils/src/bind_tables.rs`;
`scripts/gen-oils-bind-tables.py`.

**Resolution:** closed in three passes — the builtin and its listings
(2026-08-02), live mutable tables (2026-08-03), and the inputrc reader
(2026-08-03). The record of each is kept below, because most of what it holds is
measured readline behaviour that the code depends on and no document else
states.

**Narrowed 2026-08-02, twice, and again 2026-08-03.** The builtin-count gap that opened this entry is
closed, and so are all of the listings. `bind` exists, `BUILTIN_NAMES` is 61
like bash's, and the option parsing, the `bind: warning: line editing not
enabled` warning, the phase order, every diagnostic, and **every listing** —
`-l`, `-p`, `-P`, `-v`, `-V`, `-q` for a known name as well as an unknown one,
and the three that a pristine readline really does print nothing for (`-s`,
`-S`, `-X`) — are byte-exact against bash 5.2.37 in all eight keymaps. Pinned by
`tests/corpus/a-bind-warns-then-works-in-phases.sh`,
`bind_warns_then_works_in_phase_order` and
`bind_listings_come_from_readlines_tables`, and verified exhaustively across all
8 keymap names × 8 listing letters plus all 174 `-q` names.

readline's compiled-in defaults now live in `bind_tables.rs`, captured by
`scripts/gen-oils-bind-tables.py` from a reference bash under
`INPUTRC=/dev/null` rather than transcribed — so the provenance is a command
instead of a claim, and the generator *checks* the keymap aliases rather than
assuming them.

**The tables are now live** (2026-08-03). `bind_keys.rs` holds readline's three
real keymap roots and its 46 variables as owned, mutable state, hung off `Shell`
as `bind_maps: Option<Box<Maps>>` — seeded from `bind_tables` on the first
command that could read or write a binding, so the overwhelmingly common case
(a shell that never says `bind`) still costs one `None`. It is cloned into a
subshell, which is what makes a mutation inside `( … )` correctly invisible to
the parent. Every mutating path writes through it: `-r`, `-u`, `-x`, the
`"KEYSEQ": function` / `"KEYSEQ": "macro"` operand form, and `set NAME VALUE`;
and every listing reads back out of it. The corpus case now dumps every listing
*before* the mutations and again *after* them, which is the half no shell
answering from constants can pass.

Three things in there are not what a reimplementation would guess, and each is
pinned by a unit test in `bind_keys.rs`:

* **The operand separator is one byte**, a colon or whitespace, whichever comes
  first — so `"\C-t" : yank` binds `\C-t` to the *target* ` : yank`… which is
  not a function name, and an unknown target is an **unbind**, silently, at
  status 0. There is no way to tell a typo from a deliberate removal.
* **`convert-meta` redirects a bind as it happens.** With it on (the default) a
  lone high byte is sent into the escape sub-map, so `"\M-t"` and a literal
  `\xf4` are the *same* binding, `[ESC, 't']`. That also decides the spelling on
  the way back out: the function dumper writes a prefixed ESC as `\M-` while
  `convert-meta` is on and `\e` while it is off, and the macro dumper always
  writes `\e`. Turning the variable off mid-script changes what `-p` and `-q`
  print for a binding that has not moved.
* **`editing-mode` moves `keymap`.** `set editing-mode vi` also sets
  `keymap` to `vi-insert`, and `bind -v` shows both.

**Reading an inputrc closed the last gap** (2026-08-03). `Maps::read_inputrc` is
readline's `_rl_read_init_file`: a line-oriented reader over `parse_operand`'s
grammar plus the `$` directives — `$if` on `mode=`, `term=`, `version OP N` or
the application name, with `$else`, `$endif` and `$include`. It does no I/O
itself; an `$include` names a file the *shell* resolves, through the
`bind_keys::Files` trait, because readline resolves it against the shell's
working directory rather than the including file's. `bind -f` uses it, and so
does the startup read: a non-interactive bash folds `$INPUTRC` (or `~/.inputrc`,
then `/etc/inputrc`) into the tables lazily, at the first `bind` that touches a
keymap, so osh seeds the same way in `Shell::seed_bind_maps`. Pinned by
`tests/corpus/a-bind-reads-an-inputrc.sh` and five unit tests in `bind_keys.rs`.

Four more measured surprises, on top of the three above:

* **`$if application=bash` is false.** readline has no `application=` form, so
  the whole string is read as an application *name*, and it is not `bash`.
  `$if Bash` (any case) is the true one.
* **A false `$if` hides a nested `$if` entirely** — nothing turns parsing back
  on but the matching `$endif`, so an `$else` inside a switched-off region stays
  switched off. An `$if` left unclosed simply ends with the file, in silence,
  while a stray `$else`/`$endif` is reported *and* the lines around it still
  apply.
* **The keymap is one live variable, and `-m` is a save-and-restore around it.**
  `set keymap` — as an operand or as a line of a file — steers every binding
  after it in the same call and outlives the call; `-m` steers each phase, shows
  through `bind -v`, and is then put back, so even a `set keymap` reached
  through a `-m -f` does not survive. This is why `builtin_bind` reads the
  keymap afresh at each phase instead of capturing it once.
* **A directory is not a failure.** `bind -f` on one is status 0 and silence:
  POSIX opens it and reads nothing. Windows refuses the open, so
  `read_inputrc_file` supplies the emptiness for the two hosts to agree.

**Two findings from probing the reference bash** (5.2.37, readline 8.2) that
shaped the above and still apply:

* **A non-interactive bash still answers every listing**, but prefixes it on
  stderr with `bind: warning: line editing not enabled`. That is exactly and
  permanently osh's condition, so osh emits the same warning and the same
  listing — which is what makes the whole builtin corpus-testable, since the
  corpus runs both shells non-interactively.
* **`/etc/inputrc` pollutes the tables, so a naive capture is not the default
  keymap.** With it loaded `bind -p` is 493 lines and `bind -s` is 10; with
  `INPUTRC=/dev/null` they are 488 and 0, and `bind -v` differs too. Any table
  transcribed from a plain `bind -p` on this machine would be that machine's
  config baked in as if it were readline's compiled-in default. Two
  consequences: (a) the embedded table must be captured under
  `INPUTRC=/dev/null`, and (b) a corpus case must export `INPUTRC=/dev/null`
  itself, or bash reads `/etc/inputrc` while osh does not and the two diverge
  for a reason that has nothing to do with osh.

Surface to reproduce (pristine, `INPUTRC=/dev/null`): `-l` 174 function names,
`-p` 488 binding lines, `-P` 175, `-v` 46 variables, `-V` 46, `-s`/`-S`/`-X`
empty; `bind -lpvsPVSX` 929. Per-keymap `-p`: emacs/emacs-standard 488,
emacs-meta 225, emacs-ctlx 200, vi/vi-move/vi-command 221, vi-insert 422. And
one readline quirk that only shows on a busy function: `-P` and `-q` list at
most five key sequences, closing with a full stop — but a sixth turns the tail
into `"a", "b", "c", "d", "e", ...` with no full stop at all.

**Every number above is superseded** (2026-08-25). They were measured against
the MSYS bash that happened to be on the developer's PATH, and two separate
things about that reference were wrong — see
TD-B-THE-SHELL-HARNESS-STILL-MEASURES-AGAINST-MSYS-BASH for the migration that
found them. Re-measured against the glibc bash 5.2.21 SlateOS actually targets,
under `INPUTRC=/dev/null` **and** `LC_ALL=C.UTF-8`: `-l` **173** function names,
`-p` **487**, `-P` **174**, `-v`/`-V` 46, `-s`/`-S`/`-X` empty, `bind -lpvsPVSX`
**926**; per-keymap `-p` emacs/emacs-standard 487, emacs-meta 224, emacs-ctlx
199, vi/vi-move/vi-command 220, vi-insert 421; and `/etc/inputrc` moves `-p` to
**494** while leaving `bind -s` **empty** rather than at 10.

* **The 174th name was `paste-from-clipboard`**, a Windows clipboard call that
  only a Cygwin readline is configured with. That is the one genuine platform
  difference in these tables, and it had been transcribed into osh as if it
  were readline's.
* **The locale was never pinned, and it decides `convert-meta`.** readline's
  `_rl_init_eightbit` takes the eight-bit branch for any `LC_CTYPE` that is not
  exactly `C` or `POSIX` (nls.c:168-186), which turns `convert-meta` *off*, and
  that variable decides whether a listing names the escape sub-map after the
  modifier it stands for (`\M-b`) or writes the byte as itself (`\eb`). This is
  *not* a platform difference, though it was once recorded here as one:
  measured four ways, MSYS bash and glibc bash agree with each other in each
  locale and disagree with themselves across the two. The committed table was
  the worst of both — its variables had been hand-corrected to the eight-bit
  set while its key sequences were still C-locale captures, so the file
  described one locale in its variables and the other in its tables.

`scripts/gen-oils-bind-tables.py` now **refuses** a capture that meets neither
condition rather than warning about it — it asks the reference shell for its
`$MACHTYPE` and stops unless it says `linux`, and it reads `convert-meta` back
out of the capture rather than trusting that `LC_ALL` took effect. The output is
committed and nobody re-derives it, so a warning would be read once and the
wrong table would live in the tree.

**`suspend` was the other half of this gap and is now closed** (2026-08-02).
It is implemented as the refusal, because every path through it is one: osh has
no job control (TD-OILS13), so it answers bash's own
`suspend: cannot suspend: no job control` at status 1. The refusal stands for
`-f` too — in bash `-f` forces past *both* the job-control and the login-shell
check and stops regardless, but osh has no way to stop and no way to be started
again, and on SlateOS "stop" would be a process-control IPC message that does
not exist yet, since the design forbids Unix signals for process control. If
that message ever arrives, this is the builtin that should send it.

⚠️ **Do not probe `suspend -f` against real bash.** It forces the suspend even
for a login shell, and the probe shell stops for good (confirmed: the probe hung
until the pid was killed). A `run-timeout.py` wrapper or a task that can be
killed by pid is the only safe way to run it — and it is why the corpus case
below deliberately omits `-f`.
