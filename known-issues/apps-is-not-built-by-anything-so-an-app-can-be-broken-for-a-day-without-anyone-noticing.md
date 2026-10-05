## `apps/**` is not built by anything, so an app can be broken for a day without anyone noticing

**In short:** The lock screen — the program that decides who gets into the
machine — did not compile, and had not compiled for a day. Nothing noticed,
because nothing builds the `apps/` directory. A change in a shared library on
another lane broke it, that lane's own tests stayed green, and the breakage was
found only because an unrelated tidy-up happened to run `cargo clippy` on that
one app.

Found 2026-09-06 while sweeping blanket `#![allow(dead_code)]` out of
`apps/**`. `apps/lockscreen` failed to compile at `HEAD` with two errors:

- `authlib::Authenticator::with_stores` takes one path, not two
- `authlib::shadow` does not exist

Both come from `5264cba7a` ("authlib: one account store, and the /etc/shadow
branch deleted", design-decisions §353), which states in its message that "no
caller changes". That was true of the callers that commit could see: `login`,
in `userspace/`, lane B's own tree. `apps/lockscreen` is lane C's, is a caller,
and was not updated. Repaired in the commit that follows this entry.

**The bug is not the missed caller — it is that a missed caller costs nothing.**
`apps/*` is a workspace *member* (`Cargo.toml` line 5), so a
`cargo build --workspace` for the host target would have failed. Nothing runs
one:

- the boot test builds for `x86_64-unknown-none` and `apps/*` are not in
  `default-members` for it
- each lane builds the packages it touched, and no lane touches all of `apps/`
- there is no CI

So the compile error was reachable by one command that nobody has reason to run.
All 143 app crates were then checked in one go: this is the only broken one,
and the check took 58 seconds warm. The breakage is rare and the guarantee is
cheap — see open question C-Q11, where that measurement changed the
recommendation.

**The proper fix** is a check that builds every workspace member for the host
target and fails on error, run the way `scripts/check-window-wiring.py` and
`scripts/check-gates-are-wired.py` are. It is a cross-lane concern (it would
gate all three lanes' merges), and it is slow — a cold `cargo check --workspace
--target x86_64-pc-windows-gnu` is minutes, not seconds — so the shape of it
(pre-push hook? a nightly sweep? `cargo check` rather than `build`?) is worth
agreeing on rather than one lane imposing. Filed as an open question rather than
done unilaterally.

Until then, the cheap partial mitigation is what found this one: any lane
touching a shared library under `userspace/` or `gui/` should
`grep -rl '<the changed symbol>' apps/` before concluding "no caller changes".
