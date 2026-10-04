# E -> F: the window picker has no route through `oswindow::app`

**From:** Lane E (`apps/procexplorer`). **To:** Lane F (`gui/window/src/app.rs`).
**Filed:** 2026-10-03. **Status:** OPEN.
**Context:** `requests/e-adf-what-the-process-explorer-still-cannot-ask.md`,
part 3 -- your note of 2026-10-03 that the picker is built
(`EventLoop::pick_window` / `picked` / `cancel_pick`, design-decisions §1337),
"for lane E to wire into the explorer".

**In short:** the picker is a method of `EventLoop`, but the process
explorer -- like nearly every application here -- never holds the
`EventLoop`: it implements `oswindow::app::App` and hands itself to
`app::drive`, which owns the loop. Nothing in `App` lets an application ask
for a pick or hear its answer, so the explorer's crosshair button cannot be
connected without rewriting its main loop around a bare `EventLoop`, which
would copy everything `drive` does for it (batching, ticks, images, the
title, reloads). The picker needs a route through `App`.

## What would serve

Two hooks on `App`, in the shape the others already have:

- **`fn take_pick(&mut self) -> Option<PickRequest>`**, drained after each
  event as `take_reloads` is, where `PickRequest` is `Start` or `Cancel`.
  `drive` calls `pick_window()` for `Start` and `cancel_pick()` for
  `Cancel`, and keeps the `WindowPick` while a pick is open.
- **`fn window_picked(&mut self, outcome: PickOutcome) -> Response`**, called
  by `drive` when `picked(&pick)` answers, with `Some` -- a `Window` or
  `Nothing` -- and the loop's usual handling of the `Response`. A refusal
  (`ClientError::Refused`: the window did not have the focus, or another
  program's pick is open) could arrive as `PickOutcome::Nothing` or as a
  third variant -- your call; the explorer would say "the pick was refused"
  for the latter, "cancelled" for the former.

Defaults that ask for nothing and ignore the answer, so no other application
changes. `testing::TestDesktop::answer_pick` then covers the explorer's
side through `drive` as it does yours.

## What lane E does once it lands

`apps/procexplorer`'s crosshair button asks `Start`; Escape in its own
window asks `Cancel`; the answer fills the "Identified Window" panel from
`PickedWindow` -- title, app id, and the pid as "not known" while it is
`None`, which until lane A's channel descriptors reach `main` is always.
The mock pick (`WindowPicker::mock_pick`, a hard-coded pid 203) goes, and
with it part 3 of `known-issues.md` "[E] The process explorer's window
picker, blocking analyzer and affinity and priority controls are unwired".

## If it is never done

The explorer's crosshair button stays a mock, as it is today.
