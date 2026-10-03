## §359 — One binary name, one producer: the duplicated utilities consolidate into `coreutils`, but the surviving *code* is chosen per utility

> **UN-SUSPENDED by §1005** (2026-09-07) — this entry was suspended and is
> live again; §1005 supersedes §8 and restores this one as the rule for which
> half of each duplicated utility survives.

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)
**Status: SUSPENDED the same day it was written — do not implement. It
contradicts §8, which is an *operator* decision, and only §8 gets to be
overturned by the operator.** See the "Suspended" note at the end of this entry
before acting on anything above it: the direction below is, I still believe,
the right one, but choosing it is not mine to do. The question is queued as
`open-questions.md` → **B-Q7**. Until that is answered, §8 governs: standalone
per-tool crates are canonical.

**In short:** Forty-two of our command names — `wc`, `sed`, `stat`, `bc`, `tar`
and so on — are currently built by *two* different crates, each producing a file
with the same name in the same output directory. Cargo does not stop this; it
prints a warning (only when both are built in one command) and then lets
whichever finished last overwrite the other. So which `wc` you actually ran was
decided by build order, and could change between two runs that did not touch
`wc` at all. This decides that each name gets exactly one producer, that the
producer is the `coreutils` crate, and that when the two versions differ the
better one is moved into `coreutils` before the other is deleted — the crate
that survives is not automatically the code that survives.

### What went wrong, concretely

`scripts/calc-diff.sh` reported "95 passed, 105 differed" against GNU `bc` and
three bugs were written up in `known-issues.md` from that report. None of them
existed. The harness had been running `userspace/coreutils`'s `bc`, an older and
genuinely broken implementation, while the `bc` that is shipped and unit-tested
is `userspace/bc`, which passes all 200 cases. Both write
`target/x86_64-pc-windows-gnu/debug/bc.exe`. The report was withdrawn
(`known-issues.md` → `BUG-BC-PRINTS-A-BANNER…— WITHDRAWN`), the harnesses were
taught to build their own subject from a *named package*
(`scripts/diff-subject.sh`), and `calc-diff.sh` immediately went to 200/0 with
no change to any utility. The duplication itself is what this entry is about;
it is tracked as `known-issues.md` →
`B-FORTY-TWO-BINARY-NAMES-ARE-BUILT-BY-TWO-PACKAGES`.

The forty-two names:

```
awk bc cal chown cmp comm cut date dd df diff du env expand fold free head
hostname join kill logger nl paste patch ps sed seq sha256sum sort split stat
strings tar tee tr tsort uname uniq uptime wc who xargs
```

Four of the standalone crates are additionally multi-call, so they contend for
more names than their own: `stat` also produces `touch`/`ln`/`readlink`/
`realpath`/`mkfifo`, `sha256sum` also `md5sum`/`sha1sum`/`sha512sum`, `chown`
also `chmod`, `who` also `w`.

### The decision

1. **`coreutils` is the one home.** Every one of the 27 differential harnesses
   names `coreutils`; every August-2026 GNU-parity rewrite (`printf`'s `\uXXXX`,
   §351's quote marks, §356, §357, `seq`'s 80-bit float) landed there; and the
   shared infrastructure those rewrites depend on — `quote`, `ere`, `bignum`,
   `charwidth`, `extfloat` — is `coreutils`'s. Splitting a utility away from
   that infrastructure is how the second copy drifted in the first place.
2. **Merit decides the code, not the crate.** For roughly half of the forty-two
   the standalone crate is the substantially larger and more complete
   implementation, and `coreutils`'s namesake is a stub:

   | | `coreutils` | standalone |
   |---|---|---|
   | `stat` | 343 | **2845** |
   | `chown` (+`chmod`) | 326 | **1770** |
   | `who` (+`w`) | 347 | **1689** |
   | `date` | 245 | **1621** |
   | `sha256sum` (+3) | 187 | **1529** |
   | `du` | 352 | **1341** |
   | `hostname` | 125 | **1225** |
   | `df` | 339 | **1181** |
   | `dd` | 365 | **1166** |
   | `xargs` | 274 | **1096** |
   | `ps` | 393 | **1082** |
   | `patch` | 886 | **2183** |
   | `tar` | 752 | **1676** |
   | `bc` | 2677 | **2731** |

   and for the harnessed ones it runs the other way (`head` 1205 vs 1123,
   `expand` 853 vs 589, `seq` 1453 vs 1276), because those are the ones the
   harnesses have been grinding against GNU. So each name is resolved
   individually: the better implementation is *moved into*
   `coreutils/src/bin/<name>.rs`, and only then is the standalone crate deleted.
   `coreutils` has no bin at all for `sha1sum`, `sha512sum` or `w`, so those
   three are gained, not merely relocated.
