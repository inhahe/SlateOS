## TD-C-THE-STRANDED-SERIALISER-COUNT-WAS-MOSTLY-NOT-DOORS -- TRIAGED 2026-09-15

**In short:** a checker has been reporting "38 serialisers with no way to reach
a file" across twenty apps, which reads as thirty-eight missing features. It is
not. Read one by one, about half are short text formatters that never wanted a
file, a third are real exporters over data the program does not have yet, and
the genuinely missing doors are now down to one or two.

**Date:** 2026-09-15. **Lane:** C. The scanner now prints each function's
signature so this triage does not have to be redone from the names.

**The three groups.**

| group | examples | what they need |
|---|---|---|
| formatters and parsers | `colorpicker::to_hex6(self) -> String`, `mediaconvert::from_extension(ext: &str)`, `procexplorer::to_rwx`, `ircclient::to_wire`, `chess::to_algebraic`, `terminal::to_child(&mut self, bytes: &[u8])` | nothing |
| real exporters, no real content | `netscan`, `speedtest`, `credmanager`, `systemrestore`, `soundrecorder`, `devicemanager` | the data first |
| genuinely missing doors | `clipmanager`, possibly `qrcode` | a door |

**`chess::to_algebraic` is the one to keep in mind**, because the name is the
whole trap: it sounds like a PGN exporter and it renders **one move** —
`"e4"`. A reader skimming names would have written a game-record feature that
the function does not support and the app does not need.

**The second group is deliberate and should stay deliberate.** Every crate in
it had its fabricated data removed earlier in this sweep: `devicemanager::new`
is empty and says so, `speedtest` "draws no random numbers now, because it
produces no numbers". A door there would export an empty file under a name
that promises content. **A door for content that isn't real is worse than no
door** — the same rule that kept `soundrecorder` and `systemrestore` on this
list on purpose.

**What was actually fixed today**, from this list: `apps/whiteboard` (and its
export was not SVG), `apps/filediff` (which could not open a file at all),
`apps/musicplayer` (`.m3u` open and save), `apps/slides` (HTML export). Four
doors, and one format repair that mattered more than the door did.

**The lesson is about the report, not the code.** The scan keys on
`to_*`/`from_*` with a byte-ish signature, which is the only thing it *can* key
on without reading the function — that is fine, documented, and was never the
problem. The problem was that the report printed a magnitude and a list of
bare names, giving the reader nothing to triage with, so the number read as a
backlog. It prints the signature beside each name now.

That is the same defect as the corpus line in `find-overstated-records`
repaired this morning, and the same one lane B caught in
`check-collapsed-messages` before that: **a count without the thing that would
let someone check it is read as coverage.** Three tools, one shape, one day.
