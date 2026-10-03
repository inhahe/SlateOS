## B-SEVENTY-PROGRAMS-ACCEPT-AN-OPTION-THEY-DO-NOT-HAVE-AND-EXIT-ZERO (lane B, 2026-09-12) -- REOPENED 2026-09-13, then closed again with a second probe

### REOPENED 2026-09-13: the 0-accepting figure was measured by a probe that cannot see half the class

The count below was true of what it measured and the measurement was too
narrow. The sweep runs `<prog> --zzq-not-an-option` **with no other
arguments**, and reads the exit code. That cannot distinguish

    the program rejected the option            (what we want to know)
    the program ignored the option and then
      failed its own missing-operand check     (what also exits 1)

Nine programs were in the second group and read as refusing. They printed
"unknown option: X" from inside the argument loop and *kept parsing*, so the
refusal only ever appeared when the bad option was the sole argument. Add an
operand and it evaporates:

    $ cgcreate --zzq-not-an-option -g cpu:/zzqproceed
    cgcreate: unknown option: --zzq-not-an-option
    cgcreate: created /sys/fs/cgroup/zzqproceed
    $ echo $?
    0

The diagnostic is printed AFTER the work. Two more said nothing at all --
`lscgroup --zzq-not-an-option` produced output byte-identical to `lscgroup`,
exit 0 -- and `lssubsys` hid the other way round, exiting 1 because
/proc/cgroups does not exist on the dev host rather than because it rejected
anything. Same false reading from opposite causes.

**The distinguishing probe is the option PLUS valid work.** A one-argument
probe measures "this command refuses something", not "this command refuses
this option", and those come apart exactly when the command has work it could
otherwise do -- which is every case that matters.

Found statically instead, by looking for unknown-option diagnostics with no
stop in the same block: 122 such diagnostics in lane B's tree, 11 without a
stop, 6 of them real. The other 5 are correct as written (three are match arms
whose tail expression IS the exit code; one is `getopt(1)` deliberately
accumulating a flag so it can report every bad option rather than the first;
one has its `return 1` one block further out than the detector looked).

Fixed 2026-09-13: cgcreate, cgdelete, cgset, cgget, cgclassify, lscgroup,
lssubsys (`7dd5c80ec`), wipefs and blkdiscard (`a2cb51758`), fuser,
inotifywait, inotifywatch and udisksd (`c142fa0d0`). All re-probed with an
operand present. `cgexec` was deliberately left alone: everything after its
options is the command to run, so `cgexec -g cpu:/x ls --colour` must pass
`--colour` through, and its catch-all arm is a correct end-of-options marker.

The two data-destruction cases are the ones to remember the class by. `wipefs`
and `blkdiscard` both printed the refusal and then operated on the device --
an argument the program did not understand is precisely the moment not to
proceed.

### The original close, which stands for what it measured



**Closed 2026-09-13, tree-wide.** The sweep reports **0 accepting** and 555
refusing, against 510 refusing and 44 accepting when this pass began.

The five I last recorded as open -- `clipboard`, `credentials`, `desktop`,
`match3`, `pinball` -- were lane C's and were already fixed when I wrote that
line; I had measured before their work merged. Lane C said so, I rebuilt all
five and probed them directly rather than taking it, and they refuse with exit
2 while `--help` still exits 0, so the refusal is not blanket. **44-to-5 was
really 44-to-0**, and the only reason the wrong number was here is that a
count is only as current as the last time somebody re-measured it -- which is
the failure this file records in four other people's comments a few entries
above.

### The block was the wording, and the wording was not the defect

This entry used to end "blocked on the reference environment rather than on
anyone's time", because §371 forbids inventing a diagnostic and most of these
programs have no upstream installed to quote. That reasoning conflated two
things. §371 forbids **asserting a measurement not taken** -- writing
`unrecognized option` and implying GNU says so. It does not require leaving a
program that accepts options it does not have.

And the convention did not have to be invented, only measured somewhere else:
`<prog>: unknown option: <arg>`, exit 1, is what `ntpd`, `getty`, `findmnt`,
`acpi`, `blkid`, `upower` and `ss` already printed. `resolvectl` keeps its own
wording because it *has* a reference and quotes it. Recorded as a judgment
call in todo.txt, with the trigger for revisiting it: a reference environment,
then re-measure and correct any that differ.

