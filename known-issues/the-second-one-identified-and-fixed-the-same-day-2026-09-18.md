## The second one, identified and FIXED the same day (2026-09-18)

**Identified, with the mechanism.** A `cargo test --workspace` failed on
`gui/desktop`'s `icons::tests::populate_defaults_creates_four_icons` --
`left: 2, right: 4`. The same test passes alone (`1 passed`) and the whole
`desktop` suite passes on its own (`2843 passed`). It is a race on the process
environment.

| | |
|---|---|
| what the test needs | `populate_defaults` adds "Documents" and "Home" **only if `HOME` is set**, so the count is 4 with it and 2 without |
| who moves `HOME` | `settingsfile::testing` -- `config_turn`/`with_scratch_config` point `HOME` and `XDG_CONFIG_HOME` at a scratch directory and restore them in `Drop` with `set_var`/`remove_var` |
| why `desktop` is exposed | it depends on `settingsfile` with `features = ["testing"]`, and `gui/desktop/src/idle_lock.rs` calls `with_scratch_config` in four tests |
| why the lock does not help | `ENV_LOCK` serialises the tests that **take** it. `populate_defaults_creates_four_icons` does not take it, so it reads `HOME` while another thread is writing it |

**This is not the flake recorded above.** That one was a 424-test crate --
`apps/explorer`'s size at the time -- and its log was deleted before it was
read. This is a different test in a different crate, and the only reason it
could be diagnosed is that **the log was still on disk**: `build/wsV.log` held
the assertion, the counts and the crate. The rule that produced that
difference is the one in this file already -- *delete a log only after reading
a PASS* -- and it is worth the disk.

**The fix is on the test, not the code.** `populate_defaults` reading `HOME` is
correct: a desktop with a home directory should offer it. The test has to
serialise against the writers, which means taking the same lock --
`settingsfile::testing::config_turn()` for the duration, or running the body
inside `with_scratch_config`, which also makes the expected count independent
of whether the developer's machine has `HOME` at all.

**Fixed.** `icons.rs` now takes `settingsfile::testing::config_turn()` at the
four places that reach `populate_defaults` -- three tests and, more
importantly, the `populated()` helper, which **eight further tests call**. The
guard only has to span the call, because the environment is read once while
the icons are built and the assertions afterwards read the built list.

**A green run does not prove a race is fixed**, and this one is not being
claimed on that basis: 2843 tests passed afterwards, but they passed before
too, four times tonight. The argument is structural -- the reader now takes the
same lock as the writers, so the two cannot overlap.

**This class already had an entry and a remedy.**
`TD-C-A-TEST-LOCK-SERIALISES-WRITERS-AGAINST-EACH-OTHER-BUT-NOT-AGAINST-READERS`
records it, `config_turn()` was built for it, and 23 of 27 `oswindow` tests
were converted. `gui/desktop/src/icons.rs` was missed -- and the crate's own
`session/tests.rs` uses `config_turn()` in four places, so the idiom was
already in the building. **A remedy applied where the failure was seen does
not reach the places it was not**, which is the same shape as the node/edge
property row earlier today: a fix looks complete from the diff.
