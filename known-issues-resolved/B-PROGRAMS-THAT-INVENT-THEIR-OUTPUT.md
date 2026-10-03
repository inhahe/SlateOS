## B-PROGRAMS-THAT-INVENT-THEIR-OUTPUT (lane B, 2026-09-12) -- 15 found, 14 fixed, 1 filed

### `sbctl`, found 2026-09-13 -- the whole write half of a security tool

`fs::write` appears **zero times in `userspace/sbctl`**. The crate reports
creating secure-boot keys and signing EFI binaries and does neither.

    $ sbctl create-keys
    Creating secure boot keys...
      Created: /etc/secureboot/keys/PK/PK.key       <- not written
      ... five more                                 <- not written
    Keys created successfully.

    $ sbctl sign /boot/vmlinuz
    Signing '/boot/vmlinuz' -> '/boot/vmlinuz'       <- file untouched
      Using key: /etc/secureboot/keys/db/db.key

The three key DIRECTORIES are created, empty. That detail matters: `status`
decides `PK: Enrolled` by asking whether the directory exists and is
non-empty, so `create-keys` leaves the tool in a state where it contradicts
its own success message on the very next command.

**Worse than the other inventions because of what the output is for.** A user
who runs `sbctl sign` on a kernel image and reads that line has been told the
image is signed. Nothing downstream can correct them: the file is unchanged,
so the failure surfaces later, elsewhere, as a firmware refusal to boot.

**The capability exists and userspace cannot reach it.** `roadmap.md:2783`
describes `fs::secureboot` in the kernel -- four boot states, five key types,
enrolment/removal, image verification, `/proc/secureboot`, eight self-tests --
and `grep -rn "secureboot|secure_boot" posix/src/` returns nothing. No
syscall, no wrapper, no constant. sbctl was written as though the feature did
not exist.

Key GENERATION is a separate gap and not lane A's: a PK/KEK/db keypair needs
RSA and X.509, which this tree does not have.

`roadmap.md:3835` said `[x] sbctl/sbsign/sbverify/sbkeysync: secure boot
management (key enrollment, EFI signing, rotation, 645 lines)`. Corrected to
`[~]` with the read/write split stated -- fabricated done-status about a
security feature is the worst place for it. Filed as
`requests/b-a-sbctl-needs-a-userspace-door-to-fs-secureboot.md`.

`status` is untouched and was always real: it reads the efivars `SecureBoot`
and `SetupMode` variables.

**Resolved 2026-09-27** (design-decisions §1049, the operator's answer to
B-Q17). The four commands that need RSA, X.509 or Authenticode are deleted,
and so is everything that reported work it never did: the files database
(`verify FILE` printed "signature valid" for any file; `remove-file`
removed nothing), and the `sbsign`, `sbverify` and `sbkeysync`
personalities. `enroll-keys` and `reset` refuse until lane A's door lands.
One correction to the line above: `status` was *not* all real -- it called a
key "Enrolled" when a key directory was non-empty, printed a hard-coded
"Owner: Slate OS", and reported "Setup Mode: Enabled" where no EFI
variables existed at all. It now reads what the firmware and the kernel
publish, and says "unknown" where it cannot.

### Three more found 2026-09-13, all in `userspace/wipefs`, all destructive

Found by accident: a probe for a different class (an unknown option that does
not stop) printed a signature table for a file that could not have one. The
class detector never ran on this crate, which is worth noting -- the sweep that
produced the 11 below probes BEHAVIOUR, and these three only show up if you
look at what the program did to the device rather than at what it printed.

| program | what it invented | fate |
|---|---|---|
| `wipefs` | an ext4 signature at 0x438 and a DOS partition table at 0x1fe, for any device with no real signatures | `generate_default_sigs` deleted (`43d4bf7f9`) |
| `wipefs -a` | "ext4 wiped at offset 0x438" without ever opening the device | writes zeros over the magic and `sync_all`s (`ed1efd233`) |
| `blkdiscard` | "discard entire device from X at offset 0", never touching the device | `--zeroout` implemented; discard/secure refuse, naming the missing ioctl (`9f7a85397`) |

