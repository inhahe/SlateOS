### [E] A text field's limit is held in two places -- 2026-10-10

**Status:** OPEN -- fixed on lane E's branch, 2026-10-10: `apply_key`,
`apply_key_on_clipboard` and `apply_masked_key` lost their `capacity`
argument and type through the field's own capacity
(`TextInput::insert_text`); `insert_limited` is gone; every program builds
its fields with their limits; and `apply_key` asserts, in debug builds,
that the field it edits has one. Stamped FIXED once that has passed a boot
test on `main`.

**In short:** A one-line text field knows how many characters it may hold:
the limit it was built with (`textline::field_holding`, `TextInput`'s own
capacity), which a paste from its right-click menu obeys. The keys that edit
it are told the limit again, as an argument
(`textline::apply_key(input, key, capacity, ..)`), and keep to it with a loop
of their own (`insert_limited`). Every program passes the same constant to
both today. If the two ever differ, typing and pasting by key stop at one
limit and the menu's Paste at the other.

**Where:** `apps/textline` (`apply_key`, `apply_key_on_clipboard`,
`insert_limited`), and the sixty-odd programs that call them, about half of
which build their fields without a capacity of the fields' own.

**Fix:** have `apply_key` type through the field's own capacity
(`TextInput::insert_text`), dropping its `capacity` argument and
`insert_limited`, and build every field with its limit (`field_holding`). The
field menu's hosts already do. Found by the review (Opus at xhigh) of lane E's
field-menu commits. It waits for a batch of its own because it changes every
program that edits a line of text.
