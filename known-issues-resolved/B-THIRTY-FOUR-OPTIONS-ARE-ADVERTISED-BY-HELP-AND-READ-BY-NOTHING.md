## B-THIRTY-FOUR-OPTIONS-ARE-ADVERTISED-BY-HELP-AND-READ-BY-NOTHING (lane B, 2026-09-12) -- CLOSED, 0 findings

The mirror image of the unknown-option class above, found by
`scripts/check-help-vs-parser.py`. That sweep asks whether an option we do
not have gets accepted; this one asks whether an option we *advertise*
exists. It needs no reference implementation -- the program supplies both
halves of the comparison -- which is why it is worth doing now, while two
thirds of the other list waits on a reference environment.

**34 options across 14 files.** The number began at 180 and every
correction came from opening a file the tool had accused; the four
false-positive classes and their fixes are in the tool's own commit
message and docstring. Both remaining ambiguities fail toward silence, so
34 is a floor.

Two worked examples, both confirmed by hand:

- `getfacl` advertises `-a/--access`, `-d/--default` and `-n/--numeric`
  and parses none of them -- its match ends `_ => {}`. Fixing it means
  either implementing the three or removing them from the help; the help
  must stop claiming what the binary cannot do either way.
- `blkzone` advertises `-o/--offset`, `-l/--length` and `-c/--count` and
  parses none of those either.

**`getfacl` also shows why the first sweep's count is a floor.** It has
the unknown-option defect too -- `_ => {}` -- but never appeared in that
list, because `getfacl --zzq` with no file operand exits 1 with "no files
specified". It accepted the option and failed for an unrelated reason,
exactly as `blockdev` did. Two independent confirmations of the same
blind spot.

### CLOSED

`scripts/check-help-vs-parser.py` reports **0 findings**: every option
advertised by a help text in this lane is read by its parser. What was
fixed, in order: `getfacl`'s `-a`/`-d`/`-n`; `blkzone` (deleted, it
invented its output); `lscpu -B`; `lsmem`'s `--summary`, whose help line
invented a short form *and* attached it to the wrong option;
`systemd-cat`'s `-p`/`-t`, which turned out to mean the tool had never
written to the journal at all; `getfattr`'s `-e`, whose text branch was
one `-e text` away from corrupting values with `from_utf8_lossy`;
`systemd-notify`'s silently-dropped `--pid`; and `irqbalance --banmod`,
which needed no new data because the field it matches against was
already parsed into `_name` and discarded.

**The count over the tool's life: 180, 153, 74, 46, 34, 25, 19, 13, 11,
5, 3, 2, 0.** Ten of those steps were false positives, each found by
opening a file the tool had accused; three were repairs. So the tool
found roughly a dozen real defects and accused about a hundred and fifty
innocents on the way, and the only reason the last number means anything
is that every step down was checked by hand. A sweep's first number is
not a finding count -- it is a reading list.

### Progress, and the count's history

**19 findings across 11 files** as of the `blkzone` deletion, from 34 when
this was filed. Two of the drops were fixes and one was a fifth false
positive:

- `getfacl`'s `-a`, `-d` and `-n` are implemented rather than removed from
  the help; all three were cheap and useful.
- `blkzone` is **deleted**, see below.
- `objdump`'s `--radix`, `--start-address` and `--stop-address` were never
  broken: the parser holds `strip_prefix("start-address=")`, a de-dashed
  name carrying its `=`, which the checker did not recognise.

The count over the whole life of the tool: **180, 153, 74, 46, 34, 25, 19**.
The first number is nine times the last, and every correction came from
opening a file the tool had accused. Nobody should quote an untriaged
number from this or any similar sweep.

### `blkzone` was not a missing option, it was an invented answer

Reading it to add the three options found that `blkzone report` printed two
zones with hardcoded start/length/capacity/write-pointer values, for any
device, on any machine, without opening anything -- a fabricated answer to a
question about real hardware, in the format of a real answer. `reset`,
`open`, `close` and `finish` printed `blkzone: reset on /dev/sda` and exited
**0**, reporting a destructive zone operation as done when none was
attempted.

Deleted under §1005/§1006 rather than made to refuse. It was unreachable --
in `multicall-aliases-baseline.txt`, so no build produced the name -- which
means it was dead code that would have lied if anyone had wired it up.

