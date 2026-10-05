## C-CREDMANAGER-SCROLLS-INTO-EMPTY-SPACE-FOREVER (lane C, 2026-08-20)

**Status:** FIXED the same day, in `credmanager: give the two scroll offsets an
end to stop at`. The three steps below were done in that order and in one
commit; `scripts/reintro-credmanager.py` puts each of the nine resulting
defects back and checks the suite goes red for it. The write-up is kept
because the *shape* of the bug — a bound that could not exist because the
state did not know the window size — is worth recognising elsewhere in
`apps/`.

**Originally:** found while converting every wheel handler in the tree to
`guitk::wheel`; credmanager was the one app deliberately left out of that
change because fixing it properly needs a small refactor first.

`apps/credmanager/src/main.rs` holds two pixel scroll offsets, `list_scroll`
and `detail_scroll`. Both are clamped with `.max(0.0)` and **nothing else** —
there is no upper bound at all, so the wheel walks either pane off the end of
its content and keeps going. Fifty notches past the last credential leaves a
blank panel with no indication of which way is back except to scroll up an
equal amount.

The reason it has no upper bound is structural rather than an oversight:
`AppState` does not know how big the window is. `build_render_tree(state,
width, height)` takes the size as parameters, `render_entry_list` derives its
own pane height from them (`let h = height - y_start;`), and `handle_event`
does not handle `Event::Resize` at all — so at the moment the wheel is turned
there is no size in scope from which a bound could be computed. `fn main()` is
empty and the only callers of `build_render_tree` are the eight tests, which
is why nobody has noticed.

**The fix**, in this order and in its own commit:

1. Put `width`/`height` on `AppState` and handle `Event::Resize`, as every
   other app in `apps/` already does. `build_render_tree`'s parameters become
   redundant and should go, so there is one answer to how big the window is
   rather than two that can disagree.
2. Have the renderer report the content height it actually laid out, so the
   clamp uses the renderer's own derivation instead of a second copy of
   `height - y_start` that will drift the first time the header changes.
3. Only then convert both offsets to `wheel::pixels(dy, ROW_HEIGHT)` — they
   are continuous, so `pixels` and not an `Accumulator` — clamped to
   `(content - pane).max(0.0)`, and pin it with the same four tests the other
   eight apps now carry: one notch is three rows, a fraction of a notch moves
   now, the end stops at a full pane, a non-finite delta does nothing.

Relevant constants already in the file: `ROW_HEIGHT = 52.0`,
`SIDEBAR_WIDTH = 220.0`, `ENTRY_LIST_WIDTH = 320.0`, `TOOLBAR_HEIGHT = 48.0`.
