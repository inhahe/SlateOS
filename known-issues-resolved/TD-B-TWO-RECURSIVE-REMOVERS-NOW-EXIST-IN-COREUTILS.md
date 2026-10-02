## TD-B-TWO-RECURSIVE-REMOVERS-NOW-EXIST-IN-COREUTILS (lane B, 2026-09-03) — FIXED 2026-09-03 (lane B)

**In short:** deleting a directory and everything inside it is now written out
twice in our coreutils — once in `rm`, and once in `mv`. `mv` grew its copy on
2026-09-03, when a move across a disk boundary stopped being refused: such a
move is a copy followed by a *recursive* delete of the original, and `mv` had
no recursive delete to call. The two are not interchangeable today (see below),
so this is duplicated behaviour rather than duplicated code that could simply
have been shared — but two implementations of "delete a tree" will drift, and
a fix or a security hardening applied to one will silently miss the other.

**Where.**

| | file | entry point |
|---|---|---|
| `rm`'s | `userspace/coreutils/src/bin/rm.rs` | `Rm::remove_tree` |
| `mv`'s | `userspace/coreutils/src/bin/mv.rs` | `remove_tree` (called from `remove_source`) |

**Why the second one was written rather than the first one reused.** Recorded
in full as `design-decisions.md` §751; the short version is three facts:

- GNU does share one implementation, but the thing it shares is `remove.c` — a
  *library* module that both `rm.c` and `mv.c` link against, and that neither
  of them owns. Our `rm.rs` is not that: it is a binary, and its walk is a
  method on `Rm`, which is the parsed command line.
- `Rm` carries eight knobs — `-i`/`-I`/`--interactive`, `-f`, `--one-file-system`,
  `--preserve-root`, `-d`, `-v`, and the prompt state that `-I` needs — none of
  which `mv` has an answer for. Reusing it means either inventing values for
  all eight or splitting the walk away from the struct, which is the real fix.
- The stage that introduced this was certified by "the two differential
  harnesses do not move" (`cp-diff` 581/0/30, `mv-diff` 361/0/10). Refactoring
  a third binary inside that stage would have destroyed the only check that the
  stage was clean.

**What actually differs today**, so nobody merges them by assuming they match:

| | `rm` | `mv` |
|---|---|---|
| prompts before deleting | yes, under `-i`/`-I` | never — the copy already succeeded |
| `--one-file-system` | honoured | not applicable; the source is one device by construction |
| what `-v` prints | `removed 'f'` / `removed directory 'd'` | the same two sentences |
| a child that cannot be removed | reports it, keeps going, and stays **silent** about every ancestor | the same |
| an *empty* directory that cannot be **read** | held the read error, reported it, and stopped — GNU removes it | held it, called `rmdir` anyway, GNU's rule |

