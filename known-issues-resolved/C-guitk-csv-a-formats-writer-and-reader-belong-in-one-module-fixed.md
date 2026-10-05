## `guitk::csv`: a format's writer and reader belong in one module (FIXED)

`apps/spreadsheet` turned out to have the *identical pair* of defects
`apps/dbviewer` had: an `export_csv` whose quoting trigger set omitted `\r`,
and an `import_csv` that split records with `csv.lines()` before handing each
line to a perfectly correct, quote-aware field parser. Both apps could
therefore produce an export they could not themselves read back — a quoted
cell containing a newline was torn in half and the rest of its row dropped.

Two independent apps making the same two mistakes is the signal to stop
patching and restructure, so the CSV format now lives in one module,
`gui/toolkit/src/csv.rs`, holding **both** directions: `csv::field` (write)
and `csv::parse_records` (read). Keeping them adjacent is the point — the
whole bug class is a writer and a reader drifting apart, and it is much harder
to write a line-splitting reader thirty lines below an escaper that
deliberately emits newlines inside fields.

`csv_field` moved out of `guitk::escape` in the process. Escaping a CSV field
is not a standalone escaping problem the way XML or JSON escaping is; it is
half of a codec, and filing it under "escape" is what made it natural to write
the other half somewhere else. `escape` keeps a comment pointing at `csv`.

`Field { text, quoted }` reports whether the source spelled a field in quotes,
because the two apps disagreed on trimming and both were right: `dbviewer`
wants the lenient "trim a bare field" import convention, `spreadsheet` wants
cells verbatim. Quoting is exactly the writer's statement that the surrounding
whitespace is data, so `Field::trimmed_if_bare` lets a caller be lenient
without corrupting a deliberately-padded value. Locked in by a round-trip test
in each app plus `anything_written_can_be_read_back` in the module itself.

Both apps' local parsers were deleted rather than left in place; a weaker
second parser sitting in the file is the thing that gets reached for next
time.
