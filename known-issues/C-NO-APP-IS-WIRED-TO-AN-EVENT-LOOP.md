## `C-NO-APP-IS-WIRED-TO-AN-EVENT-LOOP`

**In short:** none of the 140 apps in `apps/` can be used. Each one builds its
picture correctly and has tests proving the picture is right, but no app is
connected to the thing that delivers mouse clicks and keystrokes, so `fn main()`
draws a single frame and exits. Nothing is broken *inside* the apps — the
missing piece is the wiring between an app and the compositor (the program that
owns the screen and the input devices).

**The evidence**, from a survey of all 140 crates under `apps/` (every `.rs`
file in each, not just `main.rs`):

- **59 of 140 define no event handler anywhere in the crate** — no
  `handle_event`, `handle_mouse` or `handle_key` in any of its `.rs` files.
  Among them: podcast, defrag, diskanalyzer, netmanager, taskscheduler,
  vpnmanager, tmux, explorer, notes, calendar, contacts, email.
- **60 have a stub `fn main()`** of the form `let _app = FooApp::new();` — it
  constructs the app and immediately drops it.
- The rest render one frame and print about it. `apps/settings/src/main.rs`
  says so outright: `// In a real Slate OS environment, this would enter the
  compositor event loop. // For now, render one frame to verify the UI builds
  correctly.`
- There is **no `trait App`** in `gui/toolkit` — nothing defines what an app
  must provide in order to be hosted. Each app invented its own handler
  signature by convention, which is why three different names for the same
  method exist across the tree.

**Why this has stayed invisible:** the apps' tests call `render()` and
`handle_event()` directly, so they pass and prove real things about the
drawing and the state machine. What no test can currently assert is that
anything ever *calls* those methods outside a test. That is also the root
cause of the whole dead-scroll-offset class above: a scroll offset can sit
unwritten for as long as you like when there is no input path that would have
written it.

**The proper fix:** define the app-side contract once in the toolkit — a trait
with `render(&self) -> RenderTree` and `handle_event(&mut self, &Event) ->
EventResult`, which is the shape most apps already have — and a `run()` that
connects it to the compositor client protocol already present in
`gui/compositor/src/lib.rs` (`ClientMouseKind` at line 2066 is the app-facing
half and already exists). Then convert apps to it. That is a large but
extremely mechanical change, and it wants to be done *before* many more apps
are written, since every app added meanwhile is another one to convert.

**What it costs while unfixed:** nothing regresses, because nothing runs; but
every UI behaviour in `apps/` is unverified against a real user, and each new
app adds more unexercised surface. Related: the crate-wide
`#![allow(dead_code)]` entry below, which is a direct symptom.
