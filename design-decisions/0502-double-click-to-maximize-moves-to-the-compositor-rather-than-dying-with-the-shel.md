## 502. Double-click-to-maximize moves to the compositor rather than dying with the shell's decorator

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Clicking a window's title bar twice quickly makes it fill the
screen. That gesture was written into the desktop shell, and the shell's
title-bar code was about to be deleted as a duplicate of the compositor's. But
it was not a duplicate: the compositor had never implemented the gesture, so
deleting the shell's copy would have removed double-click-to-maximize from the
product entirely, with no test failing and nothing to notice. It was moved into
the compositor instead. This entry records that, and the three smaller choices
the move forced.

### Why this was nearly a silent feature deletion

`TD-C-THE-DESKTOP-AND-THE-COMPOSITOR-BOTH-DRAW-WINDOW-TITLE-BARS` had
established over several increments that the shell's decorator was a strict
subset of the compositor's: same buttons, same drag-to-move, and the compositor
additionally does border-resize, which the shell never did. Five prerequisites
were met to make the deletion regression-free. The natural next step is to
delete and move on.

The subset claim was true of *drawing* and true of *hit testing*, and false of
one thing. `Hit::WindowTitleBar`'s `handle_press` arm read:

```rust
MouseEventKind::DoubleClick(_) => { /* ...toggle maximize... */ }
```

and grep for `DoubleClick` in `gui/compositor` found only a comment saying the
compositor never emits it:

> `Enter`/`Leave` and `DoubleClick` exist only on the client side and are never
> produced here … double-click timing belongs with the widget that has to
> honour it.

So the compositor had deliberately declined to own double-click timing on the
grounds that it is a *widget* concern — which is right for a widget inside a
client's window, and wrong for a title bar, which is not a widget and belongs to
no client. The comment was correct about the general case and had quietly
misfiled the one surface the compositor itself draws.

**The alternative considered and rejected:** leave the gesture in the shell and
have the shell ask the compositor to maximize. Rejected because the shell does
not receive the press. The compositor hit-tests its own decorations and consumes
the event before the shell hears anything — that is the entire architecture the
deletion exists to restore. A shell that owns the timing for an event it is
never sent is not a design, it is the duplicate coming back in another form.

### The three sub-decisions

**1. The record keys on the window, not only on the time.**
`last_title_press: Option<(WindowId, Instant)>`, not `Option<Instant>`. Two
quick clicks on two *different* title bars are two clicks. Pairing them would
maximize a window the user clicked once — a window moving on its own, which is
the worst class of window-manager bug because the user cannot attribute it to
anything they did. The alternative (time only) is one field smaller and
indefensible.

**2. Any intervening press breaks the pair.** Implemented as a `take()` at the
top of the left-press path, before anything is dispatched, so that *only* a
title-bar press can leave a record behind:

```rust
let previous_title_press = self.last_title_press.take();
```

Click a title bar, click the desktop, click the same title bar again — however
fast — and that is two clicks, because the user went somewhere else and came
back. The alternative is to clear the record only on presses that land
somewhere "meaningful", which requires deciding what is meaningful and gets it
wrong the first time something new is added. Clearing unconditionally and
re-arming on the one path that wants it cannot drift.

**3. A completed double-click does not re-arm.** The doubled branch leaves the
record cleared (by the `take` above) rather than replacing it. Otherwise a third
quick click pairs with the second and un-maximizes what the user just
maximized — so a user who clicks three times in a hurry gets a window that
flickers between states rather than settling. Three clicks are a double-click
followed by a single click, which is what every other platform does.

### Why the default is 400 ms and the clamp is 100–2000

Both copied from `desktop::mouse_settings`, which has offered exactly that
default and exactly that range since it was written. Nothing wires the user's
choice through yet — that is
`TD-C-THE-MOUSE-SETTINGS-PANEL-REACHES-NOTHING`, filed with this change and
blocked on `apps/settings` gaining a compositor connection. Matching the
numbers now means **wiring it up later changes nothing for a user who never
touched the slider**, which is the property that makes the pending work safe to
land at any time.

The bounds are named constants (`MIN_DOUBLE_CLICK_MS`, `MAX_DOUBLE_CLICK_MS`)
rather than literals in the `clamp` call, because the same two numbers appear in
the clamp, in the doc comment and in the test, and three copies of a bound is
how a bound comes to disagree with itself. The test asserts against the
constants for the two edges and against a literal `250` for the interior — a
clamp that returned a bound for *every* input would satisfy both edge
assertions.

### Testing a timing-sensitive gesture without flakiness

The positive tests set the interval to its 2000 ms ceiling: no plausible
scheduling delay between two synchronous method calls approaches two seconds.
The negative test (`two_title_clicks_far_enough_apart_are_two_separate_clicks`)
sets the 100 ms floor and sleeps 160 ms. The asymmetry is deliberate and is the
whole trick: **a sleep can only overshoot**, so a loaded machine makes the gap
*larger*, which is the direction the test already expects. There is no
scheduling outcome in which this test fails spuriously. A test written the other
way round — sleep a little and assert the clicks *did* pair — would be a genuine
flake, because an overshoot breaks it.

`click_title_bar` re-reads `title_bar_rect()` on every call rather than caching
it, because a maximized window's title bar is somewhere else; a cached
rectangle would make the "click again to restore" half of the test press empty
desktop and pass for the wrong reason.
