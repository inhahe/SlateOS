## `apps/**` bug-hunt sweep, round 1: five live defects and two latent (lane C)
**Status:** FIXED 2026-08-20 — `5b4dd7731` (calendar), `80dd0a5f3` (slices),
`c989b21d0` (clipboard preview), `e1390abb1` (sticky-note columns),
`d5a1e7eb8` + `ae3946e33` (grid column counts), `253478bd3` (QR block structure).

The roadmap asks lane C to run bug-hunt sweeps over `apps/**` between
features; ~200 crates there have never had a systematic audit. This is the
first round's findings, filed closed because all five were fixed in the same
sitting. They are recorded rather than merely committed because the *shape* of
each recurs, and the next sweep should start by grepping for it.

### The shape, again: a proof that lives in a different statement than the code it justifies

All five are the same fault the `gui/**` lint sweep kept finding. A value is
checked in one statement and used in another, and in between, something makes
the check not mean what it looks like it means.

### 1. `apps/explorer` listed a fabricated date for every file older than 2000-03-01

`columns.rs::format_datetime` — the Date Modified column — carried a local
civil calendar that shifted the epoch to 2000-03-01 "to simplify leap-year
handling", and then, for anything *before* that epoch, did not compute a date
at all. It estimated one:

| | |
|---|---|
| year | `1970 + days / 365` |
| month | `day_of_year / 30 + 1`, clamped to ≤ 12 |
| day | clamped to ≤ 28 |

| Real date | Shown as |
|---|---|
| 1985-07-04 | 1985-07-09 |
| 1999-06-15 | 1999-06-23 |
| 2000-02-29 (a real leap day) | 2000-03-07 |
| 1970-01-01 | 1970-01-01 (the one it got right) |

**Why nothing caught it.** Every value it produced was *in range* — a plausible
month, a plausible day — so no clamp, assertion, or type could have flagged it.
The error grew with the file's age, and old files are exactly the ones a user
sorts by date to find. The crate's single `test_format_datetime` sampled a 2024
timestamp, which is on the correct side of the seam; the whole bug lived in a
branch no test entered.

The post-2000 path was arithmetically identical to Hinnant's, which is why the
damage was confined: `719468 + 11017 = 5 × 146097`, so 2000-03-01 is an exact
era boundary and the shifted form degenerates to the standard one above it.

### 2 & 3. `starts_with(q) && ends_with(q)` is not a bounds proof

A string of a *single* `q` starts with it and ends with it — the same byte
answering both tests. The guard passes at `len == 1`, and the `&s[1..s.len() -
1]` that follows becomes `1..0`, which panics.

| Crate | Trigger | Reachable by |
|---|---|---|
| `apps/ircclient` `CtcpMessage::parse` | `"\x01"` | a PRIVMSG trailing parameter, i.e. anything the server sends — a one-line remote client kill |
| `apps/installer` YAML scalar parser | a lone `"` or `'` | a typo in a hand-edited install manifest |

The installer's had a `wrapping_sub()` in it, which is the interesting part:
somebody silenced `clippy::arithmetic_side_effects` at the subtraction instead
of asking why it fired. That made the underflow *silent but not harmless* — it
produced `usize::MAX` and the slice panicked anyway, one line later and less
legibly. **A suppression that moves a panic rather than removing it is worse
than the warning was.**

Both now `strip_prefix`/`strip_suffix` in sequence, which structurally cannot
make the mistake: after the prefix is removed, the suffix is looked for in what
is *left*, and an empty string has no delimiter to end with. Grep
`starts_with(.*).*&&.*ends_with(` before believing any similar guard; as of
this sweep the remaining lane-C hits are all `filter`/`any` predicates that
never slice.

### `is_action` vs `parse`: two recognisers for one grammar

`CtcpMessage` had a second, independent ACTION parser (`starts_with("\x01ACTION")`
plus `&text[8..text.len() - 1]`). It disagreed with `parse()` three ways, all
reachable from the wire:

| Input | `action_text` said | `parse` said |
|---|---|---|
| `"\x01ACTION\x01"` | *panic* (sliced `8..7`) | `Action("")` |
| `"\x01ACTIONfoo\x01"` | `Action("oo")` — the hard-coded `8` assumed a space nobody checked for | `Unknown("ACTIONFOO", "")` |
| `"\x01ACTIONé \x01"` | *panic* — index 8 fell inside the `é` | `Unknown(…)` |

Fixed by defining `action_text` in terms of the same framing and verb split
`parse` uses, and `is_action` in terms of `action_text`. `parse` also folds case
with ASCII rules rather than `to_uppercase`, so the two cannot drift on
`"actıon"` (dotless i, which *does* uppercase to `ACTION`).

### 4 & 5. `String::truncate`/`insert`/`remove` panic off a character boundary

