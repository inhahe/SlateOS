### TD-OILS-DEV-PATHS-OTHER-THAN-NULL-ARE-NOT-EMULATED. `/dev/zero`, `/dev/tty`, `/dev/stdin` and friends do not exist on the Windows dev host — 2026-08-04 — OPEN (host-only)

**Where:** `EMULATED_DEVICES` in `userspace/oils/src/interp.rs`, which lists
exactly one entry, `/dev/null` → `NUL`.

**What.** The reference bash on this host provides a `/dev` full of nodes that
the Windows build has no equivalent for. Measured — the primaries that hold for
each path, bash on the left, osh on the right:

| path | bash | osh |
|---|---|---|
| `/dev/null` | `-e -c -r -w -O -G` | `-e -c -r -w -O -G` ✅ (emulated) |
| `/dev/zero`, `/dev/full`, `/dev/random`, `/dev/tty` | `-e -c -r -w -O -G` | *(nothing)* |
| `/dev/stdin`, `/dev/fd/N` | `-e -p -r -O -G -L` | *(nothing)* |
| `/dev/stdout`, `/dev/stderr` | `-e -f -r -w -s -O -G -L` | *(nothing)* |

Note the last two rows are not fixed profiles: `/dev/stdin` and `/dev/fd/N`
describe whatever descriptor 0 currently *is* (a FIFO in the row above because
the probe was run from a pipe; a regular file when redirected), and
`/dev/stdout`/`/dev/stderr` likewise. bash reports them as symlinks (`-L`),
because on Linux that is what `/proc/self/fd/N` are.

**Not a parity bug in the shell's logic.** On SlateOS and on Unix these are real
nodes and the existing code stats them correctly; the gap is host emulation on
the Windows development host only. It is recorded because the corpus is measured
against that host, so any case touching these paths will diverge for a reason
that has nothing to do with the behaviour under test.

**What the proper fix looks like.** Two separable pieces:

1. *The character devices* (`/dev/zero`, `/dev/full`, `/dev/random`,
   `/dev/urandom`, `/dev/tty`) — add each to `EMULATED_DEVICES`, which gets the
   file tests for free, but only alongside a real open: `/dev/zero` must read as
   an endless stream of NULs and `/dev/full` must fail a write with ENOSPC.
   Listing a path in that table without backing the open would recreate exactly
   the incoherence the table exists to prevent (a redirect that creates a stray
   file into a device the tests say is a character device).
2. *The descriptor names* (`/dev/stdin`, `/dev/stdout`, `/dev/stderr`,
   `/dev/fd/N`) — these already work as redirection targets
   (`special_redirect_fd` handles them per the bash manual, on every platform). The file tests would
   have to ask about the descriptor rather than a path: `GetFileType` on the
   handle maps cleanly onto the primaries — `FILE_TYPE_CHAR` → `-c`,
   `FILE_TYPE_PIPE` → `-p`, `FILE_TYPE_DISK` → `-f` plus a real size for `-s`.
   `-L` would still diverge, since there is no symlink to find.

**Also on disk:** `D:\dev\null`, `D:\dev\full` and `D:\dev\test_dev` are stray
regular files left at the drive root by redirects from before `map_device_path`
existed (`D:\dev\null` holds the string `release not found`). They are harmless
now that both the opens and the tests are routed away from the literal path, but
they are why `[ -e /dev/full ]` still answers true on this host, and why a
case-insensitive `[ -e /dev/NULL ]` does too. Deleting them is the operator's
call, since they sit outside the repository.
