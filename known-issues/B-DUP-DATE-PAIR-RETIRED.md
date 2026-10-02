## B-DUP-DATE-PAIR-RETIRED (lane B, 2026-09-12)

**Measured, then deleted.** `scripts/date-diff.sh` against both halves, same
harness, same cases, minutes apart:

| half | result |
|---|---|
| `userspace/coreutils` `date` | **80 passed, 41 differed** |
| `userspace/date` (standalone) | 52 passed, 69 differed |

Twenty-eight cases apart, so §1005 applies without argument: coreutils is the
one home, the better half survives inside it, the duplicate crate goes.
`userspace/date` is deleted.

**The option counts would have got this right and that is not why it was
decided.** `dup-bins-survey.py` had coreutils ahead 4030 to 1682 — and its own
header says five pairs ranked that way have since been measured and the ranking
was wrong every time, twice backwards and three times calling a landslide
close. A count of what a source MENTIONS is not a count of what it does.

**The pre-delete audit, because `stat` taught this one.** `userspace/stat` is
not a `stat`: it dispatches on `argv[0]` and also serves `touch`, `mkfifo`,
`readlink`, `realpath` and `ln`, so retiring that crate would have deleted six
programs. Checked here before removing anything: `userspace/date` is one
`main.rs`, one `[[bin]]`, no `argv[0]` dispatch, no crate depends on it, and
`create-ext4-rootfs.sh` does not stage it. It joined the workspace through the
`userspace/*` glob, so no `Cargo.toml` edit was needed either.

**Verified after, not assumed:** `cargo build -p coreutils --bin date` still
produces a `date` — it is auto-discovered from `src/bin/`, not declared, which
is worth stating because a grep for `name = "date"` in that manifest returns
nothing and would read as evidence the survivor did not exist. The harness
still reports 80/41 with the standalone gone.

Eight pairs remain. Four still have no harness (`free`, `kill`, `logger`,
`ps`, `uptime` — five), and the survey's advice stands: run the harness, do not
delete on a count.