### The sweep's count was a floor, not a total

Five programs were broken and not in its list of 44, each for a different
incidental reason. This is the part worth carrying to the next sweep of any
kind:

| program | why the sweep missed it |
|---|---|
| `atd` | daemons are skipped rather than launched |
| `servicebus` | same |
| `cpufreq-set` | needs privileges this host lacks, so it failed before reaching the defect |
| `tuned-gui` | listed only as a personality of `tuned`, not probed separately |
| `coredump-extract` | **broken by my own fix mid-sweep**, and only caught by re-running the tool |

The last is the sharpest. `run_coredump_extract` called `cmd_dump(&rest);` and
discarded the result; when `cmd_dump` gained a return value the caller kept
dropping it, so the guard fired, printed, and the personality still exited 0.
Rust does not warn on a discarded `i32`. Build clean, clippy silent, tests
green -- nothing in the toolchain had an opinion. **Re-run the instrument at
the end; a tally kept by hand across fifteen commits is not evidence.**

### Probe the personality, not the crate

Six times a parent crate refused correctly while its personality did not, and
once all five personalities of one crate were broken while the crate itself
was fine. Probing `audit`, `capsh`, `rfkill`, `sbctl` and `prlimit` said
everything was well; probing `autrace`, `captest`, `rfkill-event`,
`sbkeysync` and `ulimit` found five defects.

### Eleven root causes, and no patch that could have been applied blind

| shape | crates |
|---|---|
| catch-all swallows the option | `at`, `coredumpctl`, `lsirq`, `hwinfo`, `ulimit` |
| no catch-all at all: the parser *asks* for the flags it wants | `sysstat`, `cpupower` |
| the option becomes **data** -- a boot entry, a program to trace, a timespec, a log message | `grub2`, `autrace`, `at`, `logger`, `numactl` |
| the option becomes a **filter**, so a query silently becomes a different query | `coredumpctl` |
| diagnostic printed, exit status 0 | `m4`, `ftp`, `coredump-extract` |
| "// ignore unknown flags", written down as a decision | `dbus` |
| the program never reads `env::args()` at all | `loginmgr`, `servicebus`, `shell` |
| daemon starts instead of failing | `tuned`, `thermald`, `fwupd`, `dbus`, `irqbalance` |

`grub2` is the one to reread if this class ever looks cosmetic:
`grub-set-default --zzq-not-an-option` **wrote** `saved_entry=--zzq-not-an-option`
into the grubenv and reported success. Not a fabrication -- a real write, of
a default boot entry naming a menu entry that does not exist.

### Two things the fixes had to get right, or they break real usage

**A value is not an option.** `sar -n DEV`, `numactl -m 0 ./prog`,
`cpufreq-info -c 0`, `coredumpctl --since yesterday`, `lsirq -o IRQ,TOTAL` --
a check that reported the *value* as unknown would have broken every real
invocation while appearing to work. Each has a test.

**`args_os`, not `args`.** `env::args()` panics on an argument that is not
valid Unicode, so a guard written with it crashes on exactly the input it
exists to reject. The pre-push `argv-utf8` gate caught this in the first
version of the `loginmgr`/`servicebus`/`shell` guard; the rest were written
with `args_os` and `quoteaf_os` from the start.

Found by `scripts/unknown-option-sweep.py`, which runs every binary in an
empty directory with nothing but a bogus long option and looks at what it
does. The sweep exists because a file named `--list.lock` was sitting in the
repository root: `flock` had been handed `--list`, had not recognised it, and
had locked it.

**Three programs made a file out of the option** and are fixed: `flock`
(created `--list.lock`), `lockfile` (created `--zzq` *and exited 0*), and the
`nohup` personality of the `timeout` crate (created `nohup.out`; removed
outright, coreutils already had a correct `nohup`).

**Seventy more accepted the option and exited 0** without a filesystem side
effect. Eleven are fixed -- `nproc`, `arch`, `pathchk`, `users` (all one
crate), `lscpu`, `lsmem`, `blkzone`, and `clear`, `tset`, `lsattr`,
`getcap`, `getopt`, the five `systemd-*` personalities, and
`resolvectl`/`resolvconf`/`systemd-resolve`. The remaining 50 are
listed below.

