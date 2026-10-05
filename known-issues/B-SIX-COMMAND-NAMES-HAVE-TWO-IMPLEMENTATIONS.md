## B-SIX-COMMAND-NAMES-HAVE-TWO-IMPLEMENTATIONS (lane B, 2026-09-10) — pinned, not fixed

**In short:** Six crates answer to a command name that a *different* crate or
coreutils bin already produces an executable for. The shadowing branch cannot
be reached, and the two implementations can drift apart with nothing noticing.

| Shadowing crate | Name | Real producer |
|---|---|---|
| `userspace/chown` | `chmod` | coreutils bin |
| ~~`userspace/chpasswd`~~ | ~~`passwd`~~ | ~~`userspace/passwd`~~ — **alias removed 2026-09-10** |
| ~~`userspace/head`~~ | ~~`tail`~~ | ~~coreutils bin~~ — **crate deleted 2026-09-10** |
| ~~`userspace/pv`~~ | ~~`fuser`~~ | ~~`userspace/fuser`~~ — **alias removed 2026-09-10**, after `-n` was made real in the producer |
| ~~`userspace/sysstat`~~ | ~~`iostat`~~ | ~~`userspace/iostat`~~ — **alias removed 2026-09-10**; `userspace/iostat` has the features and now has the tests |
| ~~`userspace/who`~~ | ~~`w`~~ | ~~`userspace/w`~~ — **alias removed 2026-09-10**; `userspace/w` is now `userspace/finger` and `who` owns `w` |

**`chpasswd:passwd` is RESOLVED 2026-09-10** — 404 lines removed, chpasswd is
713 lines and does one thing. The account below is the scoping that made the
second attempt work, kept because the first attempt failed for a reason worth
remembering: it was done in dependent steps, and each step's breakage hid the
next step's target.

**The scoping, as written before the attempt.** The verdict
is clear and the work is not small.

*The verdict:* `userspace/passwd` is the producer and the better one — 2,083
lines and 61 tests to chpasswd's 755 live lines and 31 — and it implements
every operation the shadowing branch did (`-l -u -d -e -S`). chpasswd's extra
flags (`--encrypted`, `--md5`, `--sha512`) are **chpasswd's own**, not
passwd's; comparing flag sets across a multicall binary mixes both
personalities and makes the shadow look richer.

*Why it is not a two-line change:* removing the alias leaves a one-variant
enum — the shape of the removed thing left behind, which the next reader will
reasonably add a second variant to. Doing it properly touches the
`Personality` enum, `detect_personality`, the `Config` field, the argument
`match`, `print_help`, `print_version`, the `run_*` dispatch and six tests.
Attempted across several dependent steps; the failures cascaded and it was
reverted. Tree clean, 31 and 61 tests pass.

*What the attempt established for whoever finishes it:* the `-l/-u/-d/-e/-S`
arm is the only personality-specific parsing — everything else is shared
password-file machinery that stays. And the non-UTF-8-username property does
**not** need moving: `userspace/passwd` already has `not_text` and
`an_argument_that_is_not_text_is_a_name_and_not_a_crash`, at lines 1591 and
1612. I nearly duplicated them because I searched for them and piped the search
through `head -6`, which cut the output above line 1591.

**A second `who` existed and it was the thin one — deleted 2026-09-10.**
`coreutils/src/bin/who.rs` was 359 lines with 17 tests against
`userspace/who`'s 1,691 and 53, and it **ignored its arguments entirely**
while documenting `Usage: who [-a]`. So `who -a` silently behaved like `who`.

That is the case this entry warned about — "if the shadowing implementation is
the better one, the fix is to make *it* the producer and delete the other" —
arriving from an unexpected direction. `head:tail` resolved the opposite way
the same day: there the shadowing crate was the thin one and coreutils had
both. **The rule is not "prefer coreutils" or "prefer the standalone"; it is
read both.**

