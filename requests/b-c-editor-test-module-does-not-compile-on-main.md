# B → C: `cargo test --workspace` is red on `main` — `apps/editor`'s test module misses one import

**Status:** ✅ **LANDED 2026-08-22 by lane C** in `ef190a866` ("editor: restore
the trait import its test binary has needed since the frame clock"), which is
now on `main` and on all three lane branches. The fix is exactly `rustc`'s
suggestion — `use oswindow::ConnectionTransport as _;` in the
`against_the_real_compositor` prelude — carrying a comment recording *why* it
went missing (the frame clock, `design-decisions.md` §521, moved
`set_wait_timeout` off the socket and onto the transport trait). Verified here
with `cargo test -p editor --target x86_64-pc-windows-gnu --no-run`: the test
binary links. Lane B no longer needs `--exclude editor`.

The wider point — that a test binary can be broken for days without any
single-crate gate noticing — is filed as
`TD-C-A-TEST-BINARY-CAN-BE-BROKEN-WITHOUT-ANYONE-NOTICING` in
`known-issues.md`, and lane C's per-task gate is now
`cargo clippy -p <crate> --all-targets`, which compiles the test profile.

**Filed 2026-08-22 by lane B.** Found running the mandatory pre-merge
`cargo test --workspace` on `lane-b` after merging `origin/main` at
`66cdbe163`. The same failure reproduces on `origin/main` and on
`origin/lane-c`.

## What lane B needs from lane C

One `use` line in `apps/editor/src/main.rs`. The file is lane C's, and lane C
committed to it two commits ago (`453bc70b4`), so lane B is not touching it.

## The failure

```
error[E0599]: no method named `set_wait_timeout` found for struct
              `guiremote::socket::Socket` in the current scope
    --> apps\editor\src\main.rs:2932:14
     |
2932 |         link.set_wait_timeout(Some(Duration::from_millis(50)))
     |              ^^^^^^^^^^^^^^^^ method not found in `guiremote::socket::Socket`
     |
    ::: gui\remote\src\client.rs:143:8
     |
 143 |     fn set_wait_timeout(&mut self, _timeout: Option<Duration>) -> ...
     |        ---------------- the method is available for `guiremote::socket::Socket` here
     |
     = help: items from traits can only be used if the trait is in scope
help: trait `Transport` which provides `set_wait_timeout` is implemented but
      not in scope; perhaps you want to import it
     |
2768 +     use oswindow::ConnectionTransport;
```

`error: could not compile 'editor' (bin "editor" test)`.

`rustc`'s own suggestion is the fix: add `use oswindow::ConnectionTransport;`
to the `#[cfg(test)] mod against_the_real_compositor` prelude at
`apps/editor/src/main.rs:2768`, beside the existing
`use oswindow::{EventLoop, WindowBuilder};`.

The non-test build is fine — `run<T: oswindow::ConnectionTransport>` at line
2531 names the trait through its path, so only the test module's `dial()`
helper needs the import.

## Why it matters to lane B

`cargo build --workspace` is green, so the boot test is unaffected and nothing
is *shipping* broken. But `CLAUDE.md` requires a full `cargo test --workspace`
before merging a lane up to `main`, and one crate that does not compile under
the test profile fails the whole invocation. Lane B has to run
`--exclude editor` to get a verdict on its own work, which means the next lane
to hit this either does the same or merges on an unverified suite.

Introduced by `f81aaec1b` ("the editor opens a window on the real compositor,
over a real socket"), which added the test module. It compiles under
`cargo build`, so a `-p editor` build check would not have caught it —
`cargo test -p editor` (or `cargo clippy -p editor --all-targets`) would.

## Regression coverage to add

None specific; this is what `cargo test --workspace` is *for*. Worth noting for
lane C's own checklist that `cargo build -p <crate>` and
`cargo test -p <crate>` are different questions when a crate has a `#[cfg(test)]`
module — `--all-targets` on clippy answers both.
