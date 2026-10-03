## TD-B-TWO-PACKAGES-BUILD-A-BINARY-CALLED-LOGGER (lane B, 2026-09-16) — FIXED 2026-09-16

**Status: FIXED**, 2026-09-16. `userspace/coreutils/src/bin/logger.rs` is
deleted and `userspace/logger` is the only program of that name, so `/bin/
logger` no longer depends on link order. The five-step plan below was followed
in order, with the deletion last; `scripts/check-bin-collisions.py` refused to
pass until the `logger` line left its `KNOWN_COLLISIONS`, so the baseline
shrank as the rule requires. `kill` remains, and is the harder one --
TD-B-TWO-PACKAGES-BUILD-A-BINARY-CALLED-KILL.

**In short:** there are two different programs in this tree called `logger`,
and they both compile to the same file. Every crate links into one shared
directory, so `userspace/logger` and `userspace/coreutils/src/bin/logger.rs`
both write `target/<triple>/<profile>/logger`, and the one that survives is
whichever the compiler happened to link last. `scripts/create-ext4-rootfs.sh`
then copies that single file onto the disk image as `/bin/logger`. **Which
`logger` SlateOS ships is therefore decided by build order, not by anyone's
decision.** The two are not near-identical: one accepts thirteen options and
the other accepts two, so a script that works today can stop working after an
unrelated rebuild, with no source change to blame.

### How it was proved

The same path was run twice, half an hour apart, with no edit in between:

```
before a rebuild:  logger -i   ->  logger: invalid option -- 'i'
after  a rebuild:  logger -i   ->  (accepted; -h prints a usage block)
```

Cargo says so too, and has all along — it is a warning in a build that prints
thousands of lines, which is why nobody read it:

```
warning: output filename collision at target/x86_64-pc-windows-gnu/debug/logger.exe
  = note: the bin target `logger` in package `logger` has the same output
          filename as the bin target `logger` in package `coreutils`
  = note: this may become a hard error in the future
```

### Where it lives

- `userspace/logger/` — package `logger`, ~1183 lines. A syslog client:
  `-p/--priority`, `-t/--tag`, `-i/--id`, `-f/--file`, `-s/--stderr`,
  `-u/--socket`, `-n/--server`, `-P/--port`, `--json`, `--size`, `--pid`,
  `-h/--help`, `--version`. Facility/severity parsing, RFC3339, JSON output.
- `userspace/coreutils/src/bin/logger.rs` — bin `logger` of package
  `coreutils`, 654 lines of which most are tests. Accepts `-t` and `-p` and
  `--`, and rejects everything else with `logger: invalid option -- 'X'`.

`scripts/create-ext4-rootfs.sh` builds `-p coreutils -p ar -p kill -p logger
-p logrotate` — that is, it builds *both* of these on purpose, having been
written as though they were different programs, which they are.

### What it cost besides the shipped file

Both copies were maintained, in ignorance of each other:

- `e12942c8d logger: carry the message as bytes, from argv and from stdin`
  (the coreutils applet)
- `60468ac46 logger: read argv as bytes, and refuse a message rather than
  corrupt it` (the standalone crate)

That is the same fix, made twice, to two files, each time by someone who had
one of them open and no reason to suspect the other. Effort spent on whichever
copy loses the link race is invisible: it compiles, its tests pass, and it is
not the program that runs.

It also corrupts `scripts/option-gap-baseline.txt`, which lists twelve
`logger` gaps (`-P -S -T -V -d -e -f -h -i -n -s -u`). Those twelve were
measured against whichever `logger` won on the day the baseline was taken.
They are not stale — they are **measurements of a subject that changes between
builds**, which is worse, because re-running the harness can flip them without
anyone touching `logger` at all.

### The proper fix — and why it is NOT "delete the applet"

The first draft of this entry said the standalone's option surface was "a
strict superset of the applet's `-t`/`-p`, so deleting the applet loses no
capability", and added that the claim should be confirmed behaviourally before
acting on it. It was, the same hour, and **it is false.** Both programs were
built to separate files and run side by side:

| invocation | `coreutils` applet | standalone `logger` | util-linux reference |
|---|---|---|---|
| `-Q` | `logger: invalid option -- 'Q'` | `logger: unknown option '-Q'` | `logger: invalid option -- 'Q'` then `Try 'logger --help' for more information.` |
| `-p nosuch.zz hi` | `logger: unknown priority: nosuch.zz` | `logger: unknown priority: 'nosuch.zz'` | `logger: unknown facility name: nosuch` |
| `-t TAG hello` | `<13> 2026-09-16T11:16:03 TAG: hello` | `<13>Sep 16 11:16:04 localhost TAG: hello` | — |
| no arguments | `<13> … user: ` (empty message, rc 0) | nothing at all (rc 0) | reads stdin |

So the surfaces cross rather than nest. The applet has **two** options and the
*correct* unknown-option diagnostic — `invalid option -- 'Q'` is what getopt
prints and what the reference prints. The standalone has **thirteen** options
and gets that diagnostic wrong. Neither matches the reference on a bad
priority, where it names the facility component (`unknown facility name:
nosuch`) rather than echoing the whole argument, and neither prints the
`Try 'logger --help'` line at all. The message formats differ from each other
too: RFC3339-style with no hostname versus RFC3164 with a literal `localhost`.

Deleting either therefore loses something real. The fix is a **merge**:

1. keep `userspace/logger` as the surviving program — it has the eleven extra
   options, and the rootfs script already names `-p logger`;
2. take the applet's `invalid option -- 'X'` wording with it, and add the
   `Try 'logger --help' for more information.` line the reference prints;
3. correct the priority diagnostic on the survivor to name the facility, as
   the reference does;
4. settle the message format against the reference before deleting anything,
   since that is the part no option list reveals;
5. only then delete `userspace/coreutils/src/bin/logger.rs` and remove the
   `logger` entry from `KNOWN_COLLISIONS`.

**The general lesson is the one this entry nearly failed to learn.** Comparing
two programs by their option *lists* answers "which accepts more flags", which
is not the question "which can be deleted without loss". The lists were read
first and gave a clean, wrong answer; running both binaries against each other
and against the reference gave the real one, and took about a minute. A
comparison of documentation is not a comparison of behaviour.
