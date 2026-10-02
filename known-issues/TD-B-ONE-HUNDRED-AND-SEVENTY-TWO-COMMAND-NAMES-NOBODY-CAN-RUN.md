## TD-B-ONE-HUNDRED-AND-SEVENTY-TWO-COMMAND-NAMES-NOBODY-CAN-RUN (lane B, 2026-09-11)

**In short:** 172 command names are implemented in `userspace/` as multicall
personalities — `gunzip`, `zcat`, `factor`, `printenv`, `setcap`, eleven
SELinux tools — and nothing installs an executable under any of those names.
The code is finished, tested, and unreachable. A further 11 names are worse:
they are answered to by two different programs at once.

**Why the number jumped from 51 on 2026-09-11.** It did not. The detector was
blind. `scripts/multicall-aliases.py` recognised a dispatch arm only when it
mapped to an enum literally named `Personality`, so the 8 crates that call
theirs `Mode` and the one that calls it `InvokedAs` were invisible. Widening it
to key on the *scrutinee* — is the matched variable the program's own
invocation name? — took the ledger from 51 to 172 and the shadowing ledger from
2 to 11. No source changed in that commit, so all 130 were reachable before it.

**UPDATE 2026-09-11: the shadowing ledger is empty.** All 11 entries are
cleared — the 9 below plus `chown:chmod` and the two `cron` ones. Six were
deleted as the weaker copy, two after PORTING what they had that the reachable
copy lacked (`free` gained `-l/--lohi`, `--tebi` and the GNU long forms;
`crontab` gained `-i`), and `userspace/cron` went entirely: 2,508 lines whose
every entry point was shadowed, unreachable, or an outright refusal. The
unreachable ledger stands at 169.

**UPDATE 2026-09-25: the unreachable ledger stands at 155.** Fourteen names have
left it since the 169 above. Two, `blockdev:blkzone` and `cal:ncal`, went on
2026-09-12 (commit 888598b8f: `blkzone` printed hard-coded zones for any
device and reported zone resets it never attempted). Twelve were closed the
§1005 way -- the name becomes a `coreutils` bin, ported from GNU
9.4 and checked against a build of it by a `scripts/<name>-diff.sh` harness,
and the dead branch is deleted -- rather than by adding a link to the
personality: `printenv`, `sync` and `cksum` (from `getopt`, which with all
three gone is `getopt` alone), `truncate` and `shred` (from `pv`, which with both
gone is `pv` alone),
`arch`, `pathchk` and `users` (from `nproc`), `numfmt` and `factor` (from
`shuf`, which with both gone is `shuf` alone), `base32` (from `base64`,
whose own `base64` followed on 2026-09-27 -- TD-B-BASE64-IS-STILL-THE-OLD-CRATE-UNTIL-UUENCODE-MOVES),
and `pinky` (from `finger`, which with it gone is `finger` alone).
None of the deleted branches was worth keeping: `nproc`'s `users` read the
terminal field as the user name, from wtmp instead of utmp; its `pathchk -p`
checked against 4096 and 255 instead of POSIX's 256 and 14; its `arch`
guessed from `/proc/cpuinfo`; `shuf`'s `numfmt` rounded every scaled value to
a whole number (`--to=si 1500` said `2K` where GNU says `1.5K`) and failed 135
of the 148 cases `scripts/numfmt-diff.sh` runs; its `factor` worked in `u64` by
trial division alone, so a large prime took minutes and anything past 2^64
was refused; `getopt`'s `cksum` had the CRC and nothing else of 9.4's -- no
`-a`, no `--check` -- and read each file whole into memory first; `pv`'s `shred`
wrote xorshift output three times under a help text promising `/dev/urandom`,
where GNU's schedules its passes from a table of bit patterns and, given
`--random-source`, writes bytes this port now reproduces exactly; `finger`'s
`pinky` printed finger's own layout under pinky's options, so `-b` hid the
plan where GNU's hides the home directory and shell, and `-w` and `-i` were
accepted and did nothing. With its last three personalities gone, **`userspace/nproc`
itself was retired**: `nproc` is a `coreutils` bin too now, a port of GNU's
(gnulib's `num_processors` -- affinity mask, `OMP_NUM_THREADS`,
`OMP_THREAD_LIMIT`), where the crate had counted `/sys` ranges and told a
process pinned to two CPUs that it had twelve. No GNU program lives as a
personality of something else any more. None of the new bins is on the
image yet: that is lane D's
manifest, `requests/b-d-new-coreutils-programs-for-the-rootfs-manifest.md`.

