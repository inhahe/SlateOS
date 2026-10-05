## 727. `cp`'s three overwrite options are reproduced as three separate mechanisms, acting at three different points, because that is what makes them observably different

**Date:** 2026-08-30
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `cp` has three options that all sound like "what to do about a
destination that already exists": `-f` (force), `--remove-destination`, and `-n`
(no-clobber). They are not settings of one knob. In GNU they are three separate
fields consulted at three separate moments, and the differences show up in
ordinary use — which of them replaces a working symlink, which one can be given
a read-only file and succeed, and even the *order the two lines of `-v` output
come out in*. This `cp` now reproduces all three as three mechanisms rather than
collapsing them into one "overwrite policy" enum, and the harness pins the
differences.

### The temptation, and why it is wrong

The obvious model is one three-valued setting — *replace* / *unlink first* /
*refuse* — chosen by whichever option was given last, and consulted once, where
the destination is opened. It is smaller, it is easier to explain, and it gets
the common cases right. It is also wrong in at least five measured ways, because
the three options are not three answers to one question:

| | field in GNU's `struct cp_options` | consulted where | reached when the open would have *succeeded*? |
|---|---|---|---|
| `-f` | `unlink_dest_after_failed_open` | inside `copy_reg`, after `open` returned an error other than `ENOENT` | **no** |
| `--remove-destination` | `unlink_dest_before_opening` | in `copy_internal`, before `emit_verbose`, before anything is opened | **yes** |
| `-n` | `interactive = I_ALWAYS_NO` | in `copy_internal`, at the point the destination is found to exist | **yes** |

The middle column is the whole entry. `-f` is a *recovery*, not a *policy*: on a
destination that opens for writing it does nothing whatsoever, so `cp -fv a b`
and `cp -v a b` on an ordinary file are the same program. `--remove-destination`
is a policy and applies always, so it unlinks a perfectly writable file and
makes a new one. And `-n` is neither — it is a refusal, on stderr, with exit
status 1.

### What that buys, concretely

Five behaviours that the collapsed model cannot produce, each measured against
a from-source GNU 9.4 (see 726 for why not the installed one):

- **The order of `-v`'s two lines distinguishes `-f` from `--remove-destination`.**
  `-f` prints the arrow line and *then* `removed`, because the removal is a
  recovery from a failure already in progress. `--remove-destination` prints
  `removed` first, because removing is the first thing it does. A harness
  comparing exit status alone would call the two options identical.
- **`-f` does not rescue a dangling symlink**, which is the opposite of what
  "force" suggests. The refusal (`not writing through dangling symlink`) comes
  from the `O_CREAT|O_EXCL` arm, and `-f` retries by *creating* — which is what
  already failed. `--remove-destination` succeeds there, because it never
  reaches that arm at all.
- **`-f` *does* rescue a self-referential symlink**, which is the opposite
  surprise. There the failure is in `stat`, not in `open`: `ELOOP` is checked
  against `unlink_dest_after_failed_open` too (`copy.c:2326`), so `cp -f a loop`
  succeeds with `removed 'loop'`.
- **Only `--remove-destination` replaces a *working* symlink.** Plain `cp` and
  `cp -f` both write through it to its target, because the open succeeds.
- **`-n` skips the same-file check.** `cp -n a a` is silent and exits 0 where
  `cp a a` reports that the two names are the same file — GNU guards that check
  with `x->interactive != I_ALWAYS_NO` (`copy.c:2344`), because a refusal to
  replace is not a mistake worth reporting.

### The shape in the code

- A `Dest` enum returned by `stat_destination`, with an `Opaque` variant for
  "the destination exists but cannot be stat'd", which is what models GNU's
  `ELOOP` case above. Without a distinct variant for it, the self-loop
  behaviour is unreachable.
- `create_destination` returns a `DestError` that distinguishes `Io`,
  `Dangling` and `Remove`, because `-f`'s decision is *about which error it
  was*, and a flattened `io::Error` cannot carry "this was the dangling-symlink
  refusal, which retrying will not fix".
- `-n` refuses in `copy_one`/`copy_entry` before the destination is touched,
  and is guarded by "the source is not a directory" so that `cp -rn` still
  descends — the refusal is about files, not about the walk.

### The cost

More surface than one enum: three fields on `CpFlags` instead of one, a
four-variant `Dest`, and a three-variant error where an `io::Error` would
otherwise do. That is the price of the five behaviours above, and each of them
is pinned by a case in `scripts/cp-diff.sh` section 16 and by a unit test, so a
future simplification that loses one will fail rather than pass quietly.

### How to reverse

Collapsing the three back into one policy enum is mechanical, and section 16 of
`scripts/cp-diff.sh` would report exactly which behaviours it cost — which is
the reason that section is written as fifty-eight small cases rather than a
handful of large ones.

### Where

- `userspace/coreutils/src/bin/cp.rs` — the module-doc section "The three
  overwrite policies are three different options", `Interactive`, the three
  `CpFlags` fields, `Dest`/`stat_destination`/`is_eloop`, `refuse_no_clobber`,
  `remove_destination_first`, and `create_destination`'s `DestError`.
- `scripts/cp-diff.sh` section 16 — 58 cases, all agreeing with GNU 9.4.
- `known-issues.md`, `B-CP-R-COULD-NOT-REPLACE-AN-EXISTING-SYMLINK` — a bug in
  the *existing* recursive copy that writing section 16 uncovered.