The first was measured on a **sixteen-byte file**, reported as carrying both
signatures -- one of them at an offset past the end of the file. The fabricated
ext4 row was even distinguishable from a real one: the genuine signature table
calls it `ext2/ext3/ext4` and the invented row said `ext4`.

`test_generate_default_sigs` asserted the fabrication was exactly two rows
named ext4 and dos, so the suite would have caught anyone REMOVING it. That is
the second time in this lane a test has pinned an invention in place (the
first was systemctl's thirteen).

Two further defects in the same crate, same reading but not fabrication:

* an unreadable device returned the same empty result as a clean one, so
  `wipefs /dev/does-not-exist` printed a header and exited 0;
* `blkdiscard -o notanumber` silently became offset 0 -- the START of the
  device -- and `-l notanumber` silently became ENTIRE DEVICE. Both typos
  failed in the direction of destroying more.

### The original eleven



A class, not a bug. A program prints something shaped like a measurement,
an action or an event, and the value did not come from the system. It is
distinct from an unimplemented feature, which announces itself; this looks
exactly like the working version.

### Found, in ascending order of what it costs

| program | what it invented | fate |
|---|---|---|
| `firejail` | `Child process initialized in 0.8ms` -- a literal, before refusing to run anything | line removed |
| `blkzone` | two hardcoded disk zones, for any device on any machine | deleted (§1006; needs ioctls this build lacks) |
| `systemd-cgls` | a fixed cgroup tree: `init.scope` pid 1, `dbus.service` pid 100 | walks `/sys/fs/cgroup` |
| `systemd-cgtop` | five cgroups with invented task counts, CPU and memory | reads `pids.current`/`memory.current` |
| `fio` | `usr`/`sys`/`ctx`/`minf` computed from the operation count | reads `/proc/self/stat` |
| `gdb` | `print $rxa` -- any unknown register -- evaluated to `0` | returns `No such register` |
| `prlimit` | "setting NOFILE for PID N" with no syscall at all | calls `prlimit64` |
| `mkinitramfs` | `compression: Gzip`, then wrote the cpio uncompressed | calls `deflate::gzip` |
| `systemctl` | six commands: `enable` named a symlink path, `poweroff` announced a shutdown, `list-timers` invented a timer | all refuse via `no_unit_interface` |
| `inotifywait` | a `newfile.txt` creation on every run | reads the kernel's event stream |
| `chattr` | `+i` stored in a `<file>.attrs` sidecar; the file stayed writable | filed to A for `FS_IOC_SETFLAGS` |

**The severity ordering is the useful part.** `blkzone`, `cgtop` and
`gdb` invented *readings*, which mislead a reader -- and `gdb` is the
sharpest case, because a debugger's entire product is readings, so there
is nothing else in its output to cross-check one against. `prlimit`,
`chattr` and `mkinitramfs` invented *actions*, which mislead a program --
a caller lowers a limit, sets the immutable bit, or compresses an image,
and is told it worked. `inotifywait` invented an *event*, which makes a
program **act**: `while inotifywait -e create dir; do rebuild; done`
never stops.

`mkinitramfs` extends the ordering past where I first drew it. An
invented action misleads whoever called it; this one also **wrote a
file**, so the lie outlived the process and was still there for the next
program to trust. A refusal that leaves a plausible artifact behind is
the same bug in a quieter form, which is why it now writes nothing.

`systemctl` is one program but six commands, and it spans the whole
ordering by itself: `list-timers` invented a *reading* (`logwatch.timer`,
next `Mon 2026-01-02 00:00:00`, `23h left`, on a system with no timer
units), `enable` invented an *action*, and `poweroff` invented an
*event*. `enable` is the most expensive single instance found so far, for
a reason `mkinitramfs` only half has: it printed an **exact path** --
`Created symlink /etc/slateos/system/multi-user.target.wants/sshd.service
-> /usr/lib/slateos/system/sshd.service.` -- so the administrator is left
with a specific file to believe in. No file was written, so there is not
even a wrong artifact to find; there is a confident sentence and an empty
directory, and the belief that the service starts at boot survives until
the next boot disproves it.

It also shows the class can be **half-fixed and look finished**. The
query half of `systemctl` was already honest -- `is-active` refuses and
carries a comment recording that it used to exit 0 for four hard-coded
names -- while every command that *acts* still claimed. Whoever repaired
the read side did not think to check the write side, and the file reads
as tended.

The count in this heading read `7 found, 6 fixed` while the table below
it listed eight, from the last time it was extended without being
re-totalled -- the same present-tense drift this file keeps recording in
other people's documents.

### How they were found, since the grep is the weakest part

A grep for `// stub|fake|placeholder|simulated|hardcoded|dummy` gives 45
hits in lane B and most are honest host-test shims. It is a reading list,
not a finding list. What actually identified them:

- **Reading a file for an unrelated reason.** `getfacl`'s wrong group
  lookup, `prlimit`'s missing syscall and `sanitize`'s UTF-8 panic were all
  found while fixing that file's *option parsing*.
- **A variable whose only consumer is the output.** After fixing `fio`,
  clippy reported `ops_done` as assigned and never read: the operation
  counter existed solely to manufacture four numbers.
- **A test that asserts a constant.** Five tests in this tree pinned
  fabrications in place -- `systemd-cgls`, `systemd-cgtop` and three in
  `inotify`. A test asserting `system.slice` appears in cgroup output is
  either testing the kernel or testing a literal, and it was the literal.
- **A success banner before the work.** Twice: `inotifywait` printed
  "Watches established (1 total)" and `firejail` printed "Child process
  initialized", both immediately before refusing.

### Unused model code is the best detector found so far

Not in the original list of heuristics because it was not known then.
Removing a crate's blanket `#![allow(dead_code)]` and reading what rustc
then reports has found, in four crates:

| crate | what the dead model meant |
|---|---|
| `gdb` | a duplicate tokeniser nobody called, and an unenforced breakpoint limit |
| `wpa` | the personalities that used it were deleted for fabricating |
| `logind` | the entire write side is implemented and exposed to nothing |
| `systemctl` | six commands that print and do nothing, so the unit model is never populated |

The reasoning is mechanical, which is why it works: **a model nothing
constructs means nothing populates it, and something is usually
pretending it did.** It is a far better filter than grepping for the word
"stub" -- that produced a reading list of 45 comments of which two were
real, while this produced a finding in four crates out of four.

The cost is that it only fires where a blanket allow was hiding the
evidence, so it runs out when the 19 remaining crates are cleaned.

### Negative results, so nobody re-checks them

`capsh` and `chroot` are flagged by the same grep and are both honest.
`capsh` refuses with "refusing to exec ... with unchanged capabilities" --
its "Simulated process state" is a section header over state accumulated
before exec. `chroot` routes every privilege-changing operation through an
`enosys()` helper returning "not implemented in this kernel". `firejail`'s
refusal path was right too; only its banner was wrong.

### Still open

`chattr`'s `read_attrs` still consults the sidecar and invents an
`EXT4_EXTENTS_FL` default when there is none. Left deliberately: whether
`lsattr` survives depends on A's answer to
`requests/b-a-chattr-needs-fs-ioc-getflags-or-it-should-be-deleted.md`,
and there is no honest reading to give it meanwhile.

The 45 stub comments are now triaged, and the impression I recorded here
first -- "mostly in crates whose whole purpose is host-side development
support" -- was wrong in the way that mattered. Most are honest, but not
because of where they live:

| Comment says | Actually | Verdict |
|---|---|---|
| `scp` "Stub: send/receive a file" | returns `Err(RemoteNotSupported)` | honest |
| `last` "we would do reverse DNS" | prints the real IP instead | honest, reduced |
| `crond` `weekday: 0, // placeholder` | a test input the function ignores | honest |
| `dbus` "placeholder for length" | write-zero-then-backfill | not a stub at all |
| `powerctl`, `swapon` "stubbed to `-ENOSYS`" | say so and return it | honest |
| `mkinitramfs` "would call compression library" | **returned its input unchanged** | fabricating |
| `gdb` "resolved by the caller" | **no caller; wrong by design** | dead |

Two things this cost me that the grep could not have told me. The
`mkinitramfs` one was the single most expensive of the eight fabrication
instances, and it sat in the list the whole time: the comment named the
missing dependency, and the dependency was a workspace crate four
directories up. A stub comment that names what it is waiting for is
worth checking against the tree before believing it.

The `gdb` one is why this class stayed invisible. Its 105 dead lines are
covered by

    #![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing, dead_code)]

whose eight-line comment justifies the first two lints and never
mentions the third. That shape -- a defensible suppression with an indefensible one
appended to it -- is in **22 lane-B crates**. Measured on
`x86_64-unknown-linux-gnu`, the closest installed target to the
unix-like one SlateOS userspace actually builds for, it is hiding **180
findings**, of which **175 are present on every target** (the table
below sums to the 180):

| Crate | Findings | | Crate | Findings |
|---|---|---|---|---|
| `gdb` | 27 | | `upower`, `tcpdump` | 5 each |
| `wpa` | 25 | | `ntpd`, `ar`, `login` | 4 each |
| `logind` | 22 | | `getty` | 3 |
| `systemctl` | 19 | | `resolvectl`, `findmnt`, `ss`, `ldconfig` | 2 each |
| `objdump` | 13 | | `irqbalance`, `acpi`, `blkid` | 1 each |
| `jq` | 12 | | `posix`, `libservicebus` | 8, 1 |
| `finger` | 10 | | `dhcpcd` | 7 |

**A dead-code census is target-specific, and naming the target is part
of the number.** I first published 208 here, measured on
`x86_64-pc-windows-gnu` -- which is not unix, so every `#[cfg(unix)]`
path was compiled out and counted as dead. `logind`'s real `serve` is
unix-gated, so on that host its entire bus layer -- `handle_message` ->
`dispatch` -> `authorize`, and the `ERR_*`/`OUTCOME_*` constants -- had
no caller. I had it written up as a session daemon whose authorization
function nothing calls. It is live; I caught it only because
`handle_message` visibly calls `dispatch`, which contradicted the
compiler and was worth stopping for.

| Target | Findings |
|---|---|
| `x86_64-pc-windows-gnu` | 208 |
| `x86_64-unknown-linux-gnu` | 180 |
| **real on both** | **175** |
| windows-only (cfg artifact) | 33, of which 28 are `logind` |
| linux-only (missed at first) | 5 |

Every crate's count is identical across the two targets except `logind`,
50 -> 22. Only four of the 22 crates contain `cfg(unix)` at all --
`oils` 31 occurrences, `getty` 6, `login` 5, `logind` 2 -- which is what
bounds the artifact.

Method, so it can be repeated: `RUSTFLAGS="--force-warn dead_code" cargo
check -p <each> --target <triple> --message-format=json`, deduplicated
by (file, line, message). Two details matter. `--force-warn` overrides a
crate-level `#![allow]` where `-W` does not, so nothing weaker can see
past these lines at all. And `check` rather than `build` is what makes
the second target possible: checking does not link, so a linux target
can be measured from Windows without a cross-linker.

Not all 175 are bugs. 48 are constants, and a complete ELF or DBus
constant table with some entries unread is legitimate. The defect is
that the allow is **crate-wide**, so a real finding like gdb's cannot be
told from a deliberate table -- and the annotation that would say which
is which was never required, because the lint never fired. The fix is
per-item `#[allow(dead_code)]` with a reason, and `dead_code` struck
from the 22 crate-level lists; then the next `tokenize_expr` announces
itself. That is 22 crates of work and is not started.

`logind`'s surviving 22 are not a constant table and deserve their own
look: `SessionType` (`Tty`, `X11`, `Wayland`, `Unspecified`),
`SessionClass` (`User`, `Greeter`, `LockScreen`) and `SessionState`
(`Opening`, `Online`) are never **constructed**, their `from_str` never
called, `CreateSessionParams` never built, and the inhibitor fields
`who`, `why`, `uid` and `pid` never read. It models sessions in detail
and never populates the model.
