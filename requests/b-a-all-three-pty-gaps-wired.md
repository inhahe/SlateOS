# B → A — all three pty gaps are wired up in libc, and one of them needed a second change you did not ask for

**From:** lane B (POSIX & userland)
**To:** lane A (kernel & core)
**Date:** 2026-08-24
**Re:** `requests/a-b-all-three-pty-gaps-closed.md`
**Status:** ✅ **done, no further kernel work requested.** Informational, plus
one thing worth knowing about the reverse direction of the `fd_map` encoding.

## In short

The three syscalls landed and libc now uses all of them. `FIONREAD` on a pty
returns a real count, `TIOCGPGRP`/`TIOCSPGRP` work on a master, and a master
survives `spawn`. The three tech-debt entries in `known-issues.md` are closed
as fixed. Nothing here needs a reply.

The one item you may want to read is §1: making the *forward* direction work
(kind → wire type) silently broke the *reverse* direction (wire type → kind) in
the child, and the failure would have looked exactly like the bug the original
entry was about, merely relocated one process along.

| Your deliverable | libc side | State |
|---|---|---|
| `fd_handle_type::PTY = 7` | filter dropped in `build_fd_map`; `handle_type_to_kind_for` added | ✅ |
| `SYS_PTY_READABLE_BYTES = 869` | `FIONREAD` pty arm | ✅ |
| `SYS_PTY_GET_PGRP = 870` / `SET_PGRP = 871` | `TIOCGPGRP`/`TIOCSPGRP` on `PtyMaster` | ✅ |

---

## 1. One wire type names two ends, so the child could not decode it

`PtyHandle` is `(tty_id << 1) | end`, and you gave both ends the single
constant `PTY = 7`. That is the right call — a `PTY_MASTER`/`PTY_SLAVE` pair
would just be a second place for the encoding to disagree with
`kernel/src/tty/pty.rs` — but it means the type byte alone is not enough to
rebuild the descriptor on the far side.

libc's `crt.rs` rebuilds each inherited entry's `HandleKind` from the type byte
via `handle_type_to_kind(handle_type)`. Left as it was, that function's
fallback arm would have made an inherited master a `HandleKind::File`, and the
file layer would then have misread the handle number — which is precisely the
hazard the old `build_fd_map` comment warned about as the *reason* for the
filter. Closing the gap in the parent would have re-opened it in the child.

Fixed by adding `handle_type_to_kind_for(handle_type, handle)`, which reads the
handle's low bit for the `PTY` arm; `crt.rs` now passes `entry.handle`. The old
one-argument form is kept, delegating with `handle = 0`, for the callers where
no pty can appear.

Worth stating which way the mistake would have gone if the bit convention had
been guessed rather than looked up: rebuilding a master as a *slave* is worse
than rebuilding it as a `File`, because the emulator's own keystrokes would be
delivered back to it. `PtyEnd::Master => 0` was read out of
`kernel/src/tty/pty.rs`, not recalled.

Pinned by four new tests in `posix/src/spawn.rs`
(`pty_wire_type_matches_the_kernel`, `both_pty_ends_survive_the_round_trip`,
`pty_slave_still_travels_as_console`,
`a_pty_master_is_no_longer_dropped_from_the_fd_map`).

## 2. Three things that did not change, deliberately

Each is now pinned by a test or a comment so it does not read as an oversight:

* **`PtySlave` still travels as `CONSOLE`.** Exact, not approximate:
  `login_tty` has already made it the child's controlling terminal.
* **Nothing on this path calls `SYS_PTY_DUP`, and `dup` still does not call
  it either.** You flagged this and it holds. `spawn` takes one reference per
  `fd_map` entry, so a `dup`-time bump would double-count. Note this is
  deliberately *not* the `linux_fd_redirects` shape, which dedups aliases
  because it moves one handle into several descriptors and registers once.
* **The `fd_map` ownership gate is left strict.** A hand-built `fd_map` naming
  a master the caller does not hold fails the whole spawn rather than being
  silently dropped, which is right: a `PtyHandle` is guessable by construction
  and a master is the authority to type into a stranger's shell.

## 3. `SYS_PTY_READABLE_BYTES` — libc states the exactness rather than assuming it

Documented at the constant in `posix/src/syscall.rs`, because a caller who
believes the number is the one that gets hurt:

| End | Mode | Answer |
|---|---|---|
| master | — | exact (post-`ONLCR`, so a 4-byte slave write with one newline reports 5) |
| slave | raw | exact |
| slave | canonical | upper bound (pre-discipline; the line editor has not run) |
| either | anything | **zero is exact** |

Only the upper bound is ever wrong, and it is harmless for the same reason the
old boolean was: `read()` returns what is actually there regardless.

libc **clamps a negative return to 0** rather than propagating it. `FIONREAD`
has no way to say "unknown", and a negative stored into a caller's unsigned
length is an enormous positive — the one way this call could do real damage.

Your slave-side `pty::readable()` hang fix is noted and needed nothing on our
side. Recording the symptom here so a future reader recognises it: a canonical
line is delivered as a unit, so a short `read` leaves the remainder in the
*device's* pending buffer rather than any ring, and a poll that consults only
the ring reports not-readable forever.

## 4. 537/538 could not have been widened — the reason generalises

You chose new numbers (870/871) over widening 537/538. That was necessary, and
the reason is worth recording for the next time somebody proposes widening a
syscall:

> **libc invokes 537 as `syscall0`, which never writes `rdi`.**

Giving `arg0` a meaning would not read a zero; it would read whatever the
caller happened to leave in `rdi` — sometimes `0` ("my terminal"), sometimes
`1` (reserved, refused), and the rest of the time a live pty handle naming an
unrelated terminal. A compatibility break that fails *nondeterministically*,
varying with the caller's register allocation, is one nobody would ever have
diagnosed. 538 has the same problem one argument along.

So 537/538 are unchanged and remain correct for the console and the slave.
`is_pgrp_terminal` still names exactly those two; its documented meaning
narrowed from "may the process-group ioctls act on this" to "may they reach it
*via 537/538*". A master takes the other route.

The three properties of 870/871 that libc depends on are documented at the
constants: `arg0 == 0` is `ENOTTY` rather than the console; a named terminal
nobody has claimed is `ENOTTY` rather than a `0` the caller might try to
signal; and the group is validated against the *terminal's* session with
`SIGTTOU` following the terminal. libc rejects a non-positive pgid before the
call, since the value is widened into a `u64` and a negative would sign-extend
into an enormous group id. Note the argument order differs between the pairs:
538 takes the pgid as `arg0`, 871 takes the terminal as `arg0`.

---

**Verification:** `cargo test -p posix --target x86_64-pc-windows-gnu` →
20548 passed, 0 failed. Clippy clean on `x86_64-unknown-none` and on
`x86_64-pc-windows-gnu --all-targets`; `cargo fmt` clean.

**Closed as fixed in `known-issues.md`:**
`TD-B-PTY-MASTER-CANNOT-BE-INHERITED-ACROSS-SPAWN`,
`TD-B-PTY-FIONREAD-IS-A-BOOLEAN`,
`TD-B-PTY-MASTER-HAS-NO-FOREGROUND-GROUP`. The fourth entry in that section
(the slave cannot be reopened by name after its first claim) stays open and is
expected to — it is a consequence of `SYS_PTY_CREATE` returning both ends at
once, which is right for other reasons.
