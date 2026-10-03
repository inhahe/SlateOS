## TD-B-USERSPACE-CRATES-DO-NOT-INHERIT-THE-WORKSPACE-LINTS (lane B, 2026-08-21)

**In short:** CLAUDE.md requires every crate to run a set of "defensive" compiler
warnings — the ones that point at code which can crash on bad input (an array
read past its end, an addition that overflows, an `unwrap` on something that
might be missing). Almost no crate under `userspace/` actually turns them on:
**2730 of 2762** lane-B crates never opted in. The warnings are configured once
at the top of the repository and each crate has to say one line to inherit them;
32 crates say it. Nothing is broken today, but the checks that are supposed to
catch a whole class of crash-on-bad-input bugs are simply not running over
almost any of this tree.

**How it was found.** Three of the crates that decide *whether a password is
accepted* were in the missing set: `doas` (grants privilege elevation), `logind`
(unlocks a locked screen) and `ftpd` (authenticates a network client, and parses
attacker-supplied protocol commands to do it). `doas` and `logind` had no clippy
configuration of any kind — neither `[lints] workspace = true` in `Cargo.toml`
nor a `#![deny]`/`#![warn]` in the source. `ftpd` had a bare
`#![deny(clippy::all)]`, which is the default group and excludes every lint
named in CLAUDE.md. Turning inheritance on in those three surfaced **71
production sites**: 41 in `doas`, 25 in `ftpd`, 5 in `logind`. All 71 are now
fixed (see the commit that adds this entry); the survey that followed is what
found the other 2727 crates.

**Scope, measured rather than estimated.**

| | Count |
|---|---|
| Lane-B crates (`userspace/`, `services/`, `init/`, `posix`) | 2762 |
| …that inherit the workspace lints | 32 |
| …that do not | **2730** |
| of those, `-cli` wrapper crates (~124 lines each) | 2226 |
| of those, full utilities | ~504 |

The non-inheriting crates are not uniformly unchecked: some carry a bare
`#![deny(clippy::all)]` of their own (e.g. `age`, `ab`), some carry nothing at
all (e.g. `acl`, `acpi`). Neither case gets `pedantic` or the four defensive
lints, so the distinction does not affect the exposure.

`userspace/*` is already a workspace `members` glob and `[workspace.lints]` is
already defined at the root, so this is a **one-line opt-in per crate**, not a
configuration design problem. The cost is entirely in the warnings it uncovers.

**What the sweep will cost.** Measured on four full utilities (`acl`, `acpi`,
`age`, `ab`): 87 warnings, ~22 per crate. The breakdown matters more than the
total, because only part of it is what CLAUDE.md is actually protecting:

| Kind | Count | Is it a real defect risk |
|---|---|---|
| `arithmetic_side_effects` | 20 | **Yes** — an overflow on attacker-influenced input |
| `indexing_slicing` | 18 | **Yes** — a panic on a short slice |
| `map(..).unwrap_or(..)`, redundant closure, inline format args, needless pass-by-value, precision-losing casts, … | 49 | Style; mechanical, many auto-fixable with `clippy --fix` |

So roughly **44% of the warnings are the defensive ones** and the rest are
style. Extrapolating ~22/crate over ~504 full utilities is on the order of
**11,000 warnings**, of which ~4,800 are defensive; the 2226 `-cli` wrappers are
about a sixth the size each and will add a smaller tail.

**The proper fix, and why it should be staged by trust boundary rather than
alphabetically.** Add `[lints] workspace = true` to each crate and fix what it
finds. A single 2730-crate commit is the wrong shape: it would either bury ~4800
genuine findings under ~6200 style ones, or tempt a blanket `#![allow]` that
turns the whole exercise into a no-op. Order the sweep by what a bug in the
crate can reach:

1. **Authentication and privilege** — done: `doas`, `ftpd`, `logind` (and
   `authlib`/`sshd`, which already inherited).
2. **Network-facing daemons** — anything that parses bytes off a socket before
   it knows who sent them.
3. **setuid/privileged helpers and anything that writes to `/etc`.**
4. **Everything else,** where `clippy --fix` handles most of the style half and
   the defensive half can be reviewed in batches.

Each stage is its own commit with its own green test run, the way the three auth
crates were.

