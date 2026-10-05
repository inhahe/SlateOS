## TD-A-SELFTESTS-NOT-IDEMPOTENT — the never-run self-tests panic on their *second* run, and that is why they can't just be switched on

**Lane A. Found 2026-08-23, while wiring modules up under
TD-A-FS-SELFTESTS-NEVER-RUN.**

TD-A-FS-SELFTESTS-NEVER-RUN predicted that "a substantial number will fail
or panic on first run". That is true, but it understates the shape of the
problem and mis-states when it bites. The dominant failure is not a first
run — it is the *second*. Every one of these suites has, by construction,
only ever been run once in its life, so nothing ever exercised the case of
running it against state a previous run left behind.

**The pattern.** A module keeps `static STATE: Mutex<Option<State>>`, with
an `init_defaults()` that returns early when the state already exists and a
`with_state()` that does **not** lazily initialise. The suite then:

1. calls `init_defaults()` — a no-op on the second run,
2. asserts the table is empty (or in its seeded shape),
3. creates fixtures, and
4. never removes them.

Run once, it passes. Run twice, step 2 fails against step 3's leftovers and
the assertion panics — which in the kernel is not a red test, it is a dead
machine. Each of these is reachable from a `kshell` subcommand, so this is
a user-typeable kernel panic, not merely a testing inconvenience.

**Found in six modules while converting them for §261** — it was not a
coincidence in any of them, and all six are now fixed:

| Module | Second-run failure | Fixed in |
|---|---|---|
| `fs::netshare` | test 1 `assert!(list_shares().is_empty())` — `id1` was never unmounted | `c5db19b8a` |
| `fs::filevault` | test 1 `assert_eq!(list_vaults().len(), 0)` — the created vault was never deleted; test 8's exact counter assertions could not hold twice either | `c150f1b29` |
| `fs::diskencrypt` | test 9 `start_encryption(1, …)` — run 1 leaves volume 1 `Unlocked`, and it only accepts `Unencrypted` | `59bd8befc` |
| `fs::cloudsync` | test 3 `assert_eq!(list_accounts().len(), 2)` — run 1 leaves account `id1`, its conflict row and an extra `*.bak` exclude behind, so tests 3, 6, 8, 10 and 11 all fail on the second run | `2ebe9f40c` |
| `fs::fileversion` | test 8 `assert_eq!(list_watches().len(), 1)` — run 1 leaves the watch behind. Worse, run 2's watch then *covers* the fixture, so its `KeepLast(5)` policy silently replaces the default `KeepLast(10)` the capture tests assume | `aae93b532` |
| `fs::pinnedapps` | test 7 `assert_eq!(count, 1)` — `record_launch` is cumulative, so run 1 leaves `files` at 1 and run 2 reads 2. Test 6's reorder is likewise still in place, having moved `terminal` to position 0 | `d2876326f` |

`fs::cloudsync` also showed a fixture hazard worth naming separately,
because it is not about leftovers at all and so survives any amount of
cleanup: the suite's fixtures were **plausible values a user might really
have**. It added the account `user@cloud.example` and the exclude pattern
`*.bak`; `add_account` rejects a duplicate `(provider, account_name)`, so a
user who genuinely syncs that NextCloud account would have had the suite
fail on *its first* run, on a machine where it had never run before — no
amount of cleanup discipline prevents that. The fix is to make fixtures
unmistakably synthetic: the account names are now in the reserved
`.invalid` TLD and carry a `selftest` marker, and the exclude pattern is
`*.cloudsync-selftest.bak`. Check for this whenever a suite's fixture is a
*name* rather than an index — an ID the module mints is safe, a string the
user also chooses is not.

`fs::fileversion` had the same hazard with a sharper edge, because its
fixture was fed to a *destructive* call: the suite captured versions of
`/home/user/test.txt` and then ran `purge_file_versions` on it. On a
machine where a user actually had that file under version control, typing
`fversion test` would have deleted their real version history — and, since
the purge count is what the suite asserts on, would have failed only
*after* doing so. Its fixtures now live under `/tmp/.fileversion-selftest/`,
a directory no user would keep data in. **Generalise further:** a plausible
fixture is bad; a plausible fixture handed to a delete/purge/reset entry
point is data loss.

cloudsync's test 8 was additionally asserting `list_excludes().len() >= 6` — the
count of the defaults `init_defaults()` installs. That is not a property of
the module: `remove_exclude` is public and has a shell command, so a user
can take a default away and turn the assertion into a panic. It now asserts
the round-trip (added pattern is visible, removed pattern is gone) instead.
**Generalise:** an assertion about the *defaults* is only sound in a suite
that resets to `None` first; a baseline-relative suite may assert only
about what it itself changed.

