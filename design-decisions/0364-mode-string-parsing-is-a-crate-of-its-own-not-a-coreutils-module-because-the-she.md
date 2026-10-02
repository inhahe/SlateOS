## §364 — Mode-string parsing is a crate of its own, not a `coreutils` module, because the shell and `install` need it and neither may depend on `coreutils`

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)
**Where it bites:** `userspace/modechange/` (new); `userspace/coreutils/src/bin/chmod.rs`,
`.../mkdir.rs`, `.../mkfifo.rs`, `.../stat.rs`; `userspace/install`,
`userspace/chown`; `userspace/oils`'s `umask` builtin.

**In short:** A "mode string" is the thing you type after `chmod` — `755`, or
`u+w`, or `a+rX,go-w`. Four separate places in the tree each had their own
hand-written parser for it, and they disagreed with each other on thirteen
points, so a script that set a permission with one utility and checked it with
another could get two different answers about the same file. The fix is one
parser everyone shares. The only question was *where to put it*: inside the
`coreutils` crate, where `chmod` already lives, or in a small crate of its own.
It went in its own crate.

### The options

| | *What changes* |
|---|---|
| **A. Its own crate, `modechange`** (chosen) | `install`, `chown` and the shell can call the shared parser; the workspace gains one more small crate. |
| **B. A module inside `coreutils`** | Only the utilities inside `coreutils` can call it; `install`, `chown` and the shell keep their own copies, and the disagreement survives in three places. |

### Why A

The deciding fact is that the callers are not all in `coreutils`, and cannot
be. `userspace/install` and `userspace/chown` are separate binaries with their
own crates; `oils` is the shell, whose `umask` builtin parses the same grammar.
Making any of them depend on `coreutils` to reach one parser would pull in
eighty-odd binaries' worth of code and a dependency edge from the shell to the
utilities — exactly backwards, since the utilities are the things the shell
runs. That is the same shape as §322, which put the regular-expression engine
in `userspace/ere` for the same reason: `grep`, `sed`, `awk`, `expr` and `osh`
all match the same patterns, and the shell may not depend on the utilities.

The cost of B is not that the parser is inconvenient to reach — it is that the
bug this work exists to fix would only be half fixed. The thirteen
disagreements were between `chmod`, `mkdir -m`, `install -m` and the shell's
`umask`; two of those four are outside `coreutils`. A shared module inside
`coreutils` would reconcile `chmod` with `mkdir` and leave `install` and the
shell still holding their own answers, which is a worse outcome than it looks:
the remaining disagreement would be *harder* to find, because three of the four
parsers now agree and the fourth looks like a local bug rather than a missing
abstraction.

The cost of A is one more workspace member — a real cost, since the workspace
already has hundreds and each one is a manifest to keep current. It is paid
once, and the crate is small and finished: it implements a grammar POSIX froze,
and the fixture that pins it is 24,480 rows measured from GNU chmod 9.4.

### What went in it, and what did not

`permission_string` (gnulib's `strmode` — the `rwxr-xr-x` rendering) is in the
crate even though it is a *printing* function rather than a parsing one,
because it is the same twelve bits with the same overload to get right: setuid,
setgid and sticky have no column of their own and are shown in the execute
slot, lowercase when the execute bit is set and uppercase when it is not. Three
callers need it — `chmod -v` to say what it did, and `ls -l` and `stat` to
report a mode — and `stat.rs` still carries a private copy, logged in
`known-issues.md` as the remaining one to converge.

The umask is *not* read inside the crate. `adjust` takes it as an argument, so
the crate performs no I/O and no syscalls, which is what lets the 24,480-row
fixture be a pure table-driven test rather than something that has to create
files. Reading the umask is the caller's job, and `chmod` only does it when the
mode was spelt symbolically — an octal mode ignores it entirely.

### Correction, same day: the shell's `umask` is *not* one of the callers

The reasoning above names three non-`coreutils` callers — `install`, `chown`
and the shell's `umask` builtin. Converting them turned up that the third is
wrong on the facts, and the conclusion is unchanged only by luck: `install` and
`chown` alone are enough to force the separate crate, so A is still right.

`umask` does not parse the coreutils grammar. It parses **bash's**, which looks
identical and is a strict subset with different semantics. Measured against
bash 5.2.21: it rejects `X`, `s` and `t` as permission characters, rejects the
copy-source triads (`u=g`), and rejects more than one operator in a clause
(`u+r-w`) — `modechange` accepts all of these. It works on the *complement* of
the mask rather than on a file's mode bits, so a `who`-less clause is not
umask-filtered (the value in hand *is* the umask). Its octal form has a
`07777` ceiling and then keeps only the low nine bits. And it reports errors by
quoting the single offending character, distinguishing an invalid operator from
an invalid permission letter, where coreutils quotes the whole spec.

So the shell keeps `parse_symbolic_umask`, and this is not a leftover to be
tidied later: converting it would *widen* the language `osh` accepts —
`umask u+X`, `umask u=g` and `umask 10000` would begin succeeding — and would
discard diagnostics that name the exact offending character. The general
principle worth carrying forward is that **two grammars that share a notation
are not thereby the same grammar**; the way to tell is to measure the reference
implementation of each, which is what turned this up. The comparison table is
in `known-issues.md` under
`TD-B-THREE-UTILITIES-STILL-CARRY-THEIR-OWN-MODE-PARSER`, and
`parse_symbolic_umask`'s own doc comment now carries the short form so nobody
re-opens the question from the code alone.
