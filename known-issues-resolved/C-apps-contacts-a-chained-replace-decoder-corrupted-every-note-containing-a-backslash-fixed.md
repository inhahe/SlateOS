## `apps/contacts`: a chained-`replace` decoder corrupted every note containing a backslash (FIXED)

**Status: FIXED 2026-08-15** (lane C). Found while auditing the vCard/iCalendar
family during the output-escaping sweep. This is the same defect previously
fixed in `apps/reminders`, in its third instance, and this time the *correct*
implementation was already sitting in the neighbouring app.

`vcard_unescape` decoded with a chain of `str::replace`:

```rust
s.replace("\n", "\n")     // <-- runs first
 .replace("\,", ",")
 .replace("\;", ";")
 .replace("\\\\", "\\")    // <-- too late
```

`vcard_escape` correctly writes the two-character text `\n` (a backslash
followed by the letter n) as `\n`. The decoder then scans that for the
sequence backslash-n, finds it at offset 1, and emits a real newline. So
`C:\new` came back as `C:\`, a line break, and `ew`.

The trigger is ordinary content, not a crafted one: a Windows path, a regex, a
LaTeX fragment, a `\server\share` UNC name — anything a person might
reasonably paste into a contact's NOTE field.

**The corruption happens once, on the first load, and is then a fixed point** —
re-saving does not degrade it further. That is worth stating precisely because
it makes the bug *quieter* rather than milder: the damaged value is what gets
written back, so after a single load-and-save cycle the original text is gone,
and there is no accumulating drift to make the loss noticeable. A test that
looked only for unbounded growth would have passed.

Fixed with a single left-to-right pass that consumes the backslash and the
character after it together. Such a pass structurally cannot make this mistake,
because it never re-examines output it has already produced — the ordering
question that a `.replace()` chain has to answer correctly simply does not
arise.

Two things came out of the cross-check that are worth recording:

- **`apps/calendar::ics_unescape` was already correct**, single-pass, and
  carried a comment naming this exact anti-pattern. The same format family held
  one correct and one broken implementation of the same rules, a few hundred
  lines apart in a sibling crate — which is the duplication problem the
  `guitk::csv` extraction was about, showing up in a format that has not been
  extracted yet.
- **`vcard_escape` also passed a bare CR through untouched.** vCard has no
  escape for CR and its lines are CRLF-terminated, so a CR in a value ended the
  property line early and the remainder was parsed as a new property — a note
  could forge a `TEL:` line. Fourth instance of "the format cannot represent
  this value, so reject or sanitise": here it sanitises, because a CR in a text
  field means a line break, and a CRLF pair now yields one break rather than
  two.
