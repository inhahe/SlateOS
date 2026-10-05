## Data exporters: CSV/JSON/SQL injection in `netscan`, `credmanager`, `dbviewer` (FIXED)

Third pass of the "a config file is not a safe output format" audit, covering
the tabular exporters. Four distinct defects, all the same shape:

**`apps/netscan` did no CSV escaping at all.** This is the worst of the four
because the inputs are not the user's: a `hostname` comes from reverse DNS and
a `service`/`banner` from banner grabbing, so both are chosen by the *scanned*
host — on a scan, precisely the party with no reason to be trusted. A comma in
a hostname added a column and a newline added a whole row, letting a hostile
host forge result rows for machines that were never scanned. The hand-written
`"{}"` around the port/service columns was not a defence either: it never
doubled an internal quote, so a `"` in a service name closed the field early.
Its JSON export had the same holes plus a banner escaper handling `"`, CR and
LF but *not* the backslash — a banner ending in `\` produced `"...\"`, an
unterminated string that truncates the document.

**`apps/credmanager` left `tags` and `folder` raw** in the CSV (the only two
of nine columns not escaped), its `escape_csv` omitted `\r` from the trigger
set (RFC 4180 records are CRLF-terminated, so a bare CR splits the record for
most readers), and `serialize_backup` escaped *nothing* — vault name, entry
name, tag and folder names all interpolated bare. For a credential vault that
is the worst possible failure: a `"` in any name yields a backup file that no
reader can load, i.e. a silently unrestorable backup.

**`apps/dbviewer` escaped every value in all three exporters and no column
name in any of them.** The corollary this pass added to the audit question:
*audit the field names, not just the field values.* Column names are not
privileged data — `import_csv` takes them straight from the header line of a
file the user opened. Also `export_json`'s `s.replace('"', "\\\"")` (escaping
the quote but not the backslash, worse than useless for a value ending in `\`)
and `export_sql_inserts` interpolating table/column names as bare SQL
identifiers.

**`apps/dbviewer`'s importer could not read its own exporter's output.**
Found while fixing the above. `import_csv` split the header with a naive
`split(',')` and iterated `csv_data.lines()`, so a quoted field containing a
comma (header) or a newline (any record) was torn apart — even though
`parse_csv_line` underneath it was correctly RFC 4180-aware for data rows.
Fixed properly by replacing both with one record-level `split_csv_records`
that never splits on a line boundary before it knows whether it is inside
quotes. It also now trims only *unquoted* fields: quoting is how a writer says
the surrounding whitespace is data. Locked in by a round-trip test.

**Fixed** by adding `guitk::escape::csv_field` (RFC 4180, trigger set
`, " \n \r`) to the shared module, a local `sql_ident` in `dbviewer` (standard
double-quote identifier quoting), and routing all of the above through them.
Non-vacuity verified by breaking each of the nine defences alone; each failed
only its own tests.

**A testing note worth keeping.** Three of the new tests failed on first run
*because the tests were wrong, not the code* — each had counted a naive
substring. Correctly escaped output legitimately *contains* the payload:
`\", \"admin` contains `"admin`, a quoted CSV field contains a comma and a
newline, and a quoted SQL identifier contains a `;`. A test for an injection
defence therefore cannot use `contains`/`split`/`lines` — it has to decode the
way a conforming reader does. The fix in each case was a small escape-aware
scanner (`parse_csv`, `json_string_token_count`, `sql_statement_count`) living
beside the tests. This is the same trap as the GRUB `menuentry ` substring
count from the first pass; it has now appeared in all three passes, so treat
"count the tokens a parser would see" as the default shape for these tests.