`fs::fileshare` was a near miss of a different flavour: it *does* reset at
entry, so it survives a second run, but it left `sharing_enabled = true`
and the hostname set to `"fileserver"` in the live table. Wiring it into
boot as it stood would have made `fileshare show` report sharing switched
on and the machine renamed, without the user having asked for either.

**A worse variant: the suite that "fixes" non-idempotency by wiping the
user's data.** Three of the §261 modules — `fs::certmgr`, `fs::appregistry`
and `fs::startmenu` — *were* idempotent, and were idempotent for the wrong
reason: each opened with `clear_all()`. That does make a second run pass,
and it makes the opening emptiness assertion true by construction. It also
means `certmgr test` deleted every certificate in the trust store,
`appreg test` deleted every registered application, and `startmenu test`
deleted the user's favourites and quick links. These are shell commands. A
user typing `test` on a subsystem reasonably expects to be told whether it
works, not to have its contents destroyed. Treat a `clear_all()` at the top
of a suite as a bug on sight, not as cleanup.

**The three fixes, and when to use which:**

- *Reset at both ends* — `*STATE.lock() = None; init_defaults();` on entry
  and `*STATE.lock() = None;` on exit. Correct when the module has no lazy
  init, so `None` is exactly the state a fresh boot has. This is what
  `fs::inodestat` already did and documented, and what `filevault`,
  `diskencrypt`, `fileshare`, `screenrec` and `pinnedapps` now do.
- *Baseline-relative + full cleanup* — capture the row count on entry,
  state every count relative to it, and assert on exit that the table was
  restored. Correct when the module may legitimately hold live rows the
  suite must not destroy. This is what `netshare`, `fileversion` and
  `certmgr` now do.
