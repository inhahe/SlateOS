## `B-FORTY-SIX-USERSPACE-CRATES-CAN-ISSUE-A-RAW-SYSCALL-ON-THE-DEV-HOST` (lane B, 2026-08-26)

**Status:** ✅ FIXED 2026-08-26 (`045f603e1`). Every site in `userspace/**` is now
behind `#[cfg(target_vendor = "slateos")]` with an honest host arm. Verified with
zero warnings on all three targets — the SlateOS target, `x86_64-pc-windows-gnu`,
and `x86_64-unknown-linux-gnu`. The audit that produced the numbers below now
reports 114 sites, 84 vendor-gated, 9 `target_os = "none"`-gated, and **0 ungated
in `userspace/**`, `posix/**` or `init/**`**; the 21 that remain are all in
`services/**`, which is outside the workspace and only ever builds for
`x86_64-unknown-none`, so no host compiler ever sees them. Rationale for the
choice of predicate: design-decisions.md §619.

**Correction to the "proper fix" proposed below: `target_os = "slateos"` does not
work and never did.** It is false on *every* target including the real OS, because
`toolchain/x86_64-slateos.json` must declare `os = "linux"` — `build-std` compiles
a real `std`, which picks its platform layer by `target_os`, so naming ourselves
breaks the `std` build. Anything already sitting behind that predicate has
therefore never been compiled at all. Flipping those gates to a predicate that is
actually true found two such regions, and **both were broken**:

| Where | What was wrong |
|---|---|
| `userspace/tput` terminal size | Did not compile: `get_terminal_size_ctl` defined, `get_terminal_size_ioctl` called. It also hand-rolled syscall **16**, which is our `SYS_CLOCK_ADJTIME` — so `tput cols` would have stepped the system clock, passing a pointer to a stack array as a signed nanosecond delta. |
| `sync` (in `userspace/getopt`) | Issued raw syscall **162**: Linux's `sync` number, *unassigned* in our table (ours is `SYS_FS_SYNC = 641`). Correct on a Linux dev host, ENOSYS on the real OS. `sync -f` was also parsed and then discarded. |

Both now route through the posix libc symbols (`ioctl`, `sync`, `syncfs`) under
`cfg(unix)`, which is the right gate when `posix` exports the symbol and the host
means the same thing by it — the host then does the real work instead of stubbing.
`stty` and `htop` already did this.

This is the entry's most useful lesson, and it generalises: **a gate that is false
everywhere is worse than no gate**, because it silently exempts the code from the
compiler as well as from the host. Grep for `target_os = "slateos"` before trusting
any guard that mentions it.

**In short:** The bug fixed directly above was one instance of a pattern, and
an audit of lane B's tree found 101 more places where a raw `SYSCALL`
instruction is not prevented from running on the developer's own machine.
Unlike the `pipe2` case, none of these appears to be reachable from a test
today, so nothing is currently executing them — but the guard is missing, so
the day someone writes a test that calls one of these functions, it silently
starts issuing real Linux or Windows system calls with SlateOS numbers.

**The audit.** 115 raw `syscall` instruction sites in `posix/**`,
`userspace/**`, `services/**`, `init/**`; 14 correctly gated on
`target_os = "none"`; **101 not**, across **46 crates**:

> `arp`, `at`, `chown`, `curl`, `date`, `df`, `dhcpcd`, `dig`, `diskutil`,
> `fsck`, `ftp`, `ftpd`, `fw`, `htop`, `hwclock`, `ifconfig`, `inetd`, `ip`,
> `kill`, `libservicebus`, `mkfs`, `mount`, `nc`, `netstat`, `nmap`, `ntpd`,
> `pgrep`, `ping`, `powerctl`, `route`, `rsync`, `scp`, `screen`, `service`,
> `services`, `sftp`, `ss`, `ssh`, `sshd`, `strace`, `tcpdump`, `telnet`,
> `timeout`, `traceroute`, `wget`, `whois`

**The specific trap, and it is a good one.** Most of these *look* gated —
they carry `#[cfg(target_arch = "x86_64")]`. That gate is about the
instruction set, and it is **true on both dev hosts**, so it prevents nothing
that matters here. The gate that works is one that is false on every host
build: `target_os = "none"` (our bare-metal target) or `target_os = "slateos"`.
A handful (`at`, `date`, `hwclock`) have no cfg at all. `date`'s
`clock_settime` is the one to look at first: it passes an attacker-irrelevant
but arbitrary scalar in RDI to whatever the host's `SYS_CLOCK_SETTIME` number
means.

**Current reachability: latent, not live.** For all 101 sites, the enclosing
function is not called from that file's own `#[cfg(test)]` module — which is
why `cargo test --workspace` is green on Windows rather than doing something
alarming. This is a static check and therefore weaker than the `pipe2` result,
which was measured; treat it as "no evidence of a live path" rather than
"proven unreachable".

**Proper fix.** Give each crate the same treatment `posix` already has and
`pipe2` has now: raw asm behind `#[cfg(target_os = "none")]`, and a host arm
that returns a documented sentinel (or a small simulator where tests need the
call to succeed). The mechanical part is uniform; the judgement per crate is
only "does anything need this to *work* on host, or merely to fail cleanly?"

**Confined to lane B — checked, not assumed.** The first draft of this entry
said the other lanes were probably affected too and should be told. They are
not, so no request was filed. Measured against `origin/main`:

| tree | raw `syscall` sites | verdict |
|---|---|---|
| `kernel/**`, `bench/**` (lane A) | 0 | the kernel *receives* syscalls; it issues none |
| `gui/**`, `apps/**`, `net*/**`, `pkg/**` (lane C) | 2 | both fine — see below |

Lane C's two are in `gui/compositor/src/present/{drm,evdev}/sys.rs`, and they
are deliberately *Linux* syscalls for DRM and evdev, gated
`#[cfg(target_os = "linux")]` and documented as using "the x86-64 Linux
syscall ABI". That is a raw syscall that is correct precisely because it runs
on the host it names. It is the opposite of this bug, not an instance of it.

**Still worth noting as a pattern, though.** This is the third `#[cfg]`
predicate in two days that was wrong in a way visible on only one host — see
`B-BACKUP-CFG-UNIX` and `BUG-OILS-REOPEN-TEST-IS-UNIX-ONLY` above. The
recurring shape is a gate that names the *nearly* right axis:
`target_arch` for `target_os`, `cfg(unix)` for "has procfs", `cfg(unix)` on a
dev host that is Windows.
