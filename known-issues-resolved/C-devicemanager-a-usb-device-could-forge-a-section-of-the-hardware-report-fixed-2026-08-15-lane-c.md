## devicemanager: a USB device could forge a section of the hardware report (fixed 2026-08-15, lane C)

`export_report` interpolated eight device-supplied strings raw — name, vendor,
type, hardware ID, location, and the driver's name, version and provider — into
a report whose structure is line breaks, `--- Section ---` headers and
two-space indentation.

What makes this one worth its own entry is where the strings come from. They
are not typed by the user; they are read off the hardware. A USB device chooses
its own product and manufacturer descriptors, and nothing in the descriptor
format constrains their content or forbids a line break. So a device that calls
itself

```
Mouse
--- Storage ---
  Fake Disk [OK] (ACME)
```

writes a whole forged section into the hardware report of any machine it is
plugged into — a report whose entire purpose is to be trusted when someone is
diagnosing that machine, and which is typically pasted into a bug report or
handed to whoever is helping.

There is no importer, which is worth stating precisely rather than using as a
reason to skip it: the absence of a parser does not make the output correct, it
only changes who is misled — here a person, or whatever they paste the report
into.

Fixed with a fold, not an escape. The choice follows from the reader: there is
nothing to undo an escape, so a literal `\n` in the output would be noise to a
human where a real newline is a forgery. Every control character becomes at
most one space, runs collapse, and edge space is dropped so a name padded with
spaces cannot appear to sit at a different depth in the report's indentation.

### flashcards' last lossy corner is closed

Migrating flashcards onto the shared `guitk::kv` was meant to be deduplication
— the fourth inline copy of the same escaper — but it also closed the deck
format's one documented limitation. `split_tags` trims each tag, which is the
right leniency for a hand-written `T: math, algebra`, but the trim reached the
*value*, so a tag of `" spaced "` came back as `"spaced"`. `kv` writes an edge
space as `\s`, which is not a space: the trim cannot find it, and the decode
happens afterwards. The trim still does its job — absorbing the layout of a
hand-written list — without being able to reach the data.

Worth noting as a general point: three of the four apps migrated onto the
shared escaper gained a fix they were not migrated for. Consolidating on one
correct implementation is not only less code; it retro-actively repairs every
corner each local copy had quietly given up on.

### A substring count is a `contains` in disguise

The first version of both devicemanager tests failed against the *fixed* code,
and the tests were what was wrong. They asserted
`report.matches("--- ").count() == baseline` and
`!report.contains("--- Forged ---")`.

But a correctly folded name still carries every character of its payload —
`--- Storage ---` is right there in the output, now harmlessly mid-sentence.
This is the same lesson already recorded for the escaping work ("count records,
never `contains`, because correctly escaped output legitimately *contains* the
payload") arriving in a disguise that got past it: a substring *count* looks
quantitative and structural, and is neither.

The guarantee a fold actually provides is positional, so the assertion has to
be too. Every interpolated field is preceded on its line by the report's own
indentation, therefore no field can begin a line, therefore none can *be* a
header. The tests now count lines that satisfy `starts_with("--- ") &&
ends_with(" ---")`, plus the report's total line count. Both survive breaking
each of the eight fold sites individually.