- *Decline to run* — check on entry whether the store is populated and, if
  it is, print a `self-test skipped: …` line and return `Ok(())`. Correct
  when the module holds user data **and** the suite's assertions are exact
  counts that cannot be restated relative to a baseline ("exactly one app
  in Accessories" is not a statement you can make baseline-relative). This
  is what `appregistry` and `startmenu` now do, and what `fileversion`'s
  watch guard does. It costs coverage on a machine in use and gains full
  coverage at boot, where the store is genuinely empty — the right trade,
  because the alternative on offer is not "more coverage" but "coverage
  purchased with the user's data".

Prefer the second where the module could plausibly be in use; fall back to
the third only when the assertions cannot be made relative. Never reach for
`clear_all()`: the first shape is safe only because `None` is what a fresh
boot has, which is a fact about the module, not a licence to empty a table.

**Reproduce:** run any such suite's shell subcommand twice, e.g.
`dencrypt test` then `dencrypt test`.

**The proper fix** is to apply one of the three shapes above to every
state-holding suite in the ~250 still-manual-only set, as each is wired up
under TD-A-FS-SELFTESTS-NEVER-RUN — checking specifically for (a) an
opening emptiness/shape assertion, (b) fixtures that are never removed, and
(c) exact assertions on cumulative counters, which cannot hold on a second
run even when the rows are cleaned up. Doing this *at wiring time* is
essential rather than optional: a non-idempotent suite that has been wired
into boot will pass the boot test (a fresh boot runs it exactly once) and
still panic the kernel the first time a user types the subcommand.

**Measured scope of the destructive variant: 56 modules, not three**
(counted 2026-08-23). The `clear_all()`-at-the-top shape was not a quirk of
the three §261 modules that happened to be converted first — it is the house
style for `kernel/src/fs/*.rs` self-tests. Every module below opens its
`self_test()` by wiping its own persistent table:

```
a11y appnotify autostart bootcfg capsettings cas colorpicker credentials
cursorsettings detailcols display dyndns fcomment filepicker fontmgr fstune
hotkeys ime immutable installer ioprio kbsettings keylayout locale
loginscreen mmtune netindicator netsettings notifcenter osreset partmgr
perfmon power prefetch progmgr queryable rundialog schedtune screenshot
scriptlang servicemgr soundmixer swapcfg sysinfo systray tags taskbar theme
timezone useracct vdesktop vpn wakesensor wallpaper widgets winsnap
```

(`cas` uses `clear()`, `ioprio`/`prefetch`/`tags` use `test_clear()`; the
rest use `clear_all()`. Recount with the awk one-liner in the git history of
this entry, or by hand: the destructive call is within the first six lines
of `pub fn self_test`.)

**Why this is a live data-loss bug and not merely latent.** Each of these
has a `<module> test` shell subcommand today. Nothing warns the user, and
the command reports success afterwards — `useracct test` prints `all tests
passed` having deleted **every user account, every group and every session**
on the machine and left `current_uid` at `None`. `credentials test` empties
the credential store; `hotkeys test` discards every custom key binding;
`wallpaper`, `theme`, `keylayout`, `locale` and `timezone` reset the desktop
to factory defaults. The suites that look least alarming are the settings
modules, and they are the ones a user is most likely to poke at.

**Why it is not a boot problem.** At boot the tables are empty, so the wipe
is a no-op and every one of these is safe to run from `main.rs` exactly as
written. That asymmetry is the trap: wiring one of these into boot under
TD-A-FS-SELFTESTS-NEVER-RUN gives a green boot test and leaves the
destructive shell path untouched, so the boot wiring cannot be used as
evidence that the suite is safe. The two must be fixed together.

**The proper fix** is the same three shapes above, applied per module. Most
of these are settings modules whose suites assert exact counts on a table
that `init_defaults()` populates, which points at *reset at both ends* where
there is no lazy init and *decline to run* where the table holds anything
the user chose. `useracct` additionally needs its `current_uid`, its
sessions' `active` flags and `LOGIN_COUNT` snapshotted and restored, because
authenticating during the test hijacks whoever is logged in — cleaning up
the fixture user is not enough.

### ✅ FIXED 2026-08-23 — a fourth shape, and why it beat all three above

**All 56 are done**, and none of them used one of the three shapes. Writing
them out made it clear that the three were a choice between *keeping the
user's data* and *keeping the coverage*, and that the choice was false.

The shape that landed is **move the live state aside**: swap the module's
table for a pristine one, run the suite against that, put the original back.
It is `crate::fs::selftest::with_pristine`, and every converted suite is now

```rust
pub fn self_test() -> KernelResult<()> {
    crate::fs::selftest::with_pristine(&STATE, State::new(), self_test_inner)
}
```

with the original body moved verbatim into `self_test_inner`. Every existing
assertion holds **unchanged and unweakened** — including the exact ones
(`next_id` starts at 1, "exactly one app in Accessories") that were the
reason *baseline-relative* could not be used and *decline to run* had to give
up coverage on exactly the machines where a user types `test` because they
suspect something is wrong.

Why it was not available before: it needs a *pristine value* to swap in, and
23 of the 56 modules had no name for one — their fresh state existed only as
an anonymous literal inside `static STATE: Mutex<State> = Mutex::new(State {
… });`. Those literals are now `const fn new()`. That is worth having on its
own account: the literal and `clear_all()` were two independent spellings of
"what a fresh boot looks like", free to drift apart with nothing to catch it.

**Three corrections to the survey above, found while doing the work:**

- **`ioprio`, `prefetch` and `tags` were false positives.** The survey
  counted them because they call `test_clear()`, but in all three that is a
  *test of* the per-entry clear, not a wipe of the table. `ioprio` was
  already well-behaved — synthetic task IDs in the 99996–99999 range, each
  cleared afterwards — and got only its two statistics counters restored,
  with a doc comment saying why it has no `with_pristine`. `prefetch` and
  `tags` clean up by hand, one call at a time, which is a claim nobody
  re-checks when a test is added; they are wrapped anyway, so "leaves no
  trace" is now structural rather than a promise. So the destructive count
  is **53**, not 56.
- **Free-standing counters are part of the state and the three shapes did
  not cover them.** Nearly every module keeps its statistics in
  `static AtomicU64`s outside the table, so restoring the table alone still
  left `theme stats` (and the rest) reporting the test's activity as the
  user's. All of them are now saved and restored around the call.
- **`servicemgr` is the one lazy-init module in the set** — its state is
  `Mutex<Option<State>>` — so its pristine value is `None`, which is exactly
  what a fresh boot holds.

`useracct` was fixed earlier and separately, and keeps its *decline to run*
guard: its suite authenticates, which reaches `current_uid`, the sessions'
`active` flags and `LOGIN_COUNT` — state that is not reachable from the one
table `with_pristine` swaps.

**What is deliberately not handled:** a panic mid-suite does not restore.
That is on purpose — a kernel that has just proved one of its own invariants
wrong has no business carrying the user's data forward into whatever runs
next. Every non-panicking exit, including an early `?`, does restore.