The `who:w` row was unaffected then: `userspace/who` still answered to `w`.
**Resolved 2026-10-02:** `userspace/who` is deleted, and `who` (GNU 9.4's) and
`w` (procps-ng 4.0.4's) are two coreutils programs, each its own executable.
| `userspace/cron` | `crond` | `userspace/crond` |
| `userspace/cron` | `crontab` | `userspace/crontab` |

**Eight, not six, as of 2026-09-10 — and the name of this entry is now wrong.**
Kept anyway, because the ID is cited from `design-decisions.md` and from the
baseline file, and a dangling reference costs more than a stale numeral. The
two extra were not newly written: `scripts/multicall-aliases.py` matched the
dispatch comparison only against twelve enumerated variable names, and
`userspace/cron` calls its lowercased `argv0` `lower`, so **nine** of its
personalities were invisible to the gate for as long as the gate existed. It
follows the assignment chain now.

**`cron:crond` is the dangerous one, and it is the `udisks`/`umount` shape
exactly.** `userspace/cron`'s `crond` personality prints
`crond: scheduler ready (simulated)` and exits; `userspace/crond` has a real
loop that `Command::new("/bin/sh")`s the job. Two implementations of one daemon
name, one of which does nothing, and which one a user would get is decided by
whichever binary lands at `/sbin/crond`. Neither is installed today — the
rootfs stages only `services/fastpy-*` binaries — so this is latent rather than
live, which is the only reason it is an entry and not an incident.

**Still not counted: `cron` also answers to `at`**, because `at` is its
`else` branch and has no string literal to match. A default personality is
invisible to this technique, and `userspace/at` exists. That is a known blind
spot, written down here because the gate cannot report it.

### Why this is now pinned rather than left as a printout

`scripts/multicall-aliases.py` has reported these for months and **exited 0**,
so nothing ever had to act on them. That is how `userspace/udisks`'s `umount`
personality survived: it printed `would unmount` beside `userspace/mount`,
which unmounts for real, and which of the two a user got was decided by
whichever binary the rootfs installed at `/bin/umount`. It was found by hand on
2026-09-10 while reading the crate for an unrelated reason — not by the gate
that was reporting it.

Shadowing is a ratchet now, in `scripts/multicall-shadowed-baseline.txt`, so a
**seventh** cannot appear silently. These six are pinned as known.

### What the fix is, per case

Delete the shadowing branch: the name belongs to whichever program performs
the operation, which is what `design-decisions.md` 1019 says and what
`4182acf8d` did for `login`/`loginmgr`.

**`head:tail` is resolved, and reading is what resolved it.** The pair looked
like a refactor: `Tool::Tail` threads through fifteen sites in
`userspace/head`, so removing the shadowed name meant rewriting the crate —
which has **zero tests** in 1,115 lines. Then the obvious question: coreutils
provides `tail`, but does it provide `head`?

| | lines | tests |
|---|---|---|
| `coreutils/src/bin/head.rs` | 1,284 | 22 |
| `coreutils/src/bin/tail.rs` | 2,471 | 31 |
| `userspace/head` | 1,115 | **0** |

Both, both larger, both tested. So it was never a refactor: the crate
duplicated two commands that coreutils implements more completely, and it is
deleted. Every option `userspace/head` accepted is accepted by one of the two —
checked against **both** binaries together, because comparing against
`coreutils/head` alone showed nine "missing" flags that are all tail's
(`--follow`, `--pid`, `--sleep-interval`).

**But read before deleting.** "Shadowed" means the branch is unreachable, not
that it is worse. If the shadowing implementation is the better one, the fix is
to make *it* the producer and delete the other — `head`'s `tail` and
`sysstat`'s `iostat` are the two most likely to be worth that comparison,
because in both cases the pair was probably written together and the standalone
crate may be the thinner of the two. Deleting by verdict without reading is the
mistake `cal` and `earlyoom` nearly suffered under 1006.

None is urgent: every one of the six names has a working producer today, so no
command is missing. What is at risk is the pair silently disagreeing.