The byte/character confusion again, in the standard library's most
panic-prone corner. Found by grepping for `String` mutators that take an
offset, after `String::truncate` turned one up.

**`apps/clipmanager::ClipEntry::preview` — live, and a crash in a render
path.** Documented as "`PREVIEW_MAX_CHARS` total characters", implemented
entirely in bytes: it compared `out.len()` against the limit and cut with
`out.truncate(120)`. The clipboard holds whatever the user copied, so copying
a couple of sentences of Greek, Cyrillic, Hebrew, Arabic, CJK or emoji took the
clipboard manager down *while drawing its own list* — and because the entry was
already in the history, it recurred on every restart until the entry aged out.
Counting bytes was wrong even when it did not crash: 120 bytes of Japanese is
40 characters, so a preview that filled a row in English was cut to a third of
it. The one existing test used `"a".repeat(200)`; ASCII is exactly the input
that cannot fail, the same blind spot that let `apps/explorer`'s date bug live
behind a test that only sampled 2024.

**`apps/stickynotes::Note::insert_char`/`delete_char` — latent.** Both take a
byte offset and clamped it to the line's length and no further. Clamping stops
an offset running off the *end*; it says nothing about the *boundary*. Nothing
outside the tests calls either today, which is why the 2026-08-16 audit
recorded in `GUI-TEXT-INPUT-CURSORS-STEP-BY-BYTES` missed them: that sweep
looked for `cursor ± 1` and these have no cursor field, only a parameter whose
signature never said whether it meant bytes or characters. Now snapped back to
the boundary, matching `apps/editor::snap_to_boundary` and
`apps/markdowneditor::clamp_col`, and the unit is stated.

**Everything else in that class was checked and is correct**, mostly by having
already been fixed on 2026-08-16: `apps/editor` (`snap_to_boundary`),
`apps/markdowneditor` (`clamp_col`, `len_utf8` steps), `apps/paint` (scans for
the adjacent character), `apps/launcher` and `apps/jsonviewer` and
`apps/spreadsheet` (caret counts characters, converted at each use),
`apps/calculator` (`len() - last.len_utf8()`, which is a boundary by
construction), `apps/dbviewer` (a `Vec<String>`, so `Vec::truncate`),
`apps/netscan` (`char_indices().nth`), `apps/unitconverter` (ASCII-only input
filter, invariant documented at the site).

### A test-design trap worth remembering: 120 is divisible by 2, 3 and 4

The first draft of the clipmanager regression test used `"日".repeat(200)` and
friends — and **passed against the broken code**. `PREVIEW_MAX_CHARS` is 120,
which is a multiple of every UTF-8 width, so byte 120 of a solid run of
2-, 3- or 4-byte characters lands on a boundary by accident. A test of a
boundary bug has to *shift the grid*: the cases are now a single ASCII prefix
followed by a run, and each asserts `!is_char_boundary(PREVIEW_MAX_CHARS)` up
front so it cannot quietly stop testing the bug if the constant changes.

Generally: **a regression test for a "lands inside a character" bug must assert
that its input actually lands inside a character.** Otherwise the alignment is
a coincidence, and coincidences are not stable under refactoring.

### (6) `apps/colorpicker` divided by zero in one grid and not in its twin

`ColorPickerApp::render_history` computed how many swatches fit across the
window and then indexed its cells with `i % cells_per_row` over a loop bounded
by **the number of colors in the history**, not by `cells_per_row`. A window
narrower than one 28px swatch plus its padding makes that count zero and the
modulo panics.

`render_palette`, **sixty lines below in the same file**, computes the same
quantity and writes `.max(1)` at *both* of its divisions. The author knew — in
one of the two places — and neither site says why. Same shape as everything
above: a proof that lives in a different statement from the code it justifies.

Latent rather than live today, and only by accident: `self.width` is assigned
`WINDOW_WIDTH` once in `create()` and there is no `Resize` handler yet, so
`cells_per_row` is 18 forever. It goes live the moment resize lands.

Fixed in `d5a1e7eb8`, then folded into (7). Verified regressive: with the old
body restored, the new test panics at the modulo rather than merely failing an
assertion.

### (7) …and five other spellings of the same guard, one crate away from the answer

Finding (6) is not one site, it is the sixth transcription of one piece of
arithmetic. Every place in lane C's tree that asks "how many `cell`-wide
columns fit across this width":