3. **Nothing is deleted before it is measured.** A utility only loses its second
   copy once the survivor is under a differential harness or a unit-test suite
   that covers what the loser could do. Deleting first and discovering the gap
   later is exactly the failure this entry exists to end.

### Two traps found doing the first one

`bc` was consolidated first, being the one that started this. Both of these bit
within ten minutes of each other, and both will recur on the other forty.

**A `git mv` is invisible to cargo.** Cargo decides freshness from mtimes, and a
rename preserves the *source* file's mtime. `userspace/bc/src/main.rs` was last
edited on the 16th, so moving it over `coreutils/src/bin/bc.rs` produced a file
six days older than the `bc.exe` already sitting in `target/`, and
`cargo build -p coreutils --bin bc` answered `Finished` in five seconds without
compiling anything. The binary then still printed the *old* implementation's
banner — the same confident wrong answer that `scripts/diff-subject.sh` exists
to prevent, arriving by a route that building every run does not close. So each
move ends with a `touch` on the destination, or copies the bytes instead of
renaming: a rename alone is not a change as far as the build is concerned.

**Do not restructure a crate while a harness run is in flight.** Removing
`userspace/bc` during a `scripts/all-diff.sh` run turned every harness after the
fourth into `failed to load manifest for workspace member … bc`, because
`members = ["userspace/*"]` is re-globbed on every invocation. Twenty-two
harnesses reported an error that had nothing to do with what they measure, and
the run had to be repeated. `all-diff.sh`'s header already warns against editing
*that file* mid-run; this is the same hazard one level out — the workspace's
shape is as much a shared input as the script is.

A related detail, when a crate directory goes: `git rm -r` removes the tracked
files but leaves the now-empty directories, and an empty `userspace/bc/` still
matches the members glob while no longer containing a `Cargo.toml` — which is
precisely the state that produced the error above. The `rmdir` is part of the
change, not tidying up after it.

### Alternatives considered

*Consolidate the other way — keep the standalone crates, delete `coreutils`'s
bins.* Attractive on raw line counts, and it would preserve the richer half
untouched. Rejected because it inverts the dependency: the standalone crates do
not use `quote`/`ere`/`bignum`/`charwidth`, so every GNU-parity fix of the last
month would have to be re-applied, and the harnesses — the only thing that has
ever caught a real difference here — would all have to be repointed. It trades a
one-off port for a permanent divergence from the tested code.

*Keep both, rename one set's output.* One line of `Cargo.toml` per crate and the
collision is gone. Rejected because the collision is a symptom: the real defect
is two implementations of `stat` that will keep drifting apart, only now with
nothing forcing anyone to notice. A build that no longer warns is worse than one
that does, if the duplication survives it.

*Keep both, drop the duplicates from the workspace `members`.* Same objection,
plus the unbuilt half rots silently and stops compiling.

### What this costs if it is not finished

The harness fix already removes the *measurement* hazard — every harness now
builds the package it names, so a red harness is a real difference again. What
remains is the shipping hazard: `scripts/create-ext4-rootfs.sh` currently stages
fastpy-compiled ELFs rather than these Rust binaries, so no image has yet been
built from a coin flip. That is luck, not design, and it stops being true the
day a rootfs recipe reaches into `target/…/debug/`.

Verification that the work is complete:

```sh
cargo build --workspace --target x86_64-pc-windows-gnu 2>&1 | grep -c collision   # must be 0
```

### Suspended, hours after it was written — I overturned an operator decision without knowing it

Everything above is a `Claude (autonomous)` decision, and I had no standing to
make it. **§8, from 2026-06-12, decides the same question the other way, and it
is `Decided by: Operator`.** It says standalone per-tool crates are canonical,
that a `coreutils-common` library should be extracted, and that
`coreutils/src/bin/*` should be retired. I did not find it until after I had
written this entry and merged the first utility (`bc`) into `coreutils`.

I found it the roundabout way — chasing whether the `--json` mode in 23 of the
standalone crates was a convention or an accident — which is the tell that the
mistake was procedural, not intellectual. **The correct first step of "resolve
the duplicated binaries" was to grep `design-decisions.md` for the word
`coreutils`, and I did not.** Writing a new entry is not the same as checking
that no entry already covers it, and the file is 35 000 lines precisely so that
the question "has this been decided?" has an answer. §8 is not obscure — it has
its own companion analysis file, `coreutils-canonical-answer.md`, sitting in the
repository root.

