## TD-B-FIVE-CRATES-CANNOT-BE-REACHED-BY-THEIR-DIRECTORY-NAME (lane B, 2026-09-10) -- OPEN: three still reach another crate in silence

**In short:** `cargo test -p <name>` takes a *package* name. Everyone types the
*directory* name, because for 2944 of this workspace's 2955 crates they are the
same string. For ten they are not, and for **three** of those the directory
name belongs to a
different crate -- so the command compiles, runs a test suite, prints a green
result and exits 0, having tested a crate nobody touched.

| Directory | Its package | `-p <directory>` actually reaches | Lane |
|---|---|---|---|
| `apps/backup` | `backup-app` | the `backup` crate | C |
| `apps/indexer` | `indexer-app` | the `indexer` crate | C |
| `apps/sysinfo` | `sysinfo-app` | the `sysinfo` crate | C |
| ~~`apps/tmux`~~ | `tmux-app` | **nothing — there is no `tmux` package**, so `-p tmux` errors | C |
| ~~`userspace/login`~~ | ~~`login-cli`~~ | ~~`init/loginmgr`~~ | B -- **fixed 2026-09-10**, see below |

**Re-triaged 2026-09-14 — the marker said this was closed and it is not.**

The heading used to end `-- four left; lane B's is fixed`.
`scripts/check-known-issues-index.py` slices the slug off a heading and
word-matches the tail against `fixed|resolved|withdrawn|closed|done`, so
*"lane B's is fixed"* made `is_closed()` answer `True` — and every count that
greps this file read the whole entry as done while three crates were still
live. Reported by lane A (`requests/a-b-triage-reads-an-open-entry-as-closed.md`)
and lane C, who between them ran the function against the real heading rather
than reading it. One notice, two messengers, not two findings.

The wording was mine, and the failure is worth naming: a marker that describes
*part* of an entry ("lane B's is fixed") sits in the field a tool reads as the
status of *all* of it. A per-row status belongs in the row.

**Three, not four, and not five.** Verified here with `cargo metadata` rather
than adopted from the report:

* `backup`, `indexer` and `sysinfo` all exist as packages under `userspace/`,
  so `-p <name>` from `apps/` silently builds the other crate. Live.
* **`apps/tmux` is no longer one of them.** There is no `tmux` package anywhere
  in the workspace, so `-p tmux` *errors* instead of building the wrong thing.
  The row above claimed it reached "the `tmux` crate"; that stopped being true
  and nobody noticed, which is the same class of staleness as the marker.
* `userspace/login` is genuinely fixed — `login` now resolves to
  `userspace/login/Cargo.toml`, checked rather than assumed, because it was my
  own claim.

**The slug still says FIVE on purpose.** Lane A left it alone after two lanes
had already pushed references to it, on the reasoning that a stable wrong
identifier beats a correct one that breaks citations. That is right, and it is
why the heading now disagrees with itself: the slug is an address, the marker
is the status.

**How it was found.** Giving `userspace/login` its exec on 2026-09-10. Every
`cargo test -p login` that tick, and a `cargo fmt -p login`, went to
`init/loginmgr` -- a different program, never edited. It surfaced only because the
test count did not move after three tests were added: 53 `#[test]` in the file,
46 collected, and the 46 collected names turned out not to be in the file at
all. Nothing else would have said a word.

**The asymmetry that makes this worth a gate.** A mistyped `-p` that matches no
package fails loudly and immediately. The dangerous case is the one that
*resolves*, to somebody else. Seven other crates in the tree have a package name
that differs from their directory -- `gui/toolkit` is `guitk`,
`toolchain/stubs` is `slateos-stubs` -- and they are harmless for exactly this
reason: nothing claims `toolkit` or `stubs`.

**Gate 18 (`scripts/check-crate-names.py`) stops it growing**, and only that:
the three live collisions above are baselined, because all three are lane C's
to rename and a gate
that refuses every lane's push over pre-existing state is a gate that gets
bypassed. The baseline may only shrink -- resolving one and leaving it listed
is also a failure, so the list cannot rot into things that used to be true.

