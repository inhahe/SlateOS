## 779. Userspace file access goes through `std::fs`, never a hand-rolled syscall

**Date:** 2026-09-05
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `sshd` opened, read and wrote every file by executing a raw CPU
`syscall` instruction with a SlateOS call number, instead of calling the ordinary
Rust file functions. On a real SlateOS machine that works. On the developer's own
Windows or Linux machine there is no SlateOS kernel to answer, so the code was
built to return "not implemented" for every file operation — which meant that on
every machine anyone actually tests on, `sshd` could not read *any* file at all.
That is not a small gap: the host key, the account database, the login banner,
the config file and `authorized_keys` are all files, so the entire server side of
public-key login was untestable. It is written down here because the choice
recurs: every userspace program has to decide how it reaches the filesystem, and
one of the two answers quietly costs all of its tests.

**The decision.** Programs in `userspace/**` reach the filesystem through
`std::fs`. A hand-rolled `syscall` for file I/O is a defect to be fixed on sight,
not a style. Raw syscalls remain correct for SlateOS calls that libc genuinely
does not export — `sshd` still issues them for the TCP handles and the monotonic
clock — but the moment `std` offers the operation, `std` is the route.

**Why this is not obvious.** The raw-syscall version was not sloppiness; it has a
real argument behind it. SlateOS is our kernel, its ABI is written down in
`kernel/src/syscall/number.rs`, and calling it directly means a userspace program
depends on nothing but that table. Going through `std::fs` inserts the libc port
and `std`'s platform layer between the program and the kernel, so a bug in either
becomes a bug in every program. For a *kernel* that reasoning is decisive. For a
userspace daemon it is not, and here is what it costs:

| | Raw syscall | `std::fs` |
|---|---|---|
| Depends on | the kernel ABI table only | the libc port and `std`'s unix layer as well |
| On a dev host | cannot work — the number means something else to Windows or Linux, so the only safe host arm is a stub that fails | works, against the host's own filesystem |
| Testable off SlateOS | no | yes |
| Shared with other programs | no — each crate writes its own wrappers | yes — one implementation, already exercised by everything else |
| Errors | an `i64` the caller must interpret | `io::Error` with a kind and a message |

The decisive column is the third. A file layer that fails on every development
machine does not merely go untested itself; it takes every code path *behind* it
with it. `sshd`'s public-key authentication was written, reviewed and shipped
with no test that a genuine client signature satisfies it, because the first
statement on that path was a file read that could not succeed where the tests
run.

**And the dependency objection does not survive contact with the target spec.**
`toolchain/x86_64-slateos.json` declares `os = "linux"`, `env = "musl"` and
`target-family = ["unix"]`, because `build-std` compiles a real `std` and `std`
picks its platform layer by `target_os` (see §619). So a SlateOS program already
links a real libc and already carries `std`'s unix layer, whether or not it calls
into them. Hand-rolling the syscall does not remove that dependency; it adds a
second, disagreeing copy of it beside it — which is the same shape as every other
bug in this stack, and it has already produced one: `posix` linked as an rlib is
a second libc with every syscall stubbed out (known-issues.md
`TD-B-THE-POSIX-RLIB-IS-A-SECOND-LIBC-WITH-EVERY-SYSCALL-STUBBED-OUT`).

**Alternatives considered.**

1. **Keep the syscalls; give the daemon a test seam** — an injectable "file
   reader" that tests replace with an in-memory map.
   *What changes:* the tests pass without touching a disk, and the code they
   exercise is not the code that runs on SlateOS.
   Rejected. The seam tests everything except the part that was broken. It also
   spreads: `HostKey::load_from_file`, `/etc/passwd`, the banner and the config
   file would each need one, and each would be a place where the tested path and
   the shipped path can drift.

2. **Keep the syscalls; make the host arm call `std::fs`** — i.e. the raw call
   under `cfg(target_vendor = "slateos")`, `std::fs` elsewhere.
   *What changes:* nothing observable; the tests pass and exercise a different
   implementation from the one that ships.
   Rejected for the same reason, and worse: the two arms are separately
   maintained implementations of one function, which is precisely the arrangement
   that produced eight live divergences in this stack already. Whatever the tests
   prove would be a fact about the host arm only.

3. **`std::fs` everywhere, including where libc exports nothing** — chase the
   remaining TCP and clock calls out too.
   *What changes:* nothing yet; there is no `std` API for the SlateOS TCP handle
   syscalls, so this is a proposal to invent one.
   Deferred, not rejected. When the libc port exports sockets, those wrappers
   should go the same way for the same reason. Until then a raw call is the only
   thing there is.

**A consequence worth stating separately: the cap.** `fs_read_file` previously
read into a fixed 64 KiB buffer and kept whatever fit. Moving to `std::fs` makes
an unbounded read available, and taking it would be wrong — `authorized_keys`
lives in the home directory of the account being logged into and is read *before*
that account has authenticated, so its size is chosen by an unprivileged local
user on behalf of an anonymous peer. The reader therefore keeps a limit (1 MiB)
but **refuses** an oversized file instead of truncating it. Silent truncation was
the worse half of the old behaviour: an `authorized_keys` one byte over the
buffer lost its last key with nothing reported anywhere, and a cut landing
mid-line left a partial base64 blob that read as a malformed entry rather than as
the key it was.

**Where it lives:** `userspace/sshd/src/lib.rs`, the `File access` section —
`fs_read_file`, `fs_write_private_file`, `fs_set_mode`. Commit `e0014a9b8`.