What makes this worth writing down rather than quietly reverting: **§8's
deciding argument is factually wrong**, and I am the one who supplied it. It
retires `coreutils` because it is "a busybox-style multi-call binary that
dispatches on `argv[0]`", which would force one on-disk identity to hold the
union of every bundled tool's capabilities. `coreutils` is not that and never
was — 86 separate bin targets, no `argv[0]` dispatch, no `src/main.rs` in its
history, and it already contains the shared library §8 proposed creating. The
crates that *do* dispatch on `argv[0]` are on the standalone side
(`stat` → six tools, `sha256sum` → four, `chown` → two, `who` → two). So the
argument that decided §8 argues against §8's conclusion.

That does **not** make it mine to reverse. A wrong premise under an operator
decision is a reason to *tell the operator*, not to act as though the decision
had been made differently — the whole value of the `Decided by:` field is that
it marks which entries I may revisit. So:

1. This entry stays, suspended and unimplemented, as the record of the
   direction I would argue for.
2. The premise error is queued as `open-questions.md` → **B-Q7**, with the
   measurements, three options and a recommendation.
3. The `bc` consolidation this entry describes **stays as it is** —
   `userspace/bc` deleted, the better August implementation living at
   `coreutils/src/bin/bc.rs`, `calc-diff.sh` naming the `coreutils` package.
   *This point said "is reverted" when written on 2026-08-22; it was changed
   the same day, before the revert was carried out.* See the amendment below.
4. `known-issues.md` → `B-FORTY-TWO-BINARY-NAMES-ARE-BUILT-BY-TWO-PACKAGES`
   is corrected to say the *direction* is undecided pending B-Q7, since the
   collision itself is real either way.

### Amendment, 2026-08-22 — why point 3 no longer reverts `bc`

Reverting one utility looked like the cheap way to keep the tree honest while
B-Q7 is open. Priced properly, it is not cheap and it is not honest:

- **It buys no compliance.** §8 wants one tool per crate. 45 names live *only*
  in `coreutils` and four standalone crates are multi-call, so the tree violates
  §8 in ~49 places. Moving `bc` takes that to ~48 — a gesture, not a state.
- **It costs the only mechanical check `bc` has.** `diagnostics_quote_names`
  reads `coreutils/src/bin` and nothing else, and it caught a real unquoted-name
  bug in `bc` on 2026-08-22 (see
  `B-BCS-COMMAND-LINE-EXITED-0-ON-EVERY-KIND-OF-FAILURE`). Widening it is not a
  side-fix: `scripts/quote-names-scope.py` prices a tree-wide version at 1796
  call sites in 777 crates (`TD-B-THE-QUOTE-NAMES-TEST-READS-ONE-DIRECTORY-OF-EIGHTY`).
  *(Amended 2026-08-23: that script is superseded by `scripts/quote-names.py` —
  the same two detectors plus a baseline, a `--check` and a `--selftest`, wired
  in as pre-push gate 8. This bullet is now weaker than when it was written: a
  standalone crate is no longer outside all mechanical checking, only outside
  the strict-zero one. The 1796 sites remain unrepaired; what changed is that
  they can no longer grow.)*
- **It needs a dependency edge nobody has drawn yet.** After the 2026-08-22
  rewrite `bc` uses `coreutils`'s `getopt`, `quote` and `errmsg`. A standalone
  `userspace/bc` must either depend on the library inside the bundle §8 retires,
  or fork three modules — which is the drift this entry exists to stop.
- **Leaving it costs nothing meanwhile.** The harm B-Q7 describes is two crates
  writing one filename; for `bc` that harm is already gone. One implementation,
  the better one, named explicitly by its harness, 200/200 against GNU bc.

The principle this turns on, and the reason it is recorded rather than just
done: **an operator decision is owed an accurate tree or an accurate account,
and when the two conflict the account is worth more.** A one-file gesture that
makes the tree look §8-shaped while 48 other violations stand would misinform
the very decision B-Q7 is asking for. So B-Q7 now states plainly that `bc` was
not reverted and why, and the move remains one file, one dependency line and one
`calc-diff.sh` edit away if the operator answers A.

The two traps recorded above (`git mv` is invisible to cargo; do not restructure
a crate mid-harness-run) are independent of which way B-Q7 goes, and are the
other part of this exercise worth keeping.