**The drift this entry was filed to catch arrived within the day, and the
second row is it.** As first written this table claimed the divergence was
ancestor silence — that `rm` "reports the parent too" while `mv` does not.
That was wrong: `rm.rs` already had the rule (`Rm::remove_tree`, "Silence,
deliberately"), and it is GNU's `mark_ancestor_dirs` behaviour on both sides.
Comparing the two walks properly instead turned up a real one, and in the
opposite direction from the guess — the *younger* walk had the rule and the
older one did not.

`mv.rs` carried `is_uninformative`, upstream's list of errnos an `rmdir` may
answer that say less than an earlier `opendir` failure did (`remove.c:424`).
`rm.rs` had no such notion, because it never got as far as the `rmdir`: a
directory it could not list was reported and abandoned. But listing a directory
needs `r`, while removing an empty one needs only `w`+`x` on its *parent*, so
`chmod 300 d` on an empty `d` is a directory nobody can read and anybody can
delete — and GNU deletes it. Measured against GNU 9.4 on 2026-09-03:

```text
$ mkdir d && chmod 300 d && rm -rv d
GNU : rc=0  removed directory 'd'
OURS: rc=1  rm: cannot remove 'd': Permission denied     (directory still there)
```

Fixed the same day, and the fix is what created the shared module this entry
asks for: `userspace/coreutils/src/remove.rs` now holds `is_uninformative` and
a `blame(held, failure)` that states the substitution rule once for both
binaries. `rm` gained `Rm::remove_inaccessible_directory` and upstream's third
prompt, `attempt removal of inaccessible directory 'd'? `, which it had never
implemented at all. `-r` and `-d` part company there: under `-d` the question
can be asked, because there is nothing to descend into; under `-r` the question
would have to be `descend into …?` or `remove …?` and *which one it is* depends
on the listing that just failed, so a question that comes due is fatal — but
with no question due, the `rmdir` still runs.

**Why the harness missed it, which is the part worth keeping.** `rm-diff.sh`
section 15 had nine cases for unreadable directories and every one of them used
a *non-empty* one (`tree/sub` holds `b.txt`). There GNU's `rmdir` fails too, and
prints the same substituted `Permission denied` — so a remover that never
attempted the `rmdir` at all is indistinguishable from one that did. The
missing case was not an exotic one; it was the *simpler* one. Fifteen cases were
added, and against the pre-fix binary eleven of them differ (the other four pin
the boundary from the other side: modes 0100/0000/0500, where erroring is
correct, so a future over-eager fix is caught too).

**Proper fix.** Lift the walk out of both binaries into a
`userspace/coreutils/src/remove.rs`, shaped the way `copy.rs` already is after
the five-stage copy-engine extraction — i.e. an `Opts` struct of the knobs, a
`Run` carrying the output streams, and a free function taking both. `rm.rs`'s
`Rm` becomes a producer of `remove::Opts` exactly as `cp.rs`'s `CpFlags`
became a producer of `copy::Opts`; `mv.rs` passes an `Opts` with the prompting
and the `--one-file-system` knobs off. The ancestor-silence rule then lives in
one place and both binaries get it.

**Sequencing — the blocker is gone.** This was to wait on
`TD-B-RM-WALKS-BY-PATH-SO-A-SYMLINK-SWAP-CAN-REDIRECT-A-REMOVAL`, because that
entry's fix changes the walk's *signature* — it threads a
`(dirfd, path_for_messages)` pair through every level instead of a path, so
extracting first meant extracting twice. That entry was **FIXED 2026-09-03**:
`rm`'s walk now carries `Loc { dir, path }`, the descriptor for the syscalls and
the string only for the messages. The signature the shared module has to take is
therefore known, and the extraction is unblocked.

`mv.rs`'s walk still has the weakness `rm`'s no longer does: it calls
`fs::read_dir`, `fs::remove_file` and `fs::remove_dir` on joined path strings,
so a symlink swapped in mid-walk is followed. It is narrower there — the tree
being removed is one the user just successfully copied, so the window is
smaller — but it is the same bug, and it is now the *only* place in coreutils
that still has it. Sharing `rm`'s walk is what fixes it, which makes the
extraction a security fix rather than only a tidiness one.

**What is done and what is left.** Nothing is left; the last two rows closed on
the same day the first three did.

| | state |
|---|---|
| `remove.rs` exists | done — `is_uninformative`, `blame`, 4 tests |
| the errno-substitution rule is shared | done — both binaries call `remove::blame` |
| `rm` implements GNU's inaccessible-directory rule | done — 15 harness cases |
| the *walk* is shared | **done** — `remove::Remover`, called by both |
| `mv`'s walk stops following a mid-walk symlink swap | **done**, and it followed from the row above exactly as predicted |

### How the extraction came out

`userspace/coreutils/src/remove.rs` now holds the whole walk, growing from 170
lines to 888. `rm.rs` went from 2,432 lines to 1,934, but that headline number
undersells it: the cut is entirely above the `#[cfg(test)]` line, where `rm.rs`
went from 1,262 lines to 744 — it lost **41% of its production code**. The test
module is unchanged in size (1,169 → 1,189), because the tests that belonged to
the walk moved into `remove.rs` alongside it and the ones that stayed are the
ones that test what `rm` still does. `mv.rs` lost `remove_tree`,
`report_unremovable` and `announce_removed` outright. There is one `Verdict`,
one `Loc`, one `Interactive`, one set of three prompts and one `-v` sentence
pair in the zone, where the day before there were two of each.

The shape is the one this entry prescribed, and it is `copy.rs`'s:

| | `cp` → `copy.rs` | `rm` → `remove.rs` |
|---|---|---|
| the parsed command line | `CpFlags` | `rm::Options` (9 fields) |
| what it hands the engine | `copy::Opts` | `remove::Opts` (6 fields) |
| the streams | `copy::Run` | `remove::Remover` |

`rm::Options::walk_opts` is the projection. Six of the nine fields go through
unchanged; the three that do not — `preserve_root`, `preserve_all_root`,
`presume_tty` — are precisely the three that are decided **per command-line
operand** and have no meaning for an entry found by walking, and they are
precisely the three `Rm` keeps for itself. Between the two halves every parsed
option is accounted for once and once only, which is the property that stops
the drift recurring: handing the walk a struct with three fields it must
remember never to read is how the two copies came apart the first time.

The dividing line, stated once so the next reader does not have to infer it: a
command-line operand. `--preserve-root`, the `.`/`..` refusal, `-I`'s single
up-front question and gnulib `fts`'s trailing-slash rule all apply to *the thing
the user typed*, and `mv` has no such thing to apply them to. Everything above
that line stayed in `rm.rs`; everything below it moved.

**What `mv` gained by inheriting the walk**, beyond no longer being a second
copy:

* **The symlink-swap fix.** The old walk called `fs::read_dir`,
  `fs::remove_file` and `fs::remove_dir` on joined path strings; the shared one
  resolves every syscall through an open parent descriptor (`coreutils::dirfd`),
  which a swapped component cannot redirect. This was the last place in
  coreutils that still walked by path.
* **The listing is read in full before any removal**, as `fts` does, instead of
  being interleaved with `readdir` — strictly the safer order.
* **A `readdir` iteration error is now blamed on the child** rather than the
  parent, which is GNU's behaviour and was not `mv`'s.
* **One fewer `lstat`.** `remove_source` is handed the `symlink_metadata`
  `move_one` already took (`Stat::from_metadata`) instead of taking a second.
  All seven call sites pass an `lstat`, so the second lookup bought nothing but
  a window in which the answer could change — and the field it decides is
  whether the source is a directory. Reusing the earlier stat closes that window
  in the only safe direction: a source swapped for a directory after the copy is
  unlinked rather than descended, so a tree the user never named survives.

**What did not change, checked rather than assumed:** child paths are now joined
with `/` rather than the host separator, which is harmless — the target is unix,
the host tests assert suffixes only, `mv-diff` runs on Linux, and `rm.rs` had
always behaved this way. `mv`'s `Opts` are `mv.c:87`'s `rm_option_init` verbatim
(`recursive` on, `interactive` never, `ignore_missing_files` off,
`one_file_system` off) with `verbose` from `x.verbose` at `mv.c:238`, and
`answers` is `None` rather than `mv`'s `-i` channel, because `-i` is a question
about *overwriting a destination* and is already settled by the time the source
is removed.

**Certified by** the two differential harnesses that certified the half before
it — `scripts/rm-diff.sh` and `scripts/mv-diff.sh`, both against GNU 9.4 — plus
`scripts/coreutils-check.sh` (host clippy, host tests, Linux tests). `mv-diff`
is the load-bearing one: it is what says a walk that now resolves through
descriptors behaves identically to the one that resolved through strings.

**Tests moved with the code they test.** `joining_drops_one_trailing_slash_only`
went from `rm.rs` to `remove.rs` because `join` did, and `remove.rs` gained
`worse_is_a_symmetric_maximum` — `worse` is a max over a three-valued lattice
folded in `readdir` order, so a `worse` that answered differently for
`(Declined, Abandoned)` than for the reverse would make a parent's fate depend
on the order the filesystem happened to list its contents. In `mv.rs`, the two
tests that had asserted against the deleted `announce_removed` shim were
rewritten to drive real `remove_source` calls against real files, so the
coverage lands on the code that actually prints rather than on a helper that no
longer exists.