**Addendum 2026-08-22: `userspace/coreutils` is in the non-inheriting set, and
it should be promoted out of stage 4.** It is one crate by the count above, but
it is **86 binaries** — `wc`, `sed`, `sort`, `awk`, `tr`, `head`, `cut` and the
rest — which is nearly every command a shell script runs, and every one of them
parses attacker-shaped input in the ordinary course of its job (a file's bytes,
a regular expression, a `--width` operand). Its `Cargo.toml` has no `[lints]`
section and none of its bins carries a crate-level `#![deny]`, so the whole of
it is unchecked. Weighed by "what a bug in the crate can reach", it belongs
between stages 2 and 3, not at the end.

It is also the one place where the cost is bounded by something better than an
extrapolation: 27 of its utilities are already under differential harnesses
(`scripts/*-diff.sh`), so a lint fix that changes behaviour is caught the same
run rather than at the next boot test. Sequence it **after**
`B-FORTY-TWO-BINARY-NAMES-ARE-BUILT-BY-TWO-PACKAGES` is resolved, not before:
whichever way that goes, utilities move between crates, they arrive carrying
their own warnings, and linting the same code twice is the avoidable half of
the work. Note that if `open-questions.md` → B-Q7 is answered in favour of the
standing §8 — standalone crates canonical, `coreutils/src/bin/*` retired — then
this crate's 86 binaries do not need linting at all; they need porting into 45
new crates that inherit the workspace lints by construction. That is a further
reason not to start here until B-Q7 lands. **B-Q7 landed on 2026-09-07** (§1005: coreutils is the one home), so that reason has expired — left in place rather than deleted because the paragraph above it is still the right way to think about the work, and only its last clause went stale.

**Addendum 2026-09-15: the scope above was measured through a gate that
under-reported it, and the corrected numbers are three times larger.**

`check-workspace-lints.py` accepted a bare `#![deny(clippy::all)]` as coverage
and left such crates out of its report entirely. This entry already said why
that is wrong -- *"`ftpd` had a bare `#![deny(clippy::all)]`, which is the
default group and excludes every lint named in CLAUDE.md"*, and *"Neither case
gets `pedantic` or the four defensive lints, so the distinction does not affect
the exposure"* -- but the gate was written the other way, and the gate is what
anybody reads. Corrected in `213341d27`; the baseline went 89 -> 165.

Measured under `userspace/`, `services/` and `init/` on the day of the fix:

| | crates | lines |
|---|---|---|
| inherit `[workspace.lints]` | 56 | 129,654 |
| bare `#![deny(clippy::all)]` only | **76** | **460,878** |
| neither (what the gate reported) | 89 | 148,857 |

So **62% of lane-B source under those roots** was outside the policy and
outside the report. The gate was not merely undercounting: adding that one
attribute to a listed crate removed it from the list while changing nothing
about which lints run, so the ratchet could be satisfied by a no-op.

**`userspace/coreutils` is the largest thing this hid** -- 83 binaries,
197,806 lines, carrying only the bare attribute. The 2026-08-22 addendum above
argued it should be promoted between stages 2 and 3, on the grounds that it is
nearly every command a shell script runs and every one of them parses
attacker-shaped input in the ordinary course of its job. That argument stands,
and for a month the gate could not see the crate it was about.

**Stage 2 is done, and its membership changed under this correction.** Network
crates that parse socket bytes and are reachable by `cargo test -p`: `ntpd`
(57 findings, fixed), `tcpdump` (133, fixed), `dhcpcd` (123, fixed). Not
stage 2, checked rather than assumed: `ping` and `traceroute` open no socket
and carry refusal markers, `nslookup` reads `/etc/resolv.conf`, `ss` only
lists sockets, and `kill`/`pgrep`/`powerctl`/`service` use kernel IPC channels
rather than network ones. `services/netstack` and `services/udpget` are not
workspace members -- they target `x86_64-unknown-none`, `cargo test -p` cannot
reach them, and their protocol logic is tested in the crates they delegate to
(netproto 79 tests, netipc 41, netring 9).

`dhcpcd` was only found because of this correction: it was in the invisible
middle set, I read its absence from the baseline as coverage, and it was
hiding an overflow that an RFC-conforming DHCP server triggers.

**If never fixed:** no regression — the exposure is exactly what it has been
since the crates were written. But the lints exist because this codebase has no
human reviewer, and a check that runs over 32 of 2762 crates is not the safety
net CLAUDE.md describes. Every crate added under `userspace/` without the line
makes the ratio worse, so the honest read is that this gets slowly worse rather
than staying flat.
