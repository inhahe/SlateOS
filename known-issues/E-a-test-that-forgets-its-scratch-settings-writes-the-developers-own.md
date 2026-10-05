### [E] A test that forgets its scratch settings writes the developer's own -- 2026-09-25
**Status:** OPEN, but no longer unguarded -- the one leak found that day is fixed, and since then `scripts/check-scratch-config.py` (a pre-push gate) walks each crate's call graph from every settings `save()` and refuses a push whose tests can reach one without a scratch guard; it stopped one of lane E's own pushes on 2026-09-27. What is still open is the runtime guard below, which would catch what a static walk cannot see

**In short:** a test that saves a setting without first taking a scratch
configuration directory writes to the real `~/.config/slateos` of whoever runs
it. On 2026-09-25 the weather app's unit-key tests did exactly that, the first
time pressing U, W, P or T began saving the choice: they left a
`weather.yaml` of Fahrenheit, mph, inHg and 12-hour in the developer's home.
They now run inside `settingsfile::testing::with_scratch_config`, and the file
was deleted. Nothing but care stops the next such test.

**What is still there.** On this Windows host `~/.config/slateos/` holds six
more files that nothing but a test run can have written, all older than the
2026-09-24 baseline run of every app's suite (so no current test writes them):
`explorer.yaml` lists `explorer_test_manual_*` temporary folders as saved
orderings; `appearance.yaml`, `fileassoc.yaml`, `input.yaml`,
`notifications.yaml` and `widgets.yaml` hold what look like defaults. They
were left in place, not being this session's to delete. A test that *reads*
settings without a scratch directory sees them -- an order-dependent result
waiting to happen.

**The proper fix is a guard, in lane C's `gui/settingsfile`:** with its
`testing` feature on -- which only a dev-dependency turns on -- `store` could
refuse outright unless a `with_scratch_config` turn is held, panicking with a
message that names the fix. Every app test that saves would then fail loudly
the first time it forgot, instead of writing somebody's home. Asked of lane C
on 2026-09-26: `requests/e-c-settingsfile-refuses-a-store-outside-a-scratch-config-in-tests.md`
(whether a feature-gated panic in a shared crate is acceptable is its call).
The six files above should be looked at by whoever owns the apps that wrote
them.

**A second defence, per app (2026-09-25, `apps/habits`).** An app can make
keeping opt-in: `HabitTrackerApp::new`, which every test uses, is backed by
nothing, and only `from_settings` -- what `main` calls -- turns writing on. A
test that forgets its scratch directory then writes nowhere, and
`a_tracker_made_with_new_writes_nothing` pins that. It protects the one app
that does it, not the next one written; the guard above is still the fix.
`apps/flashcards` does the same since the same day
(`an_app_made_with_new_keeps_nothing`), and `apps/finance`
(`a_window_made_by_new_keeps_nothing`).