`getopt` is worth singling out. It accepted an unknown option by making
it the *optstring*, and fixing that surfaced a second, older defect:
`parse_options` printed `invalid option -- 'x'` for the command line it
was asked to parse and returned only the words, so the caller exited 0.
`args=$(getopt "$@") || exit` -- the documented way to use the program --
was told a rejected line had parsed cleanly. The two failures also carry
different statuses, measured: **2** for an option of getopt's own, **1**
for one in the line it parsed.

The last four are the group that has no long options at all, where the
wording is `invalid option -- 'X'` naming the first character getopt has
no option for -- *not* the second byte of the argument, which is right
for `-z` and `--zzq` and wrong for `-Rz`. The rule and its measurements
live on `usageerror::invalid_option`.

The wording to fix them with is in `userspace/usageerror`; it is getopt's,
not ours, and was measured in the C locale. The exit status is *not* in that
crate and must be measured per tool: coreutils exits 1, util-linux's `flock`
exits 64, and util-linux's `lscpu`, `lsmem`, `prlimit` and `blkzone` exit 1.

**`prlimit` needs more than a refusal** and is called out separately: its
`--<resource>` arm silently ignores a resource name it cannot parse, and it
drops the trailing command entirely, so `prlimit --nofile=10 cmd` never runs
`cmd`. Refusing unknown options there without fixing that would paper over
the larger gap.

**`sysstat`'s five personalities are blocked on a reference.** `mpstat`,
`pidstat`, `tapestat`, `cifsiostat` and `sysstat` itself are all in the list,
but the sysstat package is not installed in the WSL reference environment and
`apt-get` needs a password this session does not have. Their wording is
therefore unmeasured, and guessing it is exactly what §371 forbids. Trigger to
promote: sysstat available in the reference environment.

### A neighbouring class, found while fixing this one: help that lies

`systemd-cat --help` advertises `-p, --priority=PRIO` and
`-t, --identifier=ID`. `run_cat_journal` takes no arguments and
implements neither. `blockdev`'s parser accepted `--setfra`, consumed its
value, and then reported `unknown operation`, because the option was in
the parser's value-taking list and not in the executor's. Both are the
same defect from the other side: not "an option we do not have is
accepted", but "an option we advertise does not exist".

It is sweepable the same way the first one was, and more cheaply, since
it needs no reference implementation: parse each binary's own `--help`
for the options it names, then check the parser recognises every one.
Any disagreement is a defect in one direction or the other, and the
program tells you both halves itself. Not started; noted here so the
idea is not lost with the tick that had it.

### Most of what is left is blocked on a reference, not on effort

Of the 58 remaining, only about **14** have a reference implementation in
the WSL environment this lane measures against: `ifconfig`, `objdump`,
`prlimit`, `resolvectl`, `resolvconf`, `route`, `dnsdomainname`, `lshw`,
and the five `systemd-*` personalities. (`ulimit` reads as available but
that is the shell builtin; util-linux ships no such binary.) The rest
would have to have their wording invented, which is what §371 forbids, so
they are blocked on the reference environment rather than on anyone's
time. Installing the packages needs a password this session does not have.

Wordings already measured, so the next pass does not have to re-derive
them -- and they are all different, which is the argument for measuring
each one rather than pattern-matching from the last:

| Tool | Exit | First line |
|---|---|---|
| `systemd-cat`/`-cgls`/`-cgtop`/`-escape`/`-path` | 1 | `unrecognized option '--x'`, one line, no pointer |
| `resolvconf`, `resolvectl` | 1 | same wording |
| `objdump` | 1 | same wording, then the full usage |
| `dnsdomainname` | 255 | same wording |
| `route` | **0** | same wording, then the address-family list |
| `ifconfig` | 1 | ``option `--x' not recognised.`` then ``` `--help' gives usage information.``` |

`route` exiting 0 is not a transcription error: net-tools really does
report the option and succeed. Ours differs by printing *no* diagnostic
at all, so it is still a finding -- the fix there is the message, not the
status.

### Two things the sweep does not see, both verified rather than assumed

**A program that creates the file and removes it again before exiting reads
as clean.** A create-then-remove probe reports nothing. This is very likely
what the original `flock` did -- probing it in a scratch directory showed a
clean run while a real `--list.lock` sat on disk, so that invocation simply
never reached its cleanup.