**Two more instances, found the same way** -- by reading a file the
help-vs-parser sweep had pointed at for an unrelated reason.
`systemd-cgls` printed a fixed cgroup tree (`init.scope` with pid 1,
`dbus.service` with pid 100) on every machine, having opened nothing;
it now walks `/sys/fs/cgroup`, which needs no ioctl and no privilege.
`systemd-cgtop` printed five invented rows and now reports
`pids.current` and `memory.current`, both `-` when unreadable rather than
0 -- a group whose count is unknown is not a group with no processes.
Its `%CPU` column is `-`, which is the correct answer rather than a gap:
a percentage needs two samples and an interval, and one invocation has
neither. Measured, `systemd-cgtop -n 1` prints `-` there too. Continuous
mode is where the number would come from and is neither implemented nor
claimed.

**The `cgls` test is the cautionary half.** It asserted the output
contained `system.slice` and `user.slice`, which was true on every
machine because those names were hardcoded. A test that pins a
fabrication in place is worse than no test: it makes the invention look
verified, and it would have gone on passing forever. Its replacement
asserts what a scratch directory was given comes back *and* that
`user.slice` does not.

**This is a class, and it has been hit here before.** `read_file_acl` used
to call `fs::metadata`, discard the result, and report owner `root`, group
`root` and mode 0755 for every file. Both are the same defect: output that
is shaped like a measurement and is not one. It is not cheaply sweepable --
telling an invented constant from a real one needs a reader -- but 45
comments in lane B say `stub`, `fake`, `placeholder` or `simulated`, and
most are honest host-test shims. Worth a reader's pass, not a script's.

### The 34, as filed


```
  userspace/acl/src/main.rs
      --access             nowhere                from: -a, --access    Display access ACL only
      --default            nowhere                from: -d, --default   Display default ACL only
      --numeric            nowhere                from: -n, --numeric      Numeric UIDs/GIDs
      -a                   nowhere                from: -a, --access    Display access ACL only
      -d                   nowhere                from: -d, --default   Display default ACL only
      -n                   nowhere                from: -n, --numeric      Numeric UIDs/GIDs
  userspace/blockdev/src/main.rs
      --count              nowhere                from: -c, --count NUM       Number of zones
      --length             nowhere                from: -l, --length SECTORS  Number of sectors
      --offset             nowhere                from: -o, --offset SECTOR   Start sector
      -c                   nowhere                from: -c, --count NUM       Number of zones
      -l                   nowhere                from: -l, --length SECTORS  Number of sectors
      -o                   nowhere                from: -o, --offset SECTOR   Start sector
  userspace/iptables/src/main.rs
      --opts               nowhere                from: -m match --opts      Extended match module
  userspace/irqbalance/src/main.rs
      --banmod             nowhere                from: --banmod=MOD         Ban module IRQs
  userspace/lscpu/src/main.rs
      --bytes              nowhere                from: -B, --bytes        Print sizes in bytes
      -B                   nowhere                from: -B, --bytes        Print sizes in bytes
  userspace/lsmem/src/main.rs
      -s                   nowhere                from: -s, --summary[=WHEN] Summary (auto, only, never)
  userspace/objdump/src/main.rs
      --radix              nowhere                from: --radix=N  Radix (8, 10, 16)
      --start-address      nowhere                from: --start-address=ADDR
      --stop-address       nowhere                from: --stop-address=ADDR
  userspace/oils/src/main.rs
      -C                   nowhere                from: -e -x -u -f -C …             Single-letter `set`
      -e                   nowhere                from: -e -x -u -f -C …             Single-letter `set`
      -f                   nowhere                from: -e -x -u -f -C …             Single-letter `set`
      -u                   nowhere                from: -e -x -u -f -C …             Single-letter `set`
      -x                   nowhere                from: -e -x -u -f -C …             Single-letter `set`
  userspace/pstree/src/main.rs
      --compact            nowhere                from: -c, --compact=no    Don't compact identical subt
  userspace/systemctl/src/main.rs
      --identifier         nowhere                from: -t, --identifier=ID  Set syslog identifier
      --pid                nowhere                from: --pid=PID       Send from specific PID
      --priority           nowhere                from: -p, --priority=PRIO  Set syslog priority (0-7)
  userspace/vmstat/src/main.rs
      --timestamp---       nowhere                from: ---timestamp---
  userspace/wget/src/main.rs
      --request            nowhere                from: ---request begin---
      --response           nowhere                from: ---response begin---
  userspace/xattr/src/main.rs
      --encoding           nowhere                from: -e, --encoding ENC Encoding (text, hex, base64)
  userspace/xdg/src/main.rs
      --icon               nowhere                from: --icon
```
