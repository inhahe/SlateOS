## TD-B-COREUTILS-HAS-913-DEFENSIVE-LINT-FINDINGS — 2026-09-15 — FIXED 2026-10-02

**Fixed:** `cargo clippy -p coreutils --all-targets` reports nothing, on
clippy 1.98 for Linux and 1.95 for the Windows host -- production code and
test code, every bin and the library. The last of it went in six commits:
the library (`43a13392a`), `cal` (`db3c91e5c`), `diff` (`9fab86076`, and the
port that replaced it since), `dd` (`daa2341a5`), the other 31 programs that
still had findings (`bc66f224d`), and the test code, which a run without
`--tests` never compiles (`8bdf5d2a9`: 120 findings, 89 of them the
restriction lints test modules are meant to allow). The arithmetic went by
`checked_*` and `saturating_*` rewrites and, where the indexing and the
arithmetic are the algorithm's own, by function-level allows that give their
reason (`diff`'s `analyze.rs`, for one). The warning below about defaulted
values still stands for whoever touches these sites next.

What follows is the entry as it was filed.

**In short:** the 83 commands in `userspace/coreutils` are now checked by the
warnings that point at code which can crash on bad input. They report 913
places worth looking at. Nothing is broken that was not broken yesterday --
the checks were simply not running over this crate until today, and now they
are, so the list is visible instead of hypothetical.

**Why it was invisible.** `userspace/coreutils` carried a bare
`#![deny(clippy::all)]` and no `[lints]` table. `clippy::all` is the default
group: it excludes `pedantic` and all five of CLAUDE.md's defensive lints. The
gate accepted that attribute as coverage until `213341d27`, so the largest
crate in lane B -- 83 binaries, 197,806 lines -- was absent from the very
report that exists to find it. See
`TD-B-USERSPACE-CRATES-DO-NOT-INHERIT-THE-WORKSPACE-LINTS`.

**Enabling it surfaced exactly ONE deny-level error, and my first
measurement missed it.** I ran `cargo clippy -p coreutils --all-targets
--target x86_64-pc-windows-gnu`, got zero errors, and wrote that enabling
"cannot break a build". The `coreutils-unix-half` pre-push gate then refused
the push: `src/bin/date.rs:666` trips `clippy::question_mark`, which is
deny-level through `clippy::all`.

The gate's own diagnostic explains such differences as `cfg(unix)` code that a
Windows-target build never type-checks -- but that is NOT what happened here.
`date.rs` contains no `cfg(unix)` at all and the site is in plain
`parse_date_spec`. The two builds run different clippies: mine cites
`rust-clippy/rust-1.95.0`, the gate's cites `rust-clippy/main`. So "clippy
clean" is a claim about a toolchain AND a target, and a measurement on one
pair does not transfer to the other.

Fixed in the same commit (the `if let ... else { return None }` became `?`,
which is what the lint asks for and is simpler). The unix half then reports
`clean (linux half checked)`.

Every other finding is `warn`-level, joining the ~18,000 the workspace already
carries by deliberate policy (see the clippy gate's comment in
`scripts/boot-test.sh`). So the crate is subject to the policy from today, new
code in it is checked from today, and the backlog is worked down after --
rather than the crate staying outside the policy until someone finds a week.

**Where the work is. 913 production findings across 48 targets**, of which
756 sit in six of them. A further **276 are `#[cfg(test)]`-only** and want the
allow list CLAUDE.md prescribes for test modules, not fixes.

| Target | Production findings | Character |
|---|---|---|
| `lib` | 254 | mostly `extfloat.rs` and `bignat.rs` |
| `bin "diff"` | 181 | index/offset arithmetic over two files |
| `bin "cal"` | 148 | date arithmetic |
| `bin "dd"` | 88 | block counts and seek offsets |
| `bin "od"` | 50 | offsets into a byte dump |
| `bin "df"` | 35 | size and percentage arithmetic |
| everything else | 157 | across 42 targets, 1-16 each |

**The first count of this was wrong and the correction is instructive.** I
read the per-file totals out of clippy's output and got 1,306, because a
finding inside a `#[cfg(test)]` module is reported against the file like any
other -- and cargo lints each crate TWICE, once as `bin "x"` and once as
`bin "x" test`, the second including the test module. `src/getopt.rs` is the
clean example: all 25 of its findings are in its test module, and its
production code has none. Counting by TARGET rather than by file, and dropping
the `... test` twins, gives 913. The lesson generalises past this entry: in
clippy output a file is not a unit of anything.

**`extfloat.rs` and `bignat.rs` need a different answer from the rest, and it
should be argued rather than assumed.** They implement arbitrary-precision
arithmetic; `arithmetic_side_effects` firing on a bignum kernel is not the
same finding as it firing on a packet parser, because deliberate wrapping and
carry propagation is what the code is FOR. A narrow, module-level allow with a
specific justification may be right there. That is a different thing from the
crate-wide allow removed in `213341d27`, whose justification was "every site is
gated" -- a claim nobody could check and which turned out to be false. Any
allow added here must name what makes the module safe and be checkable against
the module's own tests.

**Do not fix these by making them `.unwrap_or(0)`.** A saturating or defaulted
value in a command that reports a number to a script is how `wc` starts
printing a plausible wrong count. The sites want reading, not a sweep.

**Where it lives:** `userspace/coreutils/`, everything under `src/`.