**A program can accept the option and still exit non-zero for an unrelated
reason**, which reads as a refusal. `blockdev` is the proof: it does not
appear in the list below, because on this host it accepted `--zzq`, went on
to the device, and exited 1 with `cannot read the device size from sysfs`.
On a machine where `/dev/sda` exists it would have exited 0. It was found
only because `blkzone`, its argv[0] sibling, was flagged and the crate was
opened anyway. So the count of 70 is a floor, not a total.

### Still open (45 -- 40 lane B, 5 lane C), regenerated 2026-09-12

**Five of these are not lane B's.** `clipboard`, `credentials` and
`desktop` are in `gui/`; `match3` and `pinball` are in `apps/`. The list
below is the sweep's raw output, which knows about binaries and not about
lane boundaries, and reading it as this lane's backlog would be wrong.
They are filed to C as
`requests/b-c-five-gui-and-app-programs-accept-an-option-they-do-not-have.md`.

**All 40 of lane B's are blocked on a reference.** Checked: not one of
them has an upstream installed in the WSL environment this lane measures
against, so every refusal wording would have to be invented rather than
measured, which is what §371 forbids. The exception is the handful with
no upstream at all -- `sanitize`, `shell`, `servicebus`, `loginmgr` look
like SlateOS originals -- where there is nothing to match and the house
wording in `userspace/usageerror` is a free choice rather than a guess.
That subset is the only part of this entry that is actionable today.

Not hand-maintained. The previous list here had drifted: it still named
`route`, `dnsdomainname`, `lshw` and `objdump` as open hours after they
were fixed, because the count was being edited by hand while the sweep
was the real source of truth. Regenerate with

    python scripts/unknown-option-sweep.py

and paste. Lane A's argument for a `--pin` mode applies to prose lists
too: a count that can be regenerated beats one that has to be trusted.

**The inputs were checked before the count was believed.** The sweep
reads binaries, and 85 of 191 were older than their source -- three of
them fixed the same day, because `cargo test` and `cargo clippy` had
been run on those crates and `cargo build` had not. The sweep now
refuses to report in that state. Worth recording honestly: the count
over stale binaries was 44 and over current ones is 45, so the error
here was one, not a catastrophe. That the difference turned out small is
not a reason the check was unnecessary -- its size was unknowable in
advance -- but the reason it was small is worth knowing. Of the three
stale-and-fixed crates, none had been fixed for *unknown options*:
`xattr` was an encoding value, `acl` gained `-a`/`-d`/`-n`, `irqbalance`
gained `--banmod`. Their staleness could not move this particular count.

One binary, `irqbalance`, is still stale and could not be rebuilt: the
file is locked by this host's continuous backup scanners, the same
condition lane A recorded as
`A-TEST-RECLAIM-SPACE-RACES-THIS-HOST-BACKUP-SCANNERS`. It appears below
either way -- its fix added an option and no refusal -- so its staleness
does not change its verdict.

- `clipboard`
- `coredumpctl`
- `credentials`
- `dbus`
- `desktop`
- `ftp`
- `fwupd`
- `gdb`
- `hwinfo`
- `logger`
- `loginmgr`
- `lsirq`
- `m4`
- `match3`
- `numactl`
- `pinball`
- `sanitize`
- `selinux`
- `servicebus`
- `shell`
- `sysstat`
- `thermald`
- `tuned`
- `wpa`
- `atq (via at)`
- `autrace (via audit)`
- `captest (via capsh)`
- `cifsiostat (via sysstat)`
- `cpufreq-info (via cpupower)`
- `getenforce (via selinux)`
- `grub-reboot (via grub2)`
- `grub-set-default (via grub2)`
- `mpstat (via sysstat)`
- `numademo (via numactl)`
- `numastat (via numactl)`
- `pidstat (via sysstat)`
- `restorecon (via selinux)`
- `rfkill-event (via rfkill)`
- `sbkeysync (via sbctl)`
- `sestatus (via selinux)`
- `tapestat (via sysstat)`
- `tuned-gui (via tuned)`
- `turbostat (via cpupower)`
- `ulimit (via prlimit)`
- `update-grub (via grub2)`
