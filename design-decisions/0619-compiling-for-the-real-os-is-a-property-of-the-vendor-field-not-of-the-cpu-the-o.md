## 619. "Compiling for the real OS" is a property of the vendor field, not of the CPU, the OS name, or a flag we pass

**Date:** 2026-08-26
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** Userspace programs sometimes need to talk to the SlateOS kernel
directly, and that code must not run when the same program is compiled to run on
the developer's own Windows or Linux machine — where the same instruction would
be answered by *that* machine's kernel, which understands the numbers to mean
something else entirely. So each such piece of code needs a switch that reads
"am I being built for the real OS?". The decision is which of four possible
switches to use. We picked the target's *vendor* field, and taught the compiler
that `slateos` is a legitimate value for it.

### The problem this fixes

Forty-six programs under `userspace/` guarded their raw `syscall` instructions
with `#[cfg(target_arch = "x86_64")]` — "am I being built for a 64-bit Intel
CPU?". The developer's machine *is* a 64-bit Intel CPU, so the guard let
everything through, and `cargo test` on the dev host issued the SlateOS calling
convention at the host kernel. The numbers do not merely fail to match; they
match *other calls*:

| SlateOS number | What it means to us | What it means to Linux |
|---|---|---|
| 14 | `clock_realtime` | `rt_sigprocmask` |
| 15 | `clock_settime` | **`rt_sigreturn`** |
| 16 | `clock_adjtime` | `ioctl` |
| 62 | (ours) | `kill` |
| 90 / 92 | (ours) | `chmod` / `chown` |
| 102 | (ours) | `getuid` |
| 140 / 141 | (ours) | `getpriority` / `setpriority` |

`rt_sigreturn` invoked outside a signal handler is undefined behaviour — the
kernel reloads the register state from a stack frame that was never a signal
frame. A test run could corrupt the developer's own process in a way with no
plausible connection to the test that did it.

### The four candidates

| Switch | True on SlateOS? | On the Windows host? | On a Linux host? | Verdict |
|---|---|---|---|---|
| `target_arch = "x86_64"` | yes | **yes** | **yes** | gates nothing — the bug |
| `target_os = "slateos"` | **no** | no | no | false everywhere — the inverse bug |
| `target_os = "linux"` | yes | no | **yes** | wrong on the Linux host |
| `target_vendor = "slateos"` | **yes** | no | no | **chosen** |

`target_os` is not available to us, and this is not an oversight we can correct:
`toolchain/x86_64-slateos.json` must declare `os = "linux"` because `build-std`
compiles a real `std` for the target, and `std` selects its platform layer by
`target_os`. Naming ourselves would make `std` fail to build. (`env` must stay
`"musl"` for the same reason.) So `target_os = "slateos"` is not a gate at all —
it is a predicate that is false on every target in existence, and anything behind
it has never been compiled. Two such regions were found: see below.

`target_vendor` has no such load-bearing role. It is metadata, we own it, and
flipping it to `slateos` cost nothing.

### Why not `--cfg slateos` in the target's rustflags?

This was the obvious alternative and it is the one that would have been wrong,
for a reason that only shows up in the *failure* case rather than the working
one. Cargo lets a `RUSTFLAGS` environment variable **replace** the rustflags from
`.cargo/config.toml` rather than adding to them. Under `--cfg`, any tool, script
or CI job that sets `RUSTFLAGS` for an unrelated purpose would silently switch
every gate off — compiling the ENOSYS *host stubs* into shipping OS binaries.
That failure is silent, produces a clean build, and would be discovered on real
hardware.

`target_vendor` is a **built-in** cfg carried by the target spec itself. No
environment variable can dislodge it.

There is a second half to this. rustc keeps a fixed list of well-known
`target_vendor` values (`amd, apple, espressif, fortanix, ibm, kmc, mti,
nintendo, nvidia, openwrt, pc, risc0, sony, sun, unikraft, unknown, uwp, vex,
win7, wrs`), and `slateos` is not on it, so every gate drew an `unexpected_cfgs`
warning until we extended the list:

```toml
[target.'cfg(all())']
rustflags = ['--check-cfg=cfg(target_vendor, values("slateos"))']
```

That *is* a rustflag, and a stray `RUSTFLAGS` would drop it — but the asymmetry
is the whole argument: dropping the `--check-cfg` brings the warnings back, and
dropping a `--cfg` ships broken binaries. We put the fragile thing where its
failure is loud.

### What the host arm should do, which is a decision in itself

A gate needs two arms, and the honest content of the second one differs by call:

- **Reading a clock** (`clock_realtime`, `clock_monotonic`): use `std`. The host
  can answer this question correctly, so it should, and the code above the gate
  never has to know which arm ran.
- **Changing system state** (`clock_settime`, `hwclock -w`, `clock_adjtime`):
  return an **error**. The caller here is `date --set`, whose entire purpose is
  the side effect; a stub that returns success is a lie about a change that did
  not happen.
- **Asking the kernel something it alone knows** (a non-blocking TCP receive on
  a SlateOS handle): return `-38` (ENOSYS). Ten pre-existing stubs returned `-1`
  (EPERM), which claims the operation was attempted and denied — for something
  never attempted at all.

### Where a libc symbol beats a gate entirely

Some of these calls should never have been raw `syscall` in the first place. When
`posix` exports the C symbol (`ioctl`, `sync`, `syncfs`), the right gate is
`cfg(unix)` and the right call is the symbol: it is true on SlateOS *and* on a
Linux dev host, where the operation genuinely means the same thing, so the host
does the real work instead of stubbing. `stty` and `htop` already did this;
`tput` and `sync` now do too.

### What flipping the gate uncovered

Turning `target_os = "slateos"` into a predicate that is actually true compiled
two regions for the first time ever, and both were broken — which is the
strongest available evidence that a never-true gate is worse than no gate:

- **`tput`'s terminal-size query** did not compile at all: the function was named
  `get_terminal_size_ctl` and both callers said `get_terminal_size_ioctl`. It
  also hand-rolled syscall **16**, which in our table is `SYS_CLOCK_ADJTIME` —
  so had it ever run, `tput cols` would have stepped the system clock, with a
  pointer to a stack array reinterpreted as a signed nanosecond delta.
- **`sync`** issued raw syscall **162**, which is Linux's `sync` number and is
  *unassigned* in ours (`SYS_FS_SYNC = 641`). It would have worked on a Linux
  dev host and returned ENOSYS on the real OS — the exact inversion of correct.

**Where it bites:** `.cargo/config.toml` (the `--check-cfg` line and its
comment), `toolchain/x86_64-slateos.json` (`"vendor": "slateos"`), and every
`#[cfg(target_vendor = "slateos")]` under `userspace/**`. known-issues.md →
`B-FORTY-SIX-USERSPACE-CRATES-CAN-ISSUE-A-RAW-SYSCALL-ON-THE-DEV-HOST`.