| Where | Guard against zero |
|---|---|
| `gui/toolkit/src/grid.rs` `LayoutCache::compute` | `NonZeroUsize`, with the reasoning written out |
| `apps/worldclock` `render_grid` | `.max(1.0)` before the cast |
| `apps/charmap` `render_grid` | `.max(1.0)` before the cast |
| `apps/slides` sorter grid | `.max(1.0)`, **plus an `#[allow(arithmetic_side_effects)]` at each division** vouching for it from twenty lines away |
| `apps/charmap` recent/favorites strip | **none** — safe only because the cap is `cols * 3`, so zero columns made both loops `take(0)` |
| `apps/defrag` block map | `if cols > 0 { … checked_div … }` |
| `apps/colorpicker` `render_history` | **none at all** — finding (6) |

Seven sites, five spellings, one outright missing and one present-by-accident.
The toolkit already had the right answer — `NonZeroUsize`, with a comment
explaining exactly why the floor is one — and **kept it private**, so every app
wrote its own. That is the same asymmetry as the `tzrules` request filed the
same day (`requests/c-b-year-of-day-computes-the-month-and-day-and-throws-them-away.md`):
export one direction of a piece of arithmetic and the tree grows transcriptions
of the other, and one of them will be wrong.

`ae3946e33` adds `guitk::grid::columns_across(avail, cell, gap) -> NonZeroUsize`,
makes `LayoutCache::compute` call it, and routes colorpicker (both grids),
charmap (both grids), worldclock and slides through it. The floor of one is now
in the return type, so `i % columns` uses the `Rem<NonZeroUsize> for usize` impl
that *cannot* divide by zero — slides drops both `#[allow]`s and charmap drops a
`checked_div` and a `.max(1)`.

Each migration is an **exact identity**, deliberately, not a re-derivation:
worldclock's and slides' `(w - gap) / (cell + gap)` is
`columns_across(w - 2*gap, cell, gap)`, and charmap's gapless grids pass
`gap = 0.0`. The single visible change is charmap's recent strip, which now
shows one clipped column where it previously showed none — which is the
toolkit's documented floor, and the more useful rendering.

Two subtleties worth keeping:

- **The degenerate case is keyed off `cell`, not off the pitch.** A zero-width
  cell separated by an 8px gap would otherwise "fit" once per gap, counting the
  separators as though they were content. `LayoutCache` had this right
  (`if cell_width <= 0.0`) and the first draft of `columns_across` did not;
  the `columns_across_agrees_with_the_layout_cache` test is what pins it.
- **`inf as usize` saturates, it does not wrap**, so a zero pitch would have
  produced `usize::MAX` columns rather than a panic — non-zero, so no
  zero-check would have caught it, and a grid of `usize::MAX` columns is not an
  answer anyone wants.

**Not migrated: `apps/defrag`.** Its `if cols > 0` skips the whole block map
rather than clipping it — a different contract, and correct as it stands.
Migrating it would change what the user sees for no defect. Same rule as the
`(i + 1) % len()` sites below: divergence in *correctness* is the warrant for
extraction; divergence in *spelling*, over code that is already right, is not.

### (8) `apps/qrcode` generated unscannable codes for 16 of its 40 version/EC pairs

The worst of the round, and the one that shows the doctrine at full strength.

`get_version_info` returns a row of a transcribed spec table that **states the
same fact twice**: `data_capacity_bytes` is the published byte-mode capacity,
and `total_codewords`, `num_blocks` and `ec_codewords_per_block` imply it.
Nothing compared the two. Sixteen of the forty rows disagreed:

| Rows | Table said | Capacity implies |
|---|---|---|
| v5-Q, v5-H, v6-Q, v6-H, v8-M, v10-L | 2 or 3 blocks | 4 |
| v7-H, v9-M, v10-M | 2 or 3 | 5 |
| v7-Q, v8-Q, v8-H | 2 | 6 |
| v9-Q, v9-H, v10-Q, v10-H | 3 | 8 |

Every implied count comes out an **exact integer**, which is what identifies
the block column as the wrong one rather than the capacity column — had the
capacity been the typo, the implied block counts would have come out
fractional. The corrected values also match ISO/IEC 18004's own block table,
derived independently.

**Why nothing caught it.** This is not a panic and not a length error.
`encode_data_bits` padded to `total_codewords - ec_per_block * num_blocks` —
98 data codewords for v5-Q instead of 62 — and `apply_error_correction` then
added exactly enough EC to reach `total_codewords`, because the two errors are
the same quantity with opposite signs. **The symbol filled the matrix and
rendered correctly.** It was simply not the codeword layout any conforming
decoder de-interleaves, so it did not scan. Two of the three tests written for
this fix pass against the *broken* table; only the one comparing the table to
itself fails. Nothing short of a second opinion could have found it — there was
no crash, no overflow, no out-of-range value, and the rendered image looked
exactly like a QR code.

Reachable with the default EC level (M) at 123 bytes of payload and up, and
from **47 bytes at Q** — an ordinary URL.

