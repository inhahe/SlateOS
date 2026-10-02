## C-A-HUNDRED-AND-THIRTY-TWO-APPS-NEVER-OPEN-A-WINDOW (lane C, 2026-08-26)

**In short:** SlateOS has about 140 desktop programs. Six of them can actually
appear on screen. The other 132 are complete applications — layout, drawing,
keyboard handling, tests, the lot — whose `main` function never asks the
compositor for a window, so running one does nothing visible and it exits
immediately. Nothing in the build reports this, because from the compiler's
point of view every one of those programs is a valid program that ran and
finished. The fix is per-app and mechanical; the thing that needed fixing
*once* was the absence of anything that would ever notice.

### What "wired" means

A SlateOS application becomes a window in exactly one place:

```rust
oswindow::app::launch("name", &mut app)
```

`launch` connects to the compositor, creates the surface, and runs the event
loop until the window closes. Reaching it is the entire difference between a
program the user can use and a program that exits before they see it. There is
no partial credit and no second door.

### The count, as measured on 2026-08-26

| | count | what its `main` does |
|---|---|---|
| **wired** | 6 | reaches `app::launch` |
| **empty `main`** | 20 | nothing at all — `fn main() {}` |
| **simulation `main`** | 112 | constructs the app, renders one throwaway frame, prints a line, exits |

The six that work: `diskimager`, `imageviewer`, `metronome`, `settings`,
`editor`, `explorer`. Every one of the six was wired on 2026-08-25 or -26
(`git log -S"app::launch"` gives the dates); before that, **no application in
the tree could open a window at all.**

The twenty with an empty `main`: `alarmclock`, `archivemanager`, `benchmark`,
`clipmanager`, `colorpicker`, `credmanager`, `defrag`, `devicemanager`,
`diskanalyzer`, `diskcleanup`, `fileassoc`, `lockscreen`, `netmanager`,
`partmanager`, `pdfviewer`, `speedtest`, `startupmanager`, `stickynotes`,
`taskscheduler`, `vpnmanager`.

The other 112 range from a comment that admits it —

```rust
// In a real Slate OS environment this would enter the compositor event loop
```
> — `apps/calculator/src/main.rs`

— to `apps/snake/src/main.rs`, whose `main` is one line in its entirety:

```rust
fn main() { let _app = SnakeApp::new(); }
```

### Why nothing caught it

This is lesson 45 — *a feature with no production caller is a feature that does
not exist* — at whole-program scale, and it is the same shape as the dead Write
button and the dead Refresh button in `apps/diskimager`, both recorded above.
`dead_code` cannot see any of the three. Every type is constructed and every
method is called: by `main`, or by the tests, or by both. The lint's question is
"is this reachable?", and the answer is yes. The question nobody was asking is
"is this reachable *from a window*?"

Two further reasons it stayed invisible for so long:

* **The tests pass.** Each app's `handle_event` and `render` have real coverage,
  driven directly by the test module. Coverage of a function that production
  never calls is coverage of a program that does not run.
* **The comment reads like a plan, not a defect.** "In a real Slate OS
  environment this would…" is written in the voice of a deliberate deferral. It
  is a hundred and twelve deliberate deferrals, none of which was tracked.

### Two generations of drawing, and why the second half is harder

The 132 split by what they build:

* 39 assemble a `RenderTree` — the type `oswindow::app::App::render` returns.
  Wiring one is an `impl App` block and a new `main`; `apps/diskimager` is the
  worked example.
* 93 return a flat `Vec<RenderCommand>`, the older shape, written before the
  tree existed. These need the conversion *as well as* the launch.

That split is why the gate below matches both names. A check that knew only
`RenderTree` would report 39 and stay silent about the harder ninety-three.

### The gate: `scripts/check-window-wiring.py`

Written the same day, modelled on `scripts/check-tick-wiring.py`. It reports
every file that draws, defines a `main`, and never names `app::launch` — and it
searches **production code only**: comments and `#[cfg(test)]` items are blanked
first, so neither the promise of an event loop nor a test that calls `render` by
hand can vouch for wiring that is not there. That is not a hypothetical
precaution; the promise-of-an-event-loop comment is present in most of the 112,
and without the blanking the gate would be blind to the exact population it
exists to find.

It **ratchets rather than failing outright**: `BASELINE = 132` is the count as
found, the check fails if the number goes up, and the constant is meant to be
edited downwards as apps are wired. Failing on all 132 today would mean a gate
that is red from the minute it is written, and a gate that is always red is a
gate that gets commented out.

The shared scanning machinery — comment/literal blanking, `#[cfg(test)]`
stripping, brace matching — moved into `scripts/rustscan.py` in the same change,
because two copies of a 300-line scanner are two copies that drift, and the one
that drifts is always the one nobody is currently editing.

### Status

**Open, and expected to stay open for a while.** 132 apps is 132 separate
pieces of work, each small. Nothing is blocked on anyone else: the toolkit, the
window crate and the compositor are all in place, and `apps/diskimager` is a
complete worked example of both halves (an `impl App` with a `tick_interval`
that only asks for a clock while something is moving, and a `main` that returns
`launch`'s `ExitCode`).

Order of attack, highest value first: the shell surfaces users cannot avoid
(`launcher`, `systray`, `lockscreen`, `gui/notifications`), then the ones whose
absence makes another feature untestable (`partmanager`, `devicemanager`,
`netmanager`), then the rest.

### The lesson, stated so it generalises

**A program that never reaches its event loop is a program that does not exist,
and no lint in any language can tell you so.** The compiler's reachability
question stops at `main`. Whether `main` reaches the *outside world* — a window,
a socket, a device — is a question only a project-specific check can ask, and it
has to ask it of production text, because the codebase will otherwise answer
with its own comments and its own tests.
