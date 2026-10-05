## B-TWELVE-CRATES-HAVE-NO-TESTS-AT-ALL-AND-REPORT-ZERO-PASSED (lane B, 2026-09-13) -- CLOSED for workspace members: 12 -> 7, two defects found

### Progress, and what writing the tests actually turned up

| crate | tests | defect found while writing them |
|---|---|---|
| `userspace/backup` | 9 | `Manifest::parse` turned a corrupted file SIZE into 0 and a corrupted mtime into the epoch, silently -- the two fields an incremental backup compares to decide what to copy |
| `userspace/sysctl` | 10 | none |
| `userspace/file` | 9 | none |
| `userspace/man` | 9 | none |
| `userspace/indexer` | 9 | `indexer find notes.txt` also matched `notes.txt.bak` -- a wildcard-free pattern was matched as a PREFIX, because the "last segment must match the end" arm is an `else if` and is unreachable for a single segment |
| `userspace/shell` | -- | stays on the list with a reason: it is not a shell, it is the toolchain validation program, and its whole behaviour is printing from `main()` |

**Two real defects in five crates**, both of the same shape: a wrong answer
that looks like a right one. A backup that silently records the wrong size
still backs up. A search that returns four files instead of one still returns
files. Neither would be reported by anything.

### Closed for workspace members

`userspace/nano` took the count to 7, and **every remaining crate is one that
`cargo test -p` cannot reach or that has nothing to reach.** Each carries its
reason in the baseline:

* Six `services/*` crates -- `hello`, `httpget`, `init`, `netstack`, `ticker`,
  `udpget` -- are not workspace members. They target `x86_64-unknown-none` and
  `cargo test -p netstack` answers "did not match any packages". Their bodies
  are raw syscall wrappers and the boot/poll sequences that call them.
* `userspace/shell` is not a shell: it is the toolchain validation program,
  and its whole behaviour is printing a fixed sequence from `main()`.

**`netstack` is the one worth stating separately**, because 4,329 untested
lines reads alarming and is not: the protocol logic it would be tested FOR
lives in the shared crates it delegates to, and those are tested --
`netproto` 79 tests, `netipc` 41, `netring` 9. What remains in `netstack`
itself is syscall wrappers and frame dispatch.

A measurement note, since it nearly became a wrong accusation: my first count
of those three said `netring` had **zero** tests, and I was one command away
from telling lane C that a crate the kernel links had none. The count was
wrong -- `grep -c` on a SINGLE file prints just the number, with no `file:`
prefix, so the `awk -F: '{s+=$2}'` summing field two got nothing. It worked
for the two multi-file crates and failed for the one-file one. Recounted
recursively before saying anything.

### What the original twelve were

### The original measurement



A crate with no `#[test]` anywhere reports

    test result: ok. 0 passed; 0 failed; 0 ignored

which is not a green suite. It is an empty one, and the two are
indistinguishable in the line anybody actually reads.

Measured 2026-09-13 across lane B's trees: **219 crates, 12 with no tests at
all, 14,301 lines between them.**

| lines | crate |
|---|---|
| 4,329 | `services/netstack` |
| 2,454 | `services/init` |
| 1,923 | `userspace/nano` |
| 1,319 | `userspace/indexer` |
| 1,212 | `userspace/file` |
| 1,198 | `userspace/man` |
| 597 | `userspace/sysctl` |
| 525 | `services/udpget` |
| 303 | `services/httpget` |
| 204 | `userspace/shell` |
| 167 | `services/ticker` |
| 70 | `services/hello` |

### How it was found, which is the argument for fixing it

`userspace/backup` was the thirteenth. It had 1,336 lines of backup and
restore and no tests, and I only noticed because I ran `cargo test -p backup`
after changing its exit status and read `0 passed` instead of skimming it.

Writing its first nine tests took under an hour and found a defect in the
first: `Manifest::parse` had `parts[2].parse().unwrap_or(0)` for a file's SIZE
and the same for its mtime, so a corrupted manifest line silently became a
zero-byte file dated the epoch. Every other malformed line in that parser is
an error; those two were the exception. An incremental backup decides what to
copy by comparing size and mtime against that manifest.

That is one defect per crate-hour on the first crate tried, in the one part of
a backup tool whose failure is invisible until a restore.

### Ratcheted, not swept

`scripts/check-untested-crates.py`, gate 39, with the twelve baselined. A
crate that is not on the list and has no tests fails the push; a crate that
gains its first test makes the baseline stale and also fails, because a
ratchet that does not tighten when the work is done is a standing permission
to regress.

**What the gate deliberately does not claim.** Counting `#[test]` says nothing
about whether the tests are good, and one trivial assertion satisfies it. The
gap it closes is the one between "no tests" and "one test" -- the gap where a
whole crate is invisible to every suite that runs.
