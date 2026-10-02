## TD-C-EVERY-KEYSTROKE-WENT-TO-THE-LAST-TEXT-FIELD-IN-THE-WINDOW — RESOLVED 2026-08-24

**Lane C, found 2026-08-24 while adding a caret to `WidgetKind::TextInput`.**

The toolkit had no concept of keyboard focus. `Widget::handle_event`
(`gui/toolkit/src/widget.rs`) walked its children back to front and gave the
event to the first one that consumed it — and it did that for **key** events as
well as for mouse events, where the back-to-front walk is correct because it
means "topmost under the pointer". A key event has no pointer, so "the first
child that takes it, searched back to front" resolves to *the last text field in
the tree*, unconditionally.

So on any form with two text fields, everything the user typed went into the
second one, whatever they had clicked. The click itself was delivered correctly
— to the field under the pointer — which made the symptom look like a rendering
fault rather than a routing one.

### Why no test caught it

Every text-input test in the file built a tree with exactly *one* text field, so
"the field the user clicked" and "the last field in the tree" were the same
widget and no test could tell them apart. The regression test that does is
`typing_goes_to_the_field_that_was_clicked_and_not_the_last_one_in_the_tree`,
which uses two fields and clicks the first.

### The fix

A focus, introduced properly rather than as a flag on the key path — see
`design-decisions.md` §546 for the four choices it involved. The line that
closes the bug is the guard in `Widget::handle_event`:

```rust
Event::Key(key) if self.focused => self.handle_key(key),
```

with `WidgetTree::handle_event` moving the focus on a mouse press (by hit-test,
before the click is dispatched) and consuming Tab / Shift+Tab to step it.

### What it means for callers

`WidgetTree` now swallows typing until something is focused, where before it
delivered it to an arbitrary widget. A window that wants a field ready to type
into on open must say so with `WidgetTree::focus_first()`. That is a behaviour
change, but not one that breaks any current caller: no app in the tree routes
events through `WidgetTree::handle_event` today — they were all doing their own
key handling, which is itself a sign that this path was not usable.
