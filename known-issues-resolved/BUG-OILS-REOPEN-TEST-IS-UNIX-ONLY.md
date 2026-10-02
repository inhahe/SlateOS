### BUG-OILS-REOPEN-TEST-IS-UNIX-ONLY. `a_reopened_descriptor_starts_at_zero_and_does_not_move_the_shells_cursor` fails on the Windows dev host — 2026-08-26 — LANE B — FIXED 2026-08-26 (`ae285d901`)

**Resolution (recorded 2026-10-01).** Fixed the day it was reported, the way
the request suggests: the test now asserts the documented dup fallback on the
dev host and the re-open where procfs exists, so the fallback is covered rather
than excused (`userspace/oils/src/interp.rs`, the test's doc comment names the
request). The heading went on saying REPORTED; the entry is kept below as filed.

**In short:** `cargo test --workspace` has exactly one failing test, and it is a
test rather than a bug. The shell (`osh`) can be told to read a file "through a
descriptor number" — `read b </dev/fd/3`. On Linux that re-opens the file from
the beginning; on Windows there is no way to name a file that way, so the shell
falls back to sharing the existing descriptor, and the read continues from where
the last one stopped. The fallback is deliberate and documented. The test is
not: it asserts the Linux answer on every host, so it is red for anybody running
the suite on this machine. Nothing a user can do is broken by it — but a
permanently-red suite is, because it trains everyone to ignore the failure line.

**Repro:**

```
cargo test -p oils --lib --target x86_64-pc-windows-gnu a_reopened_descriptor_starts_at_zero
```

```
assertion `left == right` failed        (userspace/oils/src/interp.rs:106519)
  left: "[one][two][]\n"
 right: "[one][one][two]\n"
```

**Where:** `userspace/oils/src/interp.rs`. The test arrived with `4b65729b0`;
the fallback it collides with is `file_reopen_path` / `host_reopen_path`
(~65942/65930), whose `#[cfg(not(unix))]` arms return `None`, so
`Shell::resolve_special_redirect` yields `SpecialRedirect::Duplicated` and the
redirect becomes `<&3`. Under a dup, `b` reads `two` and leaves fd 3 at EOF, so
`c` reads nothing — precisely the observed `[one][two][]`.

**Whose:** lane B (`userspace/**`). Lane C does not edit it. Filed as
`requests/c-b-the-reopen-test-is-red-on-the-windows-dev-host.md`, which suggests
splitting the test so the dev host asserts the documented dup and the Unix build
asserts the re-open — that covers the fallback rather than merely excusing it.

**Proper fix:** lane B's call; either `#[cfg(unix)]` with a reason, or the split
above.