**UPDATE 2026-10-01: the §1045 triage has begun, and the ledger stands at
118.** The operator's rule (B-Q11, design-decisions §1045): each name is
decided on its own; a name for a subsystem SlateOS does not have is deleted
(§1006), and with it the crate when its own program is the same kind; a name
that is kept is installed as the same file. First settled, as whole crates:
**`userspace/apparmor`** (`aa-status` and seven more) and
**`userspace/selinux`** (`getenforce` and ten more), deleted -- SlateOS has no
Linux security module and none is planned (its security is capabilities), so
every command acted on `/sys/kernel/security/apparmor` or `/sys/fs/selinux`,
neither of which exists, and `apparmor_parser`'s load was a comment where the
write would be. Then **`userspace/grub2`** (`grub-install`, `grub-mkconfig`,
`grub-probe`, `grub-set-default`, `grub-reboot`, `grub-editenv`,
`update-grub`), judged command by command because design.txt (lines
1237-1243) plans for SlateOS to live beside a Linux GRUB on dual-boot
machines -- and deleted, because that plan is already met elsewhere: SlateOS
boots with Limine, and adding it to another OS's GRUB is the installer's
(`apps/installer`, `--grub-detect/--grub-add/--grub-update/--grub-remove`,
which knows where that GRUB's files are). None of the seven served it.
`grub-install` wrote no bootloader image (it said so, and exited 0);
`grub-mkconfig` and `update-grub` built a menu from Linux kernels in SlateOS's
own `/boot`, where there are none; `grub-set-default` and `grub-reboot` wrote
`/boot/grub/grubenv` on SlateOS's disk, which no bootloader reads, and said
the default had been set; `grub-probe` names devices the way only a GRUB
config needs; `grub-editenv`, the one that did real work, edited a file
GRUB's way but printed what GRUB's does not. If "boot the other system once"
is ever wanted, it belongs in the installer's GRUB code, by setting
`next_entry` in the grubenv it has found, with GRUB's own `grub-editenv`
ported if a command is wanted too. The ledger was 118 after the three crates,
and is **115**: `ranlib`, `strip` and `killall` were never unreachable --
lane D's manifest installs them (`ranlib = ar`) -- but the checker did not
read the manifest until 2026-10-01. **Kept, first batch** -- installed as the
same file, asked of lane D with the lines to add
(`requests/b-d-stage-every-program-that-builds.md`): `gunzip`, `zcat`,
`gzcat` (gzip); `clear`, `reset`, `tset` (tput); `groupadd`, `groupdel`,
`groupmod`, `userdel`, `usermod` (useradd); `w` (who); `unzip` (zip). Each is
implemented and tested in its crate and needs no permission its program
lacks; they leave the ledger when the lines land.

Deleted next, as whole crates, each for a subsystem SlateOS has not got and
does not plan: **`userspace/audit`** (`auditctl`, `auditd`, `ausearch`,
`aureport`, `autrace` -- the Linux audit framework's netlink rules and
`/var/log/audit`; SlateOS's auditing is the kernel's own capability and
filesystem audit rings, which none of them read; `auditctl` kept rules that
nothing enforced, `auditd` could not start), **`userspace/firejail`**
(`firecfg`, `firemon` -- a namespace-and-seccomp sandbox; it parsed profiles
and refused to run anything, and SlateOS confines programs by capabilities,
with no sandbox tool of this kind on the roadmap), **`userspace/mkinitramfs`**
(`lsinitramfs`, `update-initramfs` -- SlateOS boots no initramfs: Limine loads
the kernel, which carries its services) and **`userspace/plymouth`**
(`plymouthd` -- no splash daemon exists or is planned, and the client printed
what it "would" show). **The ledger stood at 106.**

Reading each remaining name's code turned up two that *fabricate*, which
§1006 deletes whatever the name: **`userspace/xattr`** (`getfattr`,
`setfattr`, `attr`) invented its answers -- `getfattr` listed a
`user.mime_type` guessed from the file's extension and an SELinux label for
anything under `/bin/`, `setfattr` printed "setting ... on ..." and wrote
nothing, and `attr` was mapped onto `getfattr`'s parser although it is a
different command -- so the crate is deleted (a port of the `attr`
package's tools replaces it when they are wanted); and `chattr`'s **`lsattr`**
reported the extents flag for every file whose flags it could not read.
`lsattr` is kept -- it waits on the FS_IOC_GETFLAGS lane A scheduled -- and
now refuses instead (b3367ac33). **The ledger stood at 104**, and stands at
**102** after `userspace/locale` (`getconf`, `localedef`), deleted for the same
reason: all three of its programs fabricated, as
`B-NO-GETCONF-OR-LOCALE-UNTIL-PORTED` records with the ports that replace them
(`getconf` was ported the same day). Then **`userspace/mesg`** (`write`, `talk`),
whose three programs agreed with one another through files instead of
terminals (`B-MESG-AND-WRITE-UNTIL-PORTED`). **The ledger stood at 100.**

Four daemons followed for inventing their answers, which §1006 settles
before any naming question: **`userspace/udisks`** (`udisksd`) listed a
made-up `/dev/sda`, ext4, UUID `12345678-abcd-...`, whenever it found no
block device; **`userspace/upower`** (`upowerd`) reported "daemon
initialized (simulated mode)"; **`userspace/fwupd`** (`fwupdtool`) offered
firmware 1.1.0, "Bug fixes and security updates", for a device it never
queried; **`userspace/tuned`** (`tuned-adm`, `tuned-gui`) answered every
`verify` setting `OK (simulated)`. Then **`userspace/numactl`** (`numastat`,
`numademo`, `memhog`): `numactl -m 0 CMD` printed the policy it would set and
never ran CMD, `memhog` reported "Allocation complete." having allocated
nothing, `numademo` printed invented bandwidths. **The ledger stands at 92.**

**Kept, second batch** (sent to lane D the same way): `xxd`, `atd`, `atq`,
`atrm`, `batch`, `anacron`, `lastb`, `lastlog`, `sg`, `mingetty` -- working
programs -- and `lsattr`, the seven cgroup tools and the five `lp` names,
which refuse honestly while what they wait for (FS_IOC_GETFLAGS, the
kernel's cgroupfs at `/sys/fs/cgroup`, its print queue) is scheduled or
built. Six of `systemctl`'s fourteen were never names at all -- `blame`,
`critical-chain`, `dot`, `plot`, `security` and `verify` are
`systemd-analyze`'s subcommands, which the detector mistook for program
names (`dot` is graphviz's); they are excluded in its IGNORE table and the
ledger stands at **86**. **`userspace/capsh`** (`getcap`, `setcap`,
`getpcaps`, `captest`) is deleted for fabricating: `setcap` kept "file
capabilities" in a sidecar directory that exec never consults, and
`capsh`/`captest` answered from a simulated process state -- SlateOS's
capabilities are kernel object handles, not Linux's bit sets, so there is
nothing for these to set. `sudo`'s three were read too: all work (the
command really runs as the target user; a comment saying it was simulated
was false and is fixed); `sudoedit` stays the same file, while `visudo`
and `sudoreplay` need less than `sudo` holds and so, per §1045, earn crates
of their own -- a split still to do. The ledger stood at **82**.
**`userspace/perf`** (`perf-stat`, `perf-record`, `perf-report`, `perf-top`) is
deleted for fabricating: its counters came from "simulated"
`/proc/<pid>/perf_events` files, and without them `perf stat` printed
`0 cycles` as a measurement (its tests asserted the zero). **78.**

**Third pass** (2026-10-01): names that only repeated a subcommand of their
own program, under a name no upstream ships, went -- `coredump-extract`
(`coredumpctl dump`), `lodetach` (`losetup -d`), `rfkill-event` (`rfkill
event`), `fio-verify` (fio verifies through `verify=`). Names for Linux
interfaces SlateOS will not have went too -- `cifsiostat` and `tapestat`
(`sysstat`; `/proc/fs/cifs` and `/proc/scsi/tape`). Two printed another
program's answer under a real tool's name and went: `turbostat`
(`cpupower`, labelling the current and maximum frequency as turbostat's
MSR averages) and `biosdecode` (`dmidecode`, printing DMI type 0 for a
tool that decodes the BIOS's entry points). `fio`'s trim workload, which
wrote zeros and counted them as trims, now refuses. And
**`userspace/resolvectl`** (`host`, `resolvconf`, `systemd-resolve`) is
deleted for fabricating: `resolvectl service` printed an invented SRV
record, and `host` answered NXDOMAIN to every reverse lookup but
localhost without sending a query, and ignored `-t`. **Kept, third
batch:** `mpstat`, `pidstat` (sysstat, reading `/proc`), `sockstat` (ss,
in BSD's format), `ntpdate`, `sntp` (ntpd, which really queries and sets
the clock). **The ledger stands at 67.**
`systemctl`'s eight were read next: `systemd-analyze` (a fixed "Startup
finished in 1.200s (kernel) + 2.500s" and a fixed `blame` list),
`systemd-notify` ("Sending: READY=1" to nobody; `--booted` said yes) and
`systemd-tmpfiles` (built-in entries instead of its configuration) made
their answers up and went; `systemd-cat` (the journal), `systemd-escape`,
`systemd-path`, `systemd-cgls` and `systemd-cgtop` (honest about a missing
cgroupfs) stay. **The ledger stands at 64.** **Still to judge:**
`cpufreq-info` and `cpufreq-set`
(cpupower) and `thermal-monitor`, `thermal-conf` (thermald), which wait on
the kernel's cpufreq and thermal modules reaching `/sys`; `hostnamectl`'s
four domain names (which belong to `hostname`, if anywhere -- **done**, see
below); `xdg`'s two;
and `efivar`, `volname`, `lshw`, `inotifywatch`, `userdbctl`. `sudo`'s
`visudo` and `sudoreplay` are to be split into crates.
`hostnamectl`'s `dnsdomainname`, `domainname`, `nisdomainname` and
`ypdomainname` moved to `hostname`, now a port of Debian's `hostname` 3.23,
which picks its default from those names as Debian installs them; they are
**kept, fifth batch**, as further names of that binary, and the ledger now
reads coreutils' binaries as well as the crates' so it can see them (the four
rows are renamed, not added). **The ledger stands at 64.**

**Fifth pass** (2026-10-01). Three names went for answering with something
made up, or for something SlateOS does not have: `lshw` (hwinfo -- its "H/W
path" column was `/sys/device/<index>`, a path that names nothing, and its XML
was not lshw's), `userdbctl` (loginctl -- `services` listed three systemd
varlink services SlateOS has none of, and `user`/`group` read a UID or GID that
did not parse as 0, which is root), and `volname` (eject -- no current
distribution ships it, SlateOS has no optical drive device, and it read the
whole device to look at 32 bytes). Two crates went whole, under §1006 and
§1049: **`userspace/cpupower`** (`cpufreq-info`, `cpufreq-set`), which reported
2.4 GHz current, an 800 MHz to 4.5 GHz range and three governors as this
machine's whenever sysfs had no cpufreq data -- every run here -- where no
frequency source exists or is planned (roadmap's `/sys/devices` row); and
**`userspace/thermald`** (`thermal-monitor`, `thermal-conf`), whose daemon
printed "D-Bus interface enabled" and "daemon ready" and exited having managed
nothing, under two tool names no upstream ships. **Kept, sixth batch:**
`efivar` (efibootmgr: both read efivarfs and say so when it is absent),
`sudoedit` (sudo, as upstream links it), `inotifywatch` (inotifywait), and
`xdg-mime` and `mimeopen` (xdg-open). Four crates are now built under their
command's name -- `lscgroup`, `sar`, `inotifywait`, `xdg-open` -- which also
corrected the producers named in batches two and three. **The ledger stands at
57, and every name left in it is decided:** the kept batches wait on lane D's
manifest, and `visudo` and `sudoreplay` on the split into crates of their own.
**`sudoreplay` was deleted instead** (2026-10-01, §1049): nothing on SlateOS
records sudo sessions -- `log_input` and `log_output` are accepted and record
nothing, which `visudo -c` says -- so it could only ever report that there
were none. **The ledger stands at 56**; `visudo` alone waits on its crate.
**`visudo` is a program of its own** (2026-10-01): the `sudo` package is now
a library -- the sudoers file's model, parser and check, and the editor
both programs launch -- with two binaries, `sudo` (answering to `sudoedit`
too, as upstream links it) and `visudo`, as upstream builds `visudo` apart
from `sudo` from one tree. `visudo` holds none of the right to change
identity. **The ledger stands at 55, and every name in it is a kept name
waiting on lane D's manifest.**

**The 9 new shadowed pairs were the urgent half**, because a shadowed name is
two implementations that can disagree with the winner picked by packaging:

| Shadowing crate | Name | Who really provides it |
|---|---|---|
| `userspace/nologin` | `true`, `false` | coreutils |
| `userspace/nproc` (retired 2026-09-25) | `tty`, `logname` | coreutils |
| `userspace/fuser` | `lsof` | `userspace/lsof` |
| `userspace/hostnamectl` | `hostname` | `userspace/hostname` |
| `userspace/resolvectl` | `nslookup` | `userspace/nslookup` |
| `userspace/useradd` | `newgrp` | `userspace/newgrp` |

**`userspace/swapon` / `free` left this table on 2026-09-12**, and it was
never quite the same kind of entry as the rest. The others shadow a name with
a *working* implementation and the question is which one should win. `swapon`
shadowed `free` with **nothing**: its module doc advertised a `free`
personality, there was a `Personality: free` section header with no code
under it, and a unit test asserted that `free` was extracted correctly from
`argv[0]` -- a value `main` then dropped on the floor, because the dispatch
was `match { "swapoff" => …, _ => cmd_swapon }`. The name fell through to
`cmd_swapon`, which with no arguments prints the **swap summary and exits 0**.

So the hazard was not "two implementations disagree", it was "one of them
answers a memory-report request with a swap table and a success code" --
wrong output under a success exit, which is the class of bug that never gets
filed because it looks like an answer. It was latent only because
`create-ext4-rootfs.sh` stages neither `swapon` nor a `free` alias for it;
`multicall-aliases.py` could not see it either, since that checker reads the
dispatch and this claim lived only in prose.

The first fix was an arm refusing that one name. `multicall-aliases.py`
rejected it at pre-push and was right — per §1005 the shadowing branch is
deleted, because the better implementation of a name wins and the duplicate
goes. (This sentence cited §1019 until 2026-09-25; §1019 is about exec-or-
refuse and never said this.) But deleting it and restoring the catch-all would have put the
silent wrong answer back *invisibly*: the checker reads dispatch arms, so a
catch-all lets a binary answer to every name on earth while declaring none.
`main` now dispatches `swapon` and `swapoff` explicitly and refuses anything
else, which closes it for all names rather than for the one that was noticed.

The module doc also lost a second false claim, found while fixing the first:
it listed `/proc/meminfo` among the files read, a path appearing nowhere else
in the crate. It survived by sharing a sentence with `/proc/swaps` and
`/etc/fstab`, which are real.

**Separately — `multicall-aliases.py` did not strip comments before looking
for dispatch arms, and fixing that nearly did more damage than the bug.**
A comment in the new code reading ``the hardcoded `"free" => refuse` arm`` was
counted as a live personality: the checker reported 169 personalities and 1
shadowed name and refused a push, then reported 168 and 0 when that one
comment was reworded, with no code change. It was flagging a sentence about a
branch that no longer existed.

The obvious fix — run `rustlex.strip_noise(keep_literals=True)`, which exists
for exactly this and whose docstring already diagnoses it as "the scan matches
its own documentation" — **silently deleted three real personalities**:
`crond:anacron`, `kill:killall`, `newgrp:sg`. Each was genuine code
(`"sg" => sg_main(&rest)` is a dispatch arm), and each was being detected only
because of prose. The corroborating pattern `_EXTRACTS_NAME` matched the token
`argv[0]`, which is not Rust and can therefore only ever appear in a comment,
while the code spelling it looked for — the contiguous `args.first()` — is
broken over two lines by rustfmt in all three crates:

    let prog_name = args
        .first()

So the gate was resting on comments for its evidence in those three cases, and
blanking comments took the evidence away. **That is the dangerous direction:**
the original bug over-reported and cost a push; this under-reported and would
have let a real dispatch go unseen, which is the entire thing the gate exists
to catch. It was found only by diffing the full personality list across the
change — the summary line moved 168 → 165, an amount small enough to read as
the intended fix. The checker's own self-test docstring says why that is the
trap: *"a count is the one output where a detector that stopped seeing and a
tree that got better are spelled identically."*

Fixed by making `_EXTRACTS_NAME` whitespace-tolerant so corroboration comes
from code, leaving `argv[0]` inert. Both halves now have self-test fixtures,
and each was verified to fail against a mutant with its own half reverted —
a fixture that passes with and without the fix proves nothing.

`nologin` answering to `true` and `false` is the one to look at first: those
two run in nearly every shell script on the system, and `nologin`'s job is to
*refuse* and exit non-zero.

**The fix per name is a decision, not a patch.** Either the personality is
deleted (the better implementation of a name wins — §1005), or the
name gets a real producer, preferably its own crate so it gets its own
capability identity. Both ledgers may only shrink, so the count is the
progress bar — with the caveat this entry exists to record: the count is only
a progress bar while the instrument holds still.
