## `TD-C-ONE-INTERMITTENT-TEST-FAILURE-IN-THE-WORKSPACE-SUITE` (lane C, 2026-09-17) -- **IDENTIFIED AND FIXED 2026-09-19**

**In short:** a `cargo test --workspace` failed with exactly one failing test,
and the same command on the same tree passed on the next run. Something in the
suite fails occasionally and not reproducibly. It is not fixed, and it is not
even identified, because I deleted the log before reading it.

**Update 2026-09-19 -- it recurred, the log was kept, and it is fixed.**
The test was `apps/explorer`'s
`the_preview_panel_narrows_the_grid_for_the_wheel_too`, reporting `7 -> 7`:
the preview panel did not narrow the icon grid. It passed alone and it passed
with the crate's own 430 tests, which is the signature this entry describes.

`ExplorerState::new` reads the configuration directory -- `preview_open`,
`preview_split`, `preview_side`, the icon labels and the thumbnail size all
come from `settingsfile::load`. Five tests in the same binary *write* the
environment that resolves it, and `the_preview_can_move_to_any_side` writes
`side: bottom` into its scratch config while checking all four sides. Tests
are threads of one process, so a construction on another thread read that
file -- and a preview on the bottom divides the *height*, so the column count
is unchanged and the test asserting it narrows fails while the program is
correct.

Fixed in `03a69f3a1` by taking `settingsfile::testing::config_turn()` across
the constructor in `state_at`, the one place tests build a state. The turn is
held for the constructor only: the values are copied out of the document
there and nothing later re-reads the environment.

**The hypothesis recorded below was right in mechanism and wrong in target.**
It guessed at scratch-directory resolution -- `ScratchDir::new` reading
`TMPDIR` -- and `guarded_scratch` already covers that. The unguarded reader
was the *configuration* load inside the constructor, one level further in. A
correct diagnosis of the class does not locate the instance, and I spent a
first pass misreading `guarded_scratch` as a lock dropped too early before
checking what it actually guards.

**And the rule at the bottom of this entry is what made the fix possible.**
The log was kept this time, the test name was in it, and the whole diagnosis
followed from one line of output.

**What is known.**

| | |
|---|---|
| exit | `CARGO_RC=101`, `child exited: FAIL (exit 101), 226s elapsed` |
| the failing crate's suite | `424 passed; 1 failed` |
| re-run, same tree, no changes | `PASS, 580s`, 61,752 passed, nothing failed |
| the crate | almost certainly `apps/explorer`, which has exactly 425 tests -- and which passes 425/425 when run on its own |

**Why the name is missing, which is the part worth not repeating.** The command
that read the verdict also ran `rm -f build/wsD.log`, unconditionally, in the
same line. It was written for the case where the run passes. The failure
summary was on screen for one moment and the file holding the test name was
gone before I thought to look for it. Same shape as the `open("w")`
truncation recorded above: a destructive step sequenced before the thing that
decides whether it is safe.

**Delete a log only after reading a PASS out of it.**

**The likely cause, stated as a hypothesis and not a finding.** The crate
passes alone and failed under the workspace run, which is the shape this tree
already has a gate for: `scripts/check-config-turn-guards.py` exists because
`settingsfile::testing::with_scratch_config` repoints `XDG_CONFIG_HOME` for
the whole *process*, and `cargo test` runs a crate's tests as threads of one
process -- so a test that drives an event loop can see the directory change
under it and repaint when it counted frames. That gate reports `0 unguarded`
today.

**Correction, checked afterwards: that gate is not about this.** Its
`HARNESS_CALLS` are `testing::desktop()` and `ShellSession::start`, and
`apps/explorer` uses neither -- its tests call `handle_event` directly. So its
`0 unguarded` is not a miss, and calling this "a case the gate does not
recognise" implied a coverage gap that does not exist. explorer was never in
that gate's corpus.

What survives is the mechanism, not the gate. `explorer` calls
`with_scratch_config` 32 times and `config_turn` zero times. `settingsfile`'s
`ENV_LOCK` serialises *writers* of `XDG_CONFIG_HOME` against each other, and
`config_turn` is how a *reader* takes that same lock -- so 32 writers and an
unguarded reader in one crate, whose tests are threads of one process, is a
real race whatever any gate's scope is. A lead, not a diagnosis: none of this
names the failing test, and the failing test is what was lost.

**What to capture when it happens again**, since it will and the next person
should not be starting from here: the whole log, the test name from the
`---- <name> stdout ----` block, and whether `cargo test -p <crate>` alone
reproduces it.
