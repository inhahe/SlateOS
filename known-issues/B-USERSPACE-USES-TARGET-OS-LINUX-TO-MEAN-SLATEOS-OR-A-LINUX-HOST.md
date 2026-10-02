## `B-USERSPACE-USES-TARGET-OS-LINUX-TO-MEAN-SLATEOS-OR-A-LINUX-HOST` (lane B, 2026-08-26) — **open**, low severity

**In short:** 41 places in `userspace/**` select a libc-using implementation with
`#[cfg(target_os = "linux")]`. That predicate is true on SlateOS — but only
because our target spec says `os = "linux"` for an unrelated reason. The code is
correct today; the concern is that it rests on a coincidence, and the day the
spec changes, 41 programs quietly switch to their do-nothing arms.

**This is much smaller than the raw-syscall bug, and the first draft of this
entry overstated it.** These sites are `extern "C"` declarations of ordinary libc
functions — `kill`, `nice`, `getpriority`, `setpriority`, `fcntl`, `isatty` — not
raw `syscall` instructions. Calling libc `kill()` on a Linux host does exactly
what the code means; there is no ABI mismatch and nothing undefined happens. The
predicate is honest about what it selects: "a platform with these C symbols".

**The one real defect.** It says `linux` where it means "has a libc". The *only*
thing making it true on SlateOS is `toolchain/x86_64-slateos.json` declaring
`os = "linux"`, and that declaration exists to let `build-std` compile a real
`std` — it is not a statement about libc at all. If that field ever changes, all
41 fall through to their `not(...)` arms, which is precisely the silent-stub
shape of the bug above. The predicate and the reason it holds are unrelated, and
that is the whole problem.

**Two things this is NOT, both of which the first draft got wrong:**

- **It is not a Windows-blind-spot instance.** `cfg(unix)` is *also* false on
  Windows, so converting would not make the Windows host compile these arms. The
  detection story is `B-DEV-HOST-IS-WINDOWS-SO-CFG-UNIX-CODE-IS-NEVER-COMPILED`
  and the linux-target check, which already covers them.
- **`cfg(unix)` is not a blanket answer.** For `coreutils/src/stdfd.rs` (4 of the
  41) it would be actively wrong: that gate gets a constructor into
  `.init_array`, which is an **ELF** mechanism. macOS is `unix` and uses
  `__mod_init_func` instead, so `cfg(unix)` would place the constructor in a
  section that platform does not read. `target_os = "linux"` is arguably the
  *more* correct predicate there, and if anything it wants
  `cfg(target_family = "unix", not(target_vendor = "apple"))` or an explicit
  ELF-ish gate.

**Proper fix — per site, not mechanical.** Sort the 41 into: (a) plain libc
symbol use, where `cfg(unix)` is both accurate and decoupled from the spec's `os`
field; (b) genuinely ELF- or Linux-specific mechanisms like `stdfd.rs`, where the
current gate stays and gains a comment saying *why* it is not `cfg(unix)`. Kept
out of `045f603e1` so a 54-file semantic commit did not also carry unrelated
predicate edits — and, as it turns out, so that the sorting could be done
properly rather than by `sed`.

**Where it bites:** `userspace/**`, 41 real attributes (an earlier count of 44
included three mentions in comments). Find them with:

```bash
grep -rn 'target_os = "linux"' --include=*.rs userspace/ | grep -E '#!?\[cfg'
```

Distribution: `oils/src/interp.rs` 11, `coreutils/src/bin/nohup.rs` 8,
`renice.rs` 6, `stdfd.rs` 4, `nice.rs` 4, `df.rs` 4, `kill.rs` 3,
`hostname.rs` 1.
