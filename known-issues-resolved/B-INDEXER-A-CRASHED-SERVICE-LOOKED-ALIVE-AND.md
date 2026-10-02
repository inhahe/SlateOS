## B-INDEXER-A-CRASHED-SERVICE-LOOKED-ALIVE-AND-`stop`-NEVER-STOPPED-IT (lane C, 2026-08-16) — FIXED

**Status: FIXED 2026-08-16** (lane C, `606a6e514`).

**In short:** the file indexer decided whether it was running by checking
whether a file existed. That file survives a crash, so after one crash
`indexer status` said "running" forever and there was no way to clear it short
of deleting the file by hand. Separately, `indexer stop` never actually stopped
anything — it deleted that file and printed "Stop signal sent." while the
service carried on scanning your disk.

### What the code did

`apps/indexer/src/main.rs`, three commands sharing one wrong assumption:

```rust
fn cmd_status() {
    let running = Path::new(PID_FILE).exists();      // <- the whole liveness test
```

```rust
fn cmd_stop() {
    match fs::read_to_string(PID_FILE) {
        Ok(pid_str) => {
            println!("Stopping indexer service (PID {})...", pid_str.trim());
            // On Slate OS, we'd send an IPC shutdown message. For now, remove PID file.
            fs::remove_file(PID_FILE)?;
            println!("Stop signal sent.");           // <- nothing was sent
```

```rust
fn cmd_start(config: &Config) {
    // ... no check of any kind ...
    if let Err(e) = write_pid_file() { eprintln!("warning: ..."); }
```

Three distinct failures fall out of that:

1. **A crash is permanent.** `/var/indexer/indexer.pid` outlives a panic, a
   kill and a power cut. `status` reports "running" from then on. `stop` reads
   the stale pid, announces it is stopping PID N — a number that may by then
   belong to an unrelated process — deletes the file and reports success.
2. **`stop` was a no-op with a success message.** The comment says so outright.
   The service kept rescanning and kept calling `index.save()`. The user, and
   `status`, believed it was stopped. This is the worst of the three, because
   the failure is *silent and in the opposite direction from the report*.
3. **Two services could run at once.** `start` never looked. After a `stop`
   (which stopped nothing) a second `start` succeeded, and both services then
   wrote `/var/indexer/index.db`; whichever `save()` landed second won and the
   other's whole scan was lost.

### Why the shape matters

This is the same defect as `B-APPS-SAVING-A-DOCUMENT-COULD-DESTROY-IT`, one
level up: **something downstream treated a file's existence as a promise**. There
the promise was about the file's *contents*; here it is about a *process being
alive*. Both fail the same way — the file is trivially producible by an event
that has nothing to do with the promise (an interrupted write; an abnormal
exit), and nothing ever re-checks.

The general lesson, recorded so the next sweep can use it as a search: a file's
existence can only stand in for a fact that the filesystem itself maintains. It
cannot stand in for a fact about a *running process*, because a file has no way
to learn that a process died. Anything of the form "`X.pid`/`X.lock`/`X.running`
exists, therefore X is alive" is wrong by construction, and no amount of careful
cleanup on the exit paths fixes it — the exits that matter are the ones that do
not run cleanup.

### The fix

Liveness is an **exclusive lock** (`std::fs::File::try_lock`, stable since
1.89) held on `/var/indexer/indexer.lock` for the whole life of the service.
The kernel drops the lock when the process exits by *any* route, so a dead
service leaves nothing behind that claims otherwise. A new `ServiceControl`
type owns the three files, and a `ServiceGuard` releases the lock and cleans up
on drop.

- `start` takes the lock, and exits 1 with "already running" if it cannot.
- `status`/`stop` probe it with `try_lock` and release immediately.
- `stop` writes a stop-request file, which the service polls for every 100 ms
  while it sleeps between scans, then **waits for the lock to be released**
  before reporting success — so it reports what happened, not what it asked
  for. If the service does not exit within 30 s it says so and exits 1.
- A stop request left over from a crash is discarded by the next `start`, so it
  cannot stop a service it was never aimed at.

Two details worth keeping:

- **The pid lives in a separate file from the lock, and is display-only.**
  Windows file locks are mandatory, not advisory: measured on this host, a
  second process reading a locked file gets `Os { code: 33, "another process
  has locked a portion of the file" }`. Putting the pid inside the lock file
  would make it unreadable exactly when it is wanted. Being a separate file, it
  can go stale — which is now harmless, because it is only ever read after the
  lock has already said the service is alive.
- **`start` retries a locked lock file for one second** before concluding a
  real service holds it. `status` takes the same lock for an instant to answer
  its question, and a `status` landing in that window would otherwise make a
  legitimate `start` refuse.

`stop`'s request file is a stand-in for the IPC shutdown message this will send
once the service runs under Slate OS's service manager — Slate OS has no Unix
signals for process control by design, so there is no signal to send instead.

### Verification

Eight tests, in `apps/indexer/src/main.rs`. The load-bearing one is
`a_crashed_service_leaves_nothing_that_looks_alive`, which builds precisely the
on-disk state a killed service leaves — lock file and pid file present, no lock
held — and asserts `is_running()` is false. Re-introducing the old
`self.pid_path().exists()` as the body of `is_running` was tried: that test, and
only that test, went red (70 passed / 1 failed), then green again on revert.

Note that `holding_the_lock_is_what_reports_a_running_service` passes *with* the
injected bug, because a live service has a pid file too. That is the point — the
happy path cannot distinguish the two implementations, which is why the bug
survived in the first place, and why the regression test has to construct the
crash state explicitly.

Clippy: 150 non-test warnings before, 150 after (the first draft added three
`clippy::arithmetic_side_effects` from `Instant + Duration`, which panics on
overflow; rewritten to `Instant::elapsed` and `Duration::saturating_sub`).
