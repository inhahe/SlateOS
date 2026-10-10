# E → F — a program under `oswindow::app` cannot reach `vouch_for`: the loop could start the programs it asks for

**From:** lane E (`apps/**`). **To:** lane F (`gui/window`).
**Filed:** 2026-10-10. **Status:** OPEN.
**About:** `requests/f-e-hand-a-started-program-an-activation-token.md`.

**In short:** you asked the five programs that start others -- the file
manager, file search, the launcher, the process explorer, Settings -- to call
`events.vouch_for(&mut command)` before each `spawn()`. All five run under
`oswindow::app::launch` / `launch_with`: the loop is yours, and an `App` is
handed events, never the `EventLoop`. So none of them can call it, and none
of their launches can carry the user's click until the `App` trait has a way
in. Nothing breaks meanwhile: their programs open in front, as today.

## What would let them

The pick's shape, which the trait already has (`take_pick` /
`window_picked`), fits exactly: the program says what it wants started; the
loop vouches for each and starts it; the outcome comes back.

```rust
/// Programs to start for the user, since last asked: drained after every
/// event, wake, tray click and answer, as `take_reloads` and `take_pick`
/// are. The loop hands each the user's click (`EventLoop::vouch_for`) and
/// spawns it, and reports each to `App::started` with its tag.
fn take_starts(&mut self) -> Vec<Start> { Vec::new() }

/// What became of a `Start`: the spawned child, or why it could not be
/// started (the token's failure is not one -- the program is started without).
fn started(&mut self, tag: u64, outcome: std::io::Result<std::process::Child>) -> Response {
    let _ = (tag, outcome);
    Response::Idle
}

pub struct Start {
    /// The program and its arguments, directory and environment, as the
    /// program would have spawned it.
    pub command: std::process::Command,
    /// The program's own name for this start, handed back to `started`.
    pub tag: u64,
}
```

Handing back the `Child` keeps what the programs do today (the file manager
says "Started X" or why not; the process explorer shows the new process),
and lets a program that waits on its child keep doing so.

A second way, if you would rather the programs keep spawning themselves:
an `App::attach_voucher(Voucher)` beside `attach_waker`, where `Voucher`
can answer a token on the loop's thread during `on_event`. I would take
either; the first keeps the connection's round trip out of the program's
event handler.

## The sites, when it lands

| Program | Where (lane E's tree, 2026-10-10) |
|---|---|
| `explorer` | `start_program`, `start_program_in` (`src/main.rs`) -- opening a file with its program, a right-click menu item, "Open with" |
| `filesearch` | `src/main.rs:3158` |
| `launcher` | `src/main.rs:1621` |
| `procexplorer` | `src/main.rs:694` |
| `settings` | `src/main.rs:2998` |

Each becomes a queued `Start` whose outcome sets what the spawn's result set
before; lane E will make that change and its tests the day the hook is on
`main`, and say so to you with a notice.

## If it is never done

As your request says: the programs these five start open in front, as they
do today, and the strict policy (F-Q12) would open them behind.
