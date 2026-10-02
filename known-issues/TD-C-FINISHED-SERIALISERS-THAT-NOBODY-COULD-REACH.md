## TD-C-FINISHED-SERIALISERS-THAT-NOBODY-COULD-REACH -- PARTLY FIXED 2026-09-15

**In short:** the tree turned out to contain a large amount of completed,
tested file-format code that no program could call. Four apps now can. The
remaining 82 functions are listed by a scanner rather than by hand.

**Date:** 2026-09-15. **Lane:** C. **Status:** four done, the rest OPEN and
scanned. Third of a trio, after
`TD-C-WHAT-THE-FABRICATION-SWEEP-ACTUALLY-TAUGHT` and
`TD-C-THE-OTHER-HALF-PROGRAMS-THAT-SAY-TOO-LITTLE`.

### The observation

`apps/spreadsheet` said it in its own module doc, and had said it for a while:

> CSV import and export are implemented (`Sheet::export_csv`,
> `Sheet::import_csv`) and are not on that list, because **nothing can reach
> them**: they take and return a `String`, and this program has no file
> dialog, no command line and no clipboard beyond its own internal one, so
> there is nowhere for the text to come from or go.

That is an accurate description of a consumer with no producer, written by
somebody who had understood the situation completely. **It had sat there as a
description rather than as a task.** The serialiser was the hard part and it
was already finished; what was missing was twenty lines of picker.

`scripts/find-stranded-serialisers.py` asks the general question -- does a
crate define something that turns its data into text or bytes while having no
way to read or write a file -- and found **92 such functions across 37
crates**, including several complete interchange formats:

| app | format, already written |
|---|---|
| `contacts` | vCard, with CRLF joining per spec and `BEGIN:VCARD` block parsing |
| `calendar` | iCalendar, with `VERSION:2.0`, a `PRODID` and `X-WR-CALNAME` |
| `podcast`, `rssreader` | OPML |
| `musicplayer` | M3U |
| `dbviewer` | CSV, JSON and SQL inserts |
| `diagram` | JSON and SVG |

### What was done

Four doors, all on `apps/editor`'s pattern -- `FileDialog` plus
`safeio::write_str_atomically`, never `fs::write`, because `fs::write`
truncates the target *before* writing and an interrupted save leaves a
fragment where the user's only copy used to be.

* **`notes`** -- Ctrl+S writes the selected note as Markdown. No format was
  designed: a note is text and its title is a heading. Designing one would
  mean deciding how a notebook *tree* is represented, which is a much larger
  question than "write this note down".
* **`spreadsheet`** -- Ctrl+S and Ctrl+O, through the CSV that was waiting.
* **`contacts`** -- vCard.
* **`calendar`** -- iCalendar.

### What the doors needed that the formats did not

**Refuse to write nothing.** `contacts` will not write an empty book and
`calendar` will not write a header with no events. A zero-byte `.vcf` is
indistinguishable from a failed export afterwards, and an `.ics` holding only
`BEGIN:VCALENDAR` is *valid*, which is worse: it imports silently as nothing.

**Add, do not replace.** Importing a colleague's calendar should not discard
your own, and importing an address book is not a request to delete the one you
have. Duplicates are the duplicate finder's problem, and `contacts` has one.

**Bound the read, and say so when it bites.** All four cap at 8 MiB. The
message is front-loaded with `INCOMPLETE` because it lands in a
bounded-width status line and the clause that must survive an ellipsis is the
one saying the file is not all there. This matters more than it sounds:
`parse_ics` and `import_vcards` both stop at a truncation *without
complaining*, so a cut file simply yields fewer entries -- and **a calendar
missing an appointment looks exactly like a calendar that never had one.**

**Report the read, not the parse.** `import_csv` takes any text and fills
cells from it, so a file that is not really CSV produces cells rather than an
error. Saying "could not read" about a file that *was* read points the user at
the wrong thing.

**A round-trip test is not a conformance test.** `calendar`'s asserts the
written file starts with `BEGIN:VCALENDAR` as well as round-tripping, because
a round-trip alone passes against any format that is its own inverse,
including a wrong one. Same shape as the liveness-versus-correctness point in
`apps/benchmark`: varying the input and watching the output vary proves the
measurement is live and says nothing about whether it is right.

### Why this is a different kind of finding

The fabrication sweep removed claims. The silence sweep added explanations.
This one is a list of things that **already work and are one picker away from
being usable** -- unusually cheap leads, and every one of them would have been
walked past if `spreadsheet`'s module doc had not stated its own problem so
precisely.

**A precise description of a gap is not a fix, and it reads like one.** That is
the transferable part: `spreadsheet`, `credmanager`, `defrag` and
`systemrestore` all documented their own missing capability accurately, in the
source, and the accuracy is what made it look handled.

### TRIAGED 2026-09-17 — the 37 that remain hold no unblocked door

**In short:** the scanner now reports **37 functions across 20 crates**, not
the 80 the roadmap still claims. All 37 were read this pass, and not one of
them is a door waiting to be built. They fall into three groups, and the
useful part is that the *reason* differs — so "build the rest of the
doors" is not the next task, and was quietly turning into busywork.

**1. Not file formats at all (about half).** The scanner says so itself in
its own banner: `to_hex6(self) -> String` and `export_csv(&self) -> String`
are the same shape to it. `to_wire`, `to_header`, `to_rwx`, `to_algebraic`,
`from_extension`, `from_yaml_name`, `from_str`, `from_label`, `from_code`,
`from_names_entry`, `to_string_repr` (a cron expression) and `to_hex6`/
`to_hex8` are field and protocol formatters. `qrcode`'s `to_bytes` is the
clearest case: it is a method on the encoder's internal `BitBuffer`, an
accumulator of bits mid-encode, not an image writer. `colorpicker`'s
`save_to_palette` and `regextester`'s `save_to_library` save to memory, and
the latter says in its own doc that there is no way to type the name.

**2. Exports over data the program cannot gather (most of the rest).**
`netscan`, `speedtest` and `devicemanager` each depend on `appearance`,
`guitk` and `oswindow` and nothing else — no network, no device
enumeration. `systemrestore` likewise, so its snapshot tree is synthetic; its
`export_all`/`import_all` pair is complete and symmetric and would export
fabricated restore points. A door on any of these writes a real file with
invented contents, which is worse than no door: the file outlives the window
that would have told you it was a mock.

`soundrecorder` is the one already handled, and is the proof of that
reasoning: its `to_bytes` is a genuine WAV writer (RIFF, `fmt `, PCM), and
nothing in production calls `process_samples`, so a take recorded silence
while the clock climbed and auto-save reported writing. The 2026-09-15 fix
refuses to start a take there is no input for. Same shape, caught earlier.

**3. Blocked on an answer (one).** `credmanager`. `export_csv` writes every
password as clear text and `serialize_backup` writes a file it calls a backup
that contains no passwords at all. Which to offer is a policy call —
**C-Q25**.

**What this changes.** The remaining work behind this item is not door
building. It is (a) teaching the scanner to tell a file format from a field
formatter, which its banner currently delegates to the reader, and (b) the
I/O gap those crates share, which is the same one the dead-field triage
landed on: a program that cannot gather data has no data to export. Both are
bigger than this entry and neither is unblocked by finishing "the other 80",
which do not exist.