**Correction, 2026-09-14 (lane A).** The `apps/tmux` row was wrong: there is no
package named `tmux` anywhere in the tree -- `apps/tmux` is the only claimant of
that string and it is named `tmux-app` -- so `-p tmux` does not reach another
crate, it **errors**, which is the harmless case this entry itself describes two
paragraphs down. The row asserted a collision that cannot occur, and the counts
built on it ("five", "four are lane C's") were wrong with it. Live collisions are
**three**: `backup`, `indexer`, `sysinfo`. `tmux` belongs with `gui/font`,
`gui/remote`, `gui/toolkit`, `gui/vulkan`, `gui/window` and `toolchain/stubs` --
seven crates whose name differs harmlessly.

Two independent witnesses, per 932: lane C ran `cargo pkgid` across all of them,
and lane A parsed every `Cargo.toml` in the tree directly. Both give the same
three. `scripts/check-crate-names.py`'s baseline is also exactly those three and
has been since 2026-09-04 -- and because that gate flags a *stale* baseline entry
as a failure too, tmux could never have been in it. The gate was right and this
prose drifted from it.

**The census figures in the opening paragraph (2944 of 2955) are the gate's own,
over a wider walk than the audit above, and are NOT re-verified here.** A direct
parse of the tree finds 417 crates with a `Cargo.toml`, of which ten differ from
their directory. The two numbers are not reconciled; do not treat 2955 as
confirmed. Stated rather than harmonised, because quietly adjusting a number to
agree with a different measurement is how the tmux row got here.

**The heading used to classify as CLOSED while three crates were live; lane B
fixed it on 2026-09-14.** `check-known-issues-index.py`'s `is_closed()` slices
the slug off and word-matches the tail, and the old tail read "lane B's is
fixed" -- so every triage count read the whole entry as done. Lane A found it
and deliberately did not reword it, because that flips the open/closed status of
two other lanes' crates and the words were lane B's claim about lane B's own
fix; it went out as a request plus a direct notice instead. The tail is now
"OPEN: three still reach another crate in silence", which carries no marker
word, and `is_closed()` returns false for it -- verified against the module's
own function, with a positive control first to show it can still return true.

**What this entry still does not cover, and it is the part that actually bit.**
The gate refuses an *unrecorded* mismatch. It does not, and cannot, stop anyone
typing `cargo test -p sysinfo` at a prompt -- which is how lane C lost an
afternoon on 2026-09-14, reading "26 tests ok" from `userspace/sysinfo` while the
test it had just written in `apps/sysinfo` had never been compiled. The hazard was
recorded here, with a gate, ten days earlier. The record was not found. That is a
findability failure in a 144,000-line document, not a documentation gap, and no
further gate fixes it -- the durable fix is the rename, which is lanes B and C's.

**The fix, for whoever owns each crate:** rename the package to match its
directory, or rename the directory to match the package.

**Lane B's is done, 2026-09-10, and it was not cosmetic.** `init/loginmgr` is the
graphical Display Manager; `userspace/login` is the console `login(1)`. Neither
declared a `[[bin]]`, so the binary named `login` was **the Display Manager's**,
while the console program built as `login-cli` -- and `userspace/getty` execs
`/bin/login`. Nothing populates a rootfs yet, so nothing had failed; the first
person to write that manifest would have installed the display manager where
getty looks for the console login program, and the symptom would have been a
graphical login trying to start on a serial console.

So the console program took the name it should always have had: `userspace/login`
is now the package `login`, and the manager is `loginmgr` in `init/loginmgr`.
The blast radius was smaller than this entry originally guessed -- both are leaf
binaries and nothing depends on either by path, so it was two `name =` lines and
a `git mv`. "Touches every Cargo.toml that depends on it" was an assumption; the
grep that would have checked it takes a second.

**The four remaining are lane C's** (`apps/backup`, `apps/indexer`,
`apps/sysinfo`, `apps/tmux`), and each has the same second question worth
asking: which crate currently owns the *binary* name, and is it the one that
should?
