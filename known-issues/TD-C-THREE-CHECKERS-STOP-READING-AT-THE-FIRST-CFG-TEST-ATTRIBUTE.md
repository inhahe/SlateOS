## `TD-C-THREE-CHECKERS-STOP-READING-AT-THE-FIRST-CFG-TEST-ATTRIBUTE` (lane C, 2026-09-17)

**In short:** three of the tools that inspect Rust source cut the file off at
the first `#[cfg(test)]` they find, meaning to stop at the test module. But a
`#[cfg(test)]` also appears on individual test-only helpers, indented inside an
`impl`, hundreds of lines earlier -- and everything after that point was then
treated as test code and never examined. The tools reported confident numbers
about a fraction of each file.

**Measured over `gui/` and `apps/`:** 357 files contain the attribute; **57 are
cut early** by an item-level one. The worst is `apps/photomanager/src/main.rs`,
where the attribute is on line 392 and the test module on line 4640 -- **4,251
lines of a 6,100-line application invisible**. Seven lossy-decode calls were
hiding in the cut regions of four files.

**Fixed in `scripts/lossy-decode.py`** (2026-09-17): the cut is now the
module-level attribute, at column 0 where a test module is written, and when no
unindented one exists nothing is cut. VALUE went 54 to 61 over gui/apps, in 32
to 36 files; the seven newly visible sites are pre-existing and are recorded in
the baseline, because the alternative was every lane's push failing on a
backlog none of them created.

**CORRECTION, same day: only one script had this. My own entry was wrong.**

The paragraph this replaces named three scripts as sharing the bug, on the
evidence of `grep -l 'find("#[cfg(test)]")'`. Three files do contain that
string. Only one of them was doing the thing the string suggests:

| Script | What it actually does |
|---|---|
| `scripts/lossy-decode.py` | Had the bug. Fixed. |
| `scripts/audit-cli-fabrication.py` | **Already fixed, before I looked.** Its `strip_tests` brace-matches through `rustlex.live_code`; the match my grep found is in a docstring *describing* the old bug and why it was replaced. |
| `scripts/check-config-turn-guards.py` | **Uses it in the opposite direction.** The offset marks where tests *begin* and the checker scans from there, so an early item-level attribute makes it read too much, not too little. That is a false-positive risk, not a blind spot, and the repair is a different one. |

This is the same mistake as the entries above it about a cheap proxy read as
the answer: I grepped for a string and concluded something about how three
programs behave. Reading how each used it took two minutes and would have
saved filing a wrong claim.

**And the fix I wrote is the second-best one available.** `scripts/rustlex.py`
has `live_code`, which *blanks* every `#[cfg(test)]` item by brace matching
over noise-stripped text and carries on to the end of the file -- exact, where
my column-0 rule is a convention, and offset-preserving, so reported line
numbers stay true. Its docstring records this bug class being found and
measured in `userspace/` before I arrived: **35,706 lines, 7% of that lane,
invisible to a checker**, with `oils/src/interp.rs` cut at line 3,348 of
109,742 by a `#[cfg(test)] mod stderr_tee` helper.

So this was never a new class of bug. It was a solved one that `lossy-decode.py`
never adopted the solution to. What is new is only that it was still there, and
the measurement for `gui/` and `apps/`.

**Adopting it found four more.** Swapping `production_part` to `live_code`
took VALUE from 60 to 64 over `gui/` and `apps/`: the column-0 rule still
truncated at the *first* module-level attribute, so anything past a second one
stayed hidden. All four are file names drawn as labels -- three in
`apps/pdfviewer` (window title, recent-files row, tab) and one in
`gui/desktop`'s launcher caption -- each with the real path held beside it and
used for opening. They are exempted in the IGNORE table with that reason, not
baselined, which is what the checker's own failure message tells you to do.
The VALUE backlog is unchanged at 60.

**The floors did not catch it, and the reason generalises.** `rustlex`'s note
makes the point: losing 7% of a corpus still leaves far more findings than any
aggregate floor demands, and every file is still *opened*, so the file count
never moves. A floor on a total cannot see a hole in the middle of it.

**How to tell if a checker has this class of fault.** Not by reading its
output, which is the whole problem. Ask what it *skipped* and whether it says
so. A tool that reports "391 scanned" while silently reading a third of one
file is indistinguishable, from its output alone, from one that read all of
them. The three questions worth asking of any of them: what corpus does it
walk, what does it exclude within a file, and does it say which.

**The bug this uncovered, which is real and is not fixed.**
`apps/photomanager/src/main.rs` line 2986 does

    self.import_photo_with_exif(&path.to_string_lossy(), ...)

and that string becomes `Photo::file_path`, which is what the application
later opens to decode the photograph and what the library file stores. On this
OS a filename may hold any byte but `/` and NUL, so a name that is not UTF-8
becomes a path with U+FFFD in it -- **a name nobody can open**, saved to the
library as though it were the real one. The photograph then fails to decode
with "could not be read", pointing at a file that does exist under a name the
application has destroyed.

The proper fix is not at that line: `Photo::file_path` is a `String`, so the
damage is done by the type. It wants to be an `OsString`/`PathBuf`, with the
library format storing the bytes (the format already escapes, and
`gui/pathcodec` exists for exactly this). That is a real change through the
whole crate -- import, search, the library file, the thumbnail cache key --
and is worth doing properly rather than papering over at the call site.
