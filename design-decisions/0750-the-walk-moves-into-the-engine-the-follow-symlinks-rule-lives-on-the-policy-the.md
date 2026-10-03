## 750. The walk moves into the engine: the follow-symlinks rule lives on the policy, the options *borrow* the backup, and the run's stdout is a trait object

**Date:** 2026-09-03
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** `cp` and `mv` each had their own code for deciding *what to copy*
— walking a directory, choosing the destination name, refusing the copies that
must be refused. Stage 4 of the copy-engine extraction moved `cp`'s 1,180 lines
of it into the shared `copy` module so that `mv` can drive the same code
instead of its own. Three choices inside that move had a real argument on both
sides. §749 records stage 3's three; these are stage 4's, and like those they
are the kind a later reader would "simplify" straight back to the rejected
option.

### 1. `cp -H`/`-L`/`-P`'s rule lives on `Deref`, not on `Opts`

Three options say whether a copy follows a symbolic link or duplicates it, and
a fourth thing decides it when none of them is given — `-r` flips the default,
so plain `cp link d` writes a *file* and plain `cp -r link d` writes a *link*.
That rule is four lines and it must exist in exactly one place, because a
second copy of it is a place for the two to disagree about a command line
nobody tested.

The obvious home is `Opts`, the engine's options struct, which already holds
both the policy and `recursive`. It is the wrong home: **`cp` needs the answer
before it has an `Opts` to ask.** Three sites in `cp.rs` decide how to `stat`
an operand, and they run while the options are still being assembled; `cp`'s
parse tests pin the whole table of answers and would have to fabricate a
umask, a backup policy and a program name to build an `Opts` and ask it.

So the rule lives on the `Deref` enum itself, with `recursive` passed in as an
argument, and both `Opts` and `cp`'s `CpFlags` are three-line delegations to
it.

| | rule on `Opts` | rule on `Deref` |
|---|---|---|
| where it is written | once | once |
| `cp`'s three pre-`Opts` `stat` sites | must build a whole `Opts` from values they do not have | call `flags.follow_operand()` |
| `cp`'s parse tests | assert on a fabricated `Opts` | assert on the parsed policy itself |
| cost | — | `recursive` is passed at each of the four call sites instead of being read from `self` |

The cost is real and it is the whole argument for the other side: the
`recursive: bool` parameter can be passed *wrongly*, where a field cannot.
It is bounded by there being exactly four callers of the four methods, all in
these two files, and by `Opts`' delegations being the only path the engine
itself ever takes.

### 2. `Opts` borrows the backup policy rather than owning it

`Opts` gained a `--backup` policy in this stage. `backup::Backup` is `Clone`
but not `Copy`, because the suffix can come from `$SIMPLE_BACKUP_SUFFIX` and so
is an owned `Vec<u8>` rather than a borrow of `argv`. Holding it *by value*
would therefore do two things, and both are worse than a lifetime:

- It takes `Copy` away from `Opts`, which is passed by value at every call
  inside the engine and is a dozen scalars.
- It allocates **once per file copied**. `cp` rebuilds its `Opts` at every
  call, deliberately — that is what makes `-b` on the command line reach the
  preserve tail — so "per call" here means per file, not per run.

Against that, the lifetime parameter appears in every signature that names
`Opts` or `Run`. That is a readability cost paid in about a dozen places, once,
versus an allocation paid per file forever. Both programs own their backup
policy for the whole run, so the borrow is free and cannot dangle.

### 3. `Run::out` is `&mut dyn Write`, not a second type parameter

`Run` already carried `err` as a generic `E: Write`, and stage 4 gave it
`out` — where `--verbose` prints `'a' -> 'b'`. Symmetry says make it a second
type parameter `O: Write`.

It is the wrong shape here, for the reason §749 gives for `Clobber` and one
more. Ten of the engine's functions name `Run` in their signature and **six of
them never write to stdout at all**; a second parameter puts `O` in all ten,
and every intermediate function that merely passes a `Run` along has to name a
type it has no opinion about. `dyn Write` is also already this crate's house
style for a stdout that is threaded rather than used —
`hardlink::force_link` takes `out: &mut dyn io::Write` for exactly this reason.

The cost is an indirect call per `-v` line, which is one per file and only when
`-v` was given, against a vtable-free call in a function that is doing a
`write(2)` anyway. `err` stays generic because it is written on a per-*file*
failure path that the tests assert on directly.

### What this stage did *not* decide

Stage 4 makes §749's choice 2 obsolete rather than wrong: `Clobber::Unlink`
carries a stdout and a `verbose` flag only because the code that built it could
not reach the options, and now that `Run` holds `opts` and `out` together it
can. Collapsing `Dest`/`Clobber` into "does it exist" is a separate change with
its own reasoning, deliberately not folded into a move that must not change
behaviour.

### How the stage was certified

As stage 3 was, and for the reason `known-issues.md` gives: not by the test
suite but by the two differential harnesses, run before and after, requiring
`cp 581 passed, 0 differed, 30 differ on purpose` and `mv 360/0/11` on both
sides. A stage that moves code without changing behaviour and *does* move those
numbers has a bug in the move.

The mechanical part of the move was scripted rather than hand-edited, and every
rewrite rule asserted the number of times it was expected to fire — a rule that
fired the wrong number of times aborted the script. That caught one wrong
assumption immediately (a fourth `"cp"` literal in a shape the rule's author had
not counted), which would otherwise have compiled and been certified only by the
harnesses two hundred and fifty seconds later, if at all.