Fixed in `253478bd3`: the sixteen counts corrected; `byte_mode_capacity()`
derives the payload size from the block structure and a test asserts it equals
every row's stated capacity; both halves of the encoder read the data codeword
count from one `data_codewords()` instead of each spelling out the subtraction.

**It was not the last of them (2026-09-25, lane E).** The second opinion this
finding asked for arrived as a test that reads each symbol back the way a
scanner does, from a layout written again from the standard
(`every_symbol_reads_back_as_what_was_encoded`). It found versions 7-10 with
no version information at all -- the data placed where the version belongs,
every later bit one place off, so the symbol was unreadable from 123 bytes at
M, the same threshold as above -- version 10's alignment centres at 52 instead
of 50, and the second format copy missing its bit 7. Fixed with the qrcode
rework (see `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`).
**The lesson is this finding's own, one layer down:** the tests checked the
codewords, and nothing checked the matrix a scanner reads them from.

**A lint suppression whose justification did not cover the case that bit.**
`apps/qrcode` allows `arithmetic_side_effects` and `indexing_slicing`
file-wide, with a comment explaining that "indices are computed from QR-version
metadata, all bounded by the matrix dimension." That is true of the matrix
pokes and Galois-field lookups it was written for, and was never true of
`total_data / num_blocks`, which divides by a *table column*. A file-wide
allow inherits the reasoning of the sites the author had in mind and extends
it silently to every site added afterwards — the same defect as finding (2)'s
`wrapping_sub`, one scope larger. `num_blocks` is now a `NonZeroUsize` and the
function returns `None` on a zero row rather than dividing.

### Deliberately not changed: the ~15 `(i + 1) % len()` sites

Every wrapping-index step in `apps/**` that divides by a runtime length was
checked, and **every one is guarded** — always in a different statement from the
division, which is the shape above, but the guard is genuinely there each time.
`startupmanager::field_count()` and `life`'s `Grid` dimensions look unguarded
but are constants behind a method and a constructor.

They were left alone on the sweep's own rule: **duplication alone is not the
warrant for extraction — divergence is.** These ~15 do not disagree with each
other; they all wrap a stale index by `%`. They *do* disagree with
`guitk::step::wrapping_after`, which clamps first (`step::wrapping_after(3, 9)`
is `0`; `(9 + 1) % 3` is `1`) — as does `apps/filediff`'s local
`wrap_next`/`wrap_prev`. So a migration to `guitk::step` would not be removing
a divergence, it would be *introducing* one into fifteen call sites at once, to
delete a `%`. Not done. If `guitk::step` ever grows a mod-wrap variant, that
changes; until then this note is why nobody should "tidy" these.

### Remaining backlog

`apps/**` opts into the workspace lints (all 142 crates do), but the defensive
five are `warn`, not `deny`, so a warning backlog accumulates uncounted — the
installer's `wrapping_sub` and `apps/backup`'s `days + 719_468` were both
firing silently for months.

**Measured 2026-08-21** (`cargo clippy --workspace --exclude kernel
--all-targets --keep-going --target x86_64-pc-windows-gnu`, counted with
`scripts/clippy-sites.py`, deduplicated by `(file, line, column, lint)`):

| Tree | Distinct sites |
|---|---|
| `apps/**` | **4986** |
| `net*/**` | 87 |
| `gui/**` | 1 |
| `textfmt` | 0 |

Within `apps/**` the defensive five are 4872 of the 4986 — **97.7%**:
`indexing_slicing` 2195, `arithmetic_side_effects` 1887, `unwrap_used` 617,
`expect_used` 117, `panic` 56. The remaining 114 are ordinary style lints.
Worst files: `indexer` 196, `terminal/pty.rs` 162, `unitconverter` 159,
`markdowneditor` 155, `paint` 136.

The shape of that table is the finding, not the total. The shared code — the
toolkit, the desktop, the extracted crates — is essentially clean, and the
200 application crates hold effectively all of it. That is not because the
apps are worse code; it is because **the shared crates are the only ones
anything ever pointed a linter at.** `gui/**` was audited during the
extractions that produced `guitk::grid`, `guitk::date` and `textfmt`;
`apps/**` has never had a pass at all.

Two cautions before anyone treats 4986 as a defect count:

- **It is an upper bound on suspicion, not a bug count.** Most
  `indexing_slicing` sites index a literal-sized array with a bounded loop
  counter. The sweep's five real defects were each found by *reasoning about
  a duplicated shape*, not by walking a lint list — none of the five would
  have been top of this ranking.
- **It includes test code, which is where the lints are least meaningful**
  (panicking on bad data is the point of a test). The workspace lint table
  allows the defensive five under `#[cfg(test)]` by convention, but that is a
  convention applied by hand, not a `cfg` in the table, so test sites are
  counted here. Splitting the figure by target is worth doing before using it
  to prioritise.
