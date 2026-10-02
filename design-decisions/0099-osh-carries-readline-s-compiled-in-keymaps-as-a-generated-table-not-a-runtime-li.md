## §99 — osh carries readline's compiled-in keymaps as a generated table, not a runtime library

**Date:** 2026-08-02
**Decided by:** Claude (autonomous)

osh has no line editor and never will have readline. Yet `bind`'s listings are
not optional decoration: a *non-interactive* bash answers every one of them,
prefixing only a `bind: warning: line editing not enabled` on stderr, and that
is exactly and permanently osh's condition — so the whole builtin is
corpus-testable, and "print nothing" is a visible wrong answer rather than an
honest silence. Answering `-p`, `-P`, `-v`, `-V` and `-q NAME` requires
readline's *default keymaps*: 174 function names, five keymaps totalling 928
bindings, and 46 variables.

**The decision.** Embed them as `const` tables in
`userspace/oils/src/bind_tables.rs`, captured from a reference bash by
`scripts/gen-oils-bind-tables.py`.

**Why generated rather than transcribed.** 928 `(key sequence, function)` pairs
transcribed by hand would be wrong somewhere and there would be no way to tell
where. A script makes the provenance a command instead of a claim: rerun it
against any bash and the diff is the answer. It also *checks* what a
transcription would assume — that `emacs-standard` really is `emacs` and that
`vi`, `vi-move` and `vi-command` really are one map — by capturing all of them
and refusing to proceed if they differ.

**`INPUTRC=/dev/null` is load-bearing, and was the trap.** A plain `bind -p` on
this host gives 493 lines, not 488, and `bind -s` gives 10, not 0, because
`/etc/inputrc` is loaded. A naive capture would have baked one machine's
configuration into osh as though it were readline's compiled-in default. The
generator exports it, the module doc records the two numbers, and the corpus
case exports it too — otherwise bash reads `/etc/inputrc` while osh does not and
the two diverge for a reason that has nothing to do with osh.

**Addendum 2026-08-25 — two more conditions, and the numbers above are the
wrong reference's.** `INPUTRC` turned out to be one of *three* things a capture
has to pin, and the original capture pinned only it. The reference bash was the
MSYS one on the developer's PATH, and the locale was whatever the generator
inherited:

- **The platform decides the function list.** A Cygwin readline is configured
  with `paste-from-clipboard`, a Windows clipboard call, and a Linux one is not
  — 174 names against 173. osh answered `bind -l` with the Cygwin list for
  three weeks.
- **The locale decides `convert-meta`, and `convert-meta` decides the escape
  spelling.** readline's `_rl_init_eightbit` takes the eight-bit branch for any
  `LC_CTYPE` that is not exactly `C` or `POSIX` (nls.c:168-186), turning
  `convert-meta` off, and that variable is what decides whether a listing names
  the escape sub-map after the modifier it stands for (`\M-b`) or writes the
  byte as itself (`\eb`). Measured four ways: MSYS bash and glibc bash agree
  with each other in each locale and disagree with themselves across the two,
  so this is *not* a platform difference — it was simply never pinned, which
  made the capture irreproducible. The committed table had drifted into
  describing both at once: its variables had been hand-corrected to the
  eight-bit set while its key sequences were still C-locale captures.

The generator now pins `LC_ALL=C.UTF-8` — the locale osh actually runs in,
since osh is UTF-8-only (§104) — and **refuses** rather than warns: it asks the
reference shell for `$MACHTYPE` and stops unless it says `linux`, and it reads
`convert-meta` back out of the capture rather than trusting that the request
took effect, because a system without `C.UTF-8` falls back to `C` silently. A
warning would be the wrong instrument here: the output is committed and nobody
re-derives it, so anything short of a refusal leaves a wrong table in the tree.

Re-measured against glibc bash 5.2.21: 173 function names, 487 emacs bindings
(494 with `/etc/inputrc` loaded, and `bind -s` empty either way rather than 10),
46 variables. The tests now derive these from the table rather than spelling
them out — nine assertions had the number 174 written into them, so re-capturing
broke them all at once while saying nothing about which count was right.

**Why its own module.** `interp.rs` is already large enough that adding ~1200
lines of table to it ran rustc out of memory under `--test`
(`STATUS_STACK_BUFFER_OVERRUN`). That is recorded in known-issues; the split is
the mitigation.

**Alternatives considered.**

- *Link real readline.* Correct by construction and would bring `-f` inputrc
  parsing and mutation for free. Rejected: it is a C dependency on a shell that
  is meant to build for a `no_std`-adjacent target, for a feature whose only
  consumer is a listing. The tables are 40 KB of `const`; the library is not.
- *Reimplement readline's initialisation.* The tables are what
  `rl_initialize()` builds from static C arrays; transcribing the *arrays*
  rather than their output is the same data with more code between it and the
  answer.
- *Print nothing and document the gap.* What osh did for a day. It is a wrong
  answer that the corpus can see, and the exclusion list was growing.

**The cost, and the part since closed.** As first written the tables were
`const`, so `-u`, `-r` and `-x` — which in bash mutate readline's live tables
even with no line editor — were accepted, reported exactly as bash reports them,
and then forgotten. That was closed on 2026-08-03: `bind_keys::Maps` holds an
owned, mutable copy on `Shell`, seeded from these constants on first use, and
`bind -f` and the startup `$INPUTRC` read fold an inputrc into it. See
known-issues TD-OILS-NO-BIND-BUILTIN. The constants remain the *seed*, which is
the whole reason they must be captured under `INPUTRC=/dev/null`.

**How to reverse.** Delete `bind_tables.rs` and the generator, and cut the
`list_p`/`list_pp`/`list_v`/`list_vv` blocks and the `-q`-known path out of
`builtin_bind`. `bind_listings_come_from_readlines_tables` and
`tests/corpus/a-bind-warns-then-works-in-phases.sh` pin the behaviour.
