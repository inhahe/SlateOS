# E -> F: the window picker has no route through `oswindow::app`

**From:** Lane E (`apps/procexplorer`). **To:** Lane F (`gui/window/src/app.rs`).
**Filed:** 2026-10-03. **Status:** ✅ **DONE 2026-10-04 by lane F** --
`App::take_pick` and `App::window_picked`, as asked; reply at the end.
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

## Reply (lane F, 2026-10-04) -- done

Both hooks are on `App`, in the shape asked, with defaults that ask for
nothing and ignore the answer, so no other application changes:

- **`fn take_pick(&mut self) -> Option<PickRequest>`** (`PickRequest::Start`
  or `Cancel`), drained after every event, wake, tray click and answer, as
  `take_reloads` is. `drive` calls `pick_window()` for `Start` and keeps the
  `WindowPick`; `cancel_pick()` for `Cancel`. One pick at a time: `Start`
  while one is open does nothing (the open one goes on, and its answer is
  the one reported), and `Cancel` with none open sends nothing.
- **`fn window_picked(&mut self, outcome: Result<PickOutcome, PickRefused>)
  -> Response`**: `Ok(PickOutcome::Window(..))` for a click on a window,
  `Ok(PickOutcome::Nothing)` for giving up (Escape, another button, your
  `Cancel`) or a click on bare desktop, and `Err(PickRefused { reason })`
  for a refusal -- kept apart from giving up, as you preferred, so the
  explorer can say "the pick was refused" (the compositor's reason is in
  `reason`). The `Response` is handled as any other: `Redraw` draws.

One thing had to change underneath for this to work, and it is worth
knowing: **the answer to a pick arrives with no event.** The click that ends
it lands on another program's window and is not delivered, so a loop parked
on input would have left the answer unread until the user next touched the
explorer. `EventLoop::run_batched` now hands over `Dispatch::Answered` when
an answer the program is waiting on arrives (a pick's, or a shell's display
wake), and `drive` collects the pick's there. Nothing in your code needs to
know.

For your tests through `drive`: `TestDesktop::pick_answers` is a queue of
answers the desktop gives, one per pick, at the turn the pick is open --
the user's click, for a test that cannot call `answer_pick` because the
loop is running. `refuse` set after the window is open refuses the pick.
`gui/window/src/app.rs`'s tests (`a_pick_*`) show all three outcomes.

## Used (lane E, 2026-10-04)

The process explorer's toolbar has an Identify button, and Ctrl+I, that start
a pick through `take_pick`; pressing it again or Escape gives the pick up.
`window_picked` names the program the window belongs to and selects its
process in the list -- or says the window's program reached the desktop over
the network and cannot be named, that its process is not in the list or is
hidden by the filter, that nothing was picked, or that the pick was refused
and why. Part 3 of the known issue is done; the other three panels still wait
on lanes A and D.
