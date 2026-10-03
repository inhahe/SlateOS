## Almost no `apps/` crate opts into the workspace lints (lane C)

**Status: PARTIALLY FIXED 2026-08-16** (lane C) — the opt-in has landed
tree-wide; the warning backlog it exposed is what remains open. See
"What the measurement actually found" at the end of this entry; the original
report follows first.

**Status: OPEN 2026-08-15** (lane C). Noticed while checking whether
`clippy::arithmetic_side_effects` applied to a fix in `apps/kanban`. It does
not — because `apps/kanban/Cargo.toml` has no `[lints] workspace = true`.

Nor do 124 of the ~126 crates under `apps/`. `apps/jsonviewer` has it;
essentially nothing else does. So the workspace's `clippy::all = deny`,
`pedantic = warn`, and the four correctness lints `CLAUDE.md` specifically asks
for (`unwrap_used`, `expect_used`, `indexing_slicing`, `arithmetic_side_effects`)
are silently not enforced across the entire application tree — which is, not
coincidentally, where every one of the eighteen byte/character sites above
lived. `indexing_slicing` in particular would have flagged a good number of them
at the moment they were written.

The fix is mechanical (add two lines to each `Cargo.toml`) but not free: it will
surface a large backlog of warnings, and `clippy::all` at `deny` will outright
break crates that currently build clean. The right shape is to land the opt-in
crate by crate, fixing each crate's fallout as it goes, rather than as one
tree-wide commit that has to be reverted the moment anything is red. Worth
doing: a lint that is configured but not applied is worse than no lint, because
the workspace config reads as though the guarantee is in force.

### What the measurement actually found (2026-08-16)

The paragraph above guessed that `clippy::all = deny` would break crates, and
prescribed a crate-by-crate rollout on that basis. **The guess was wrong, and
it was wrong in the direction that mattered.** Rather than assume, the opt-in
was added to all 124 crates at once *as a measurement* and the whole tree run:

```
CARGO_TARGET_DIR=target-hl cargo clippy --keep-going --message-format=short \
    -p <all 141 apps packages> --target x86_64-pc-windows-gnu
```

Result: **5174 findings, 0 errors, in 120 of the 141 packages.** Nothing goes
red. Two independent reasons:

- Almost every finding is one of the four `warn`-level correctness lints
  `CLAUDE.md` asks for, not a `clippy::all` lint. The explicit
  `arithmetic_side_effects = "warn"` / `indexing_slicing = "warn"` in the
  workspace config *overrides* the group's `deny`, so those never error.
- `grep -rn 'D warnings|deny(warnings)|Dwarnings' scripts/ .cargo/ Cargo.toml`
  is empty. No build, script, or CI path in this tree turns warnings into
  errors, so a warning backlog cannot break anyone.

So the opt-in landed as **one tree-wide commit** after all. The crate-by-crate
advice was mitigating a risk that does not exist, at the cost of 124 commits
and of leaving the lints unenforced for however long that took.

What remains open is the backlog itself:

| Finding | Count |
|---|---:|
| `arithmetic_side_effects` ("arithmetic … unexpected side-effects") | 3161 |
| `indexing_slicing` — "indexing may panic" | 1796 |
| `indexing_slicing` — "slicing may panic" | 123 |
| `expect_used` on an `Option` | 8 |
| assorted `pedantic`/style (Debug formatting, manual `String::new`, …) | 86 |

Worst crates: `editor` 500, `markdowneditor` 348, `explorer` 264, `paint` 234,
`backup` 198, `imageviewer` 193, `indexer` 149, `connect4` 123, `rssreader`
117, `spreadsheet` 106. Twenty-one packages are already clean, and
`contacts`, `simon`, `diskcleanup`, `pomodoro` and `radio` have exactly one
finding each.

**Two of those counts were wrong, in opposite directions** (found 2026-08-16
while sweeping them; see the entries below). `contacts` really did have one
*production* finding — and following that one warning found two user-visible
defects. `simon` had **35**: the crate blanket-allowed five defensive lints at
its crate root, so every count ever taken for it was measured against a
suppressed baseline, and one of the 35 hidden findings was the entire game
being a fixed four-step cycle. `apps/hangman` (5 such allows) and
`apps/jsonviewer` (2) suppress the same way and have not been re-counted, so
**treat any per-crate figure above as a lower bound until that crate's root has
been checked for `#![allow]`** — a count taken through a suppression measures
the suppression, not the crate.

The 1919 `indexing_slicing` findings are the ones worth attacking first: that
is precisely the lint class that would have caught the eighteen byte/character
sites recorded above at the moment they were written. `arithmetic_side_effects`
is the larger pile but the lower yield — most of its 3161 hits are loop
counters and layout arithmetic on values that cannot overflow, and rewriting
those to `checked_*` buys correctness theatre rather than correctness. Take
`indexing_slicing` to zero first; then judge `arithmetic_side_effects` on
whether a per-crate `allow` with a justification beats 3161 mechanical edits.

**The single largest cluster is one line repeated, and it is a false positive —
do not "fix" it.** `kernel/src/syscall/dispatch.rs` accounts for **329** of the
`indexing may panic` findings (measured 2026-08-30, lane A), ~17% of the whole
`indexing_slicing` backlog. Every one of them is the syscall-table registration
`handlers[SYS_FOO as usize] = Some(handlers::sys_foo);` inside
`build_v1_table()`.

That function is a **`const fn`**, and its only caller is
`static V1_TABLE: SyscallTable = build_v1_table();` — so the whole table is
evaluated **at compile time**. An out-of-range syscall number there is a
*build error*, not a runtime panic. The lint is reporting a panic that cannot
occur, because there is no run time in which it could.

**The obvious refactor would make the kernel less safe, not more.** A
`register(&mut handlers, SYS_FOO, f)` helper storing through `get_mut()` — the
shape the three patterns above would suggest — converts a condition currently
caught by `rustc` before the image is linked into one handled at run time,
where the honest options are to panic in early boot or to silently leave a
syscall unregistered. Trading a compile error for either is a regression.
(`get_mut` is also not usable in a `const fn` today, so the helper would have
to abandon compile-time construction to exist at all.)

**Done 2026-08-30 (lane A):** a scoped `#[allow(clippy::indexing_slicing)]` on
`build_v1_table`, carrying that reasoning as its comment — the same "an `allow`
with a justification beats N mechanical edits" judgement the paragraph above
reserves for `arithmetic_side_effects`, except the justification here is
unusually strong: the compiler already proves the property the lint asks for.

Measured before and after: kernel warnings **17999 → 17670**, a drop of exactly
329, with **0** `indexing may panic` findings left anywhere in `dispatch.rs`.
That residual zero is worth more than the count: the allow is scoped to the one
function, so it could not have covered `dispatch()` — which indexes with a
number that *came from userspace*. Zero there means that path was already
bounds-checked (`if idx >= MAX_SYSCALL_NR`, dispatch.rs:705) rather than
quietly suppressed. Deliberately not a module-level allow, for exactly that
reason.

**So the repo-wide `indexing_slicing` figure quoted above is stale in the
useful direction: the real debt is ~1590, not 1919.** Anyone sizing the
remaining per-crate sweep off 1919 is overestimating it by ~17%.

This was also why adding a syscall *raised* the warning count by one per number
registered — three, for `SYS_FS_UNLINKAT_PINNED` / `_FSTATAT_PINNED` /
`_GETDENTS_PINNED` in §647. It no longer does.

### Sweep progress: `editor` 500 → 80, `indexing_slicing` 0 (2026-08-16)

The worst crate is done. `apps/editor` went from ~200 `indexing_slicing`
findings to **0** (total warnings 500 → 80, all of them
`arithmetic_side_effects`); tests 108 → 119, all passing. Commits `a0dcac89a`
(`highlight.rs`) and `64998fde2` (`main.rs`, `syntree.rs`).

**The sweep was not cosmetic — it found four reachable bugs**, listed
separately below. That is the answer to "is this lint class worth the effort":
one crate's worth of it turned up four defects that no existing test caught,
in code that had been reviewed and shipped. The remaining worst crates are
`markdowneditor` 348, `explorer` 264, `paint` 234, `backup` 198,
`imageviewer` 193, `indexer` 149.

Three patterns did most of the work and should be reused:

- **`i < len && bytes[i]` states the bound twice**, and the two drift. Replace
  with an `Option`-returning accessor (`at`, `starts_with_at`,
  `scan_to_delimiter`) so the bound is stated once, in one place.
- **A `Vec` plus an index is an invariant expressed as a convention.**
  `documents: Vec<Document>` + `active_tab: usize` became a `Tabs` type whose
  first document is a plain field, so "there is always a document open" is
  what the type says rather than what five call sites assumed.
- **A stale index handed back across a rebuild is the real hazard**, not the
  in-bounds access. `SyntaxTree::node`, `Document::line` and
  `Document::replace_in_line` all now return `Option`/`bool` rather than
  trusting an index a caller has held since before the last edit.

Test modules carry `#![allow(clippy::indexing_slicing, clippy::unwrap_used,
clippy::panic)]` — a test that indexes out of range should fail loudly and
point at the line that did it.

### Sweep progress: `markdowneditor` 348 → 40, `indexing_slicing` 0 (2026-08-16)

Second worst crate done, and the same three patterns did the work again.
`apps/markdowneditor` went from 38 `indexing_slicing` findings to **0** (total
warnings 348 → 40, all `arithmetic_side_effects`); tests 192 → 199. Commit
`2a5c23082`. Two more reachable bugs fell out of it, recorded below.

The tab pattern was fixed **structurally rather than a second time**. Rather
than repeat the editor's `Tabs` fix in markdowneditor's `documents:
Vec<Document>` + `active_doc: usize`, the editor's local `Tabs` was
generalised to `guitk::tabs::Tabs<T>` and the local copy deleted, so both
editors — and the next app with a tab bar — share one type whose first element
is a plain field. Non-emptiness is now a property of the type, not a
convention eight index expressions relied on. `Tabs<T>` has 8 tests of its own
and a doctest; guitk 701 → 709.

### `textfind`: the fold-then-index defect family, swept tree-wide (2026-08-16)

The editor's bugs 2, 3 and 4 above are not an editor problem — they are what
*every* hand-written case-insensitive search in this tree looked like. A sweep
found six more instances across five apps, all fixed in commit `59b097746` by
routing them through `guitk::textfind`, which folds incrementally while
walking the real string and therefore only ever returns offsets into it:

| App | Site |
|---|---|
| `radio` | stations, genres, favourites, sleep timer, recording, spectrum | a caller. **The compiler already knew**: with `main` never calling the private `render`, dead-code analysis reported the whole UI -- every palette colour, `PlayState::Playing`, three of four `Screen`s, every `SleepTimer` duration -- and `#![allow(dead_code)]` at the top of the file said not to mention it | `FRAME_TICK` while playing or a sleep timer runs |
| `ircclient` | IRC parsing, channels, users, slash commands | anything to type with. `input_text` was drawn by the renderer and written by nobody, and `input_history`/`input_history_idx` beside it were written once in `new` and never touched | `None` -- messages come from a server, not a timer |
| `whiteboard` | drawing tools, layers, pages, undo, sticky notes | events. `on_canvas_press`/`move`/`release`/`scroll` and `start_pan` were all written and none was called, and `Tool::shortcut` named a letter per tool that nothing dispatched on | `None` -- nothing on a board moves on its own |
| `photomanager` | albums, EXIF, ratings, tags, smart albums, duplicates | any input at all, and a scroll offset: the grid cut itself off at the window edge with no offset to move, so a library of four hundred photos showed the first screenful and hid the rest for good | `SLIDESHOW_TICK` while a slideshow runs |
| `videoplayer` | playback, playlist, chapters, subtitles, equalizer | any input at all -- and it drew a **Keyboard Shortcuts** tab listing thirty-two keys, none of which were bound to anything, because the file did not import `guitk::event` | `FRAME_TICK` while playing or a message is expiring |
| `podcast` | subscriptions, playback, downloads, queue, search, OPML | any input at all. The file did not import `guitk::event`: there was no key handler and no click handler, so every one of those features was reachable only from a test | `PLAYBACK_TICK` while playing or downloading |
| `ebook` | `find_all_matches` |
| `pdfviewer` | `PdfDocument::search` |
| `spreadsheet` | `case_insensitive_replace`, `SearchState::find_all` |
| `filediff` | `SearchState::search` |
| `rssreader` | `extract_snippet`, `search_articles`, `discover_feeds`, `extract_attribute`, `filtered_article_indices` |

Every one of them had all three defects: offsets taken from a `to_lowercase()`
copy, a match length taken from the needle rather than from what matched, and
a scan resuming one byte past each match's *start* so matches overlapped. Each
now has a regression test, and each test was verified by reverting the fix and
watching it fail.

**The lesson for future work: a case-insensitive search written by hand is
wrong.** `to_lowercase()` is not length-preserving (Turkish `İ` U+0130 is two
bytes and folds to three), so the copy's offsets and the original's diverge at
the first such character, and the divergence ends in a slice inside a
character — a panic. Call `textfind`; do not write the loop.

This was a targeted pass at one defect family, not a full sweep of those five
crates; their remaining `indexing_slicing` counts are `rssreader` 93,
`spreadsheet` 42, `filediff` 17, `ebook` 16, `pdfviewer` 13. (`rssreader`,
`spreadsheet`, `filediff`, `ebook` and `pdfviewer` have since all been swept
to zero, so this list is now closed.)

### Sweep progress: `explorer` 264 → 55, `indexing_slicing` 0 (2026-08-16)

Third worst crate done. `apps/explorer` went from 125 `indexing_slicing`
findings to **0** and 264 total warnings to 55; every `unwrap_used`,
`expect_used`, `float_cmp` and `duration_suboptimal_units` finding is gone too.
Tests 181 → 187, plus 5 new in `textfind` (12 → 17). Six reachable bugs fell
out of it, recorded below.

Pattern 2 — *a `Vec` plus an index is an invariant expressed as a convention* —
did the bulk of the work again, in its most extreme form yet. `thumbs.rs` held
a `Vec<u8>` of ARGB pixels alongside a `width` and `height`, and **115 index
expressions** each independently restated the relationship between the three.
Three of those 115 proofs were wrong. Replacing the triple with a `Canvas` type
whose `set`/`fill_rect` clip and whose `get` returns `Option` took all 115 to
zero and made the three bugs unwritable rather than merely fixed.

A consequence worth naming: `Thumbnail::is_valid()` existed but was never
called at construction, so an invalid thumbnail could be built and only fail
later. `Canvas::into_thumbnail` is now the only way to build one, and it cannot
build an invalid one, so the check is a property of the type.

The 55 that remain are all `arithmetic_side_effects`, and every one was read:
each is guarded by a proof stated within a few lines (a `count > 0` before a
division, a `len == 0` early return before `len - 1`, a `.take(16)` bounding a
shift). None is reachable. Whether that lint earns its keep tree-wide — it is
3161 findings — is still open; see the note under `editor` above.

### `textfind::compare` — the fourth member of the fold-then-index family (2026-08-16)

The `textfind` sweep above covered *searching* case-insensitively. Sorting
case-insensitively is the same mistake with the same cause, and the sweep found
it in `explorer`. `textfind` gained `compare(a, b, case) -> Ordering`, which
folds lazily with `char::to_lowercase` and stops at the first differing
character.

The hand-written spelling it replaces, `a.to_lowercase().cmp(&b.to_lowercase())`,
is wrong in a way the search bugs are not — it does not panic, it is merely
ruinous: a comparison is what a sort calls `n log n` times, so ordering a
directory of ten thousand names allocates a quarter of a million strings to
answer a question that needs none. It also lets a list that is *filtered* by
`textfind::contains` and *sorted* by hand disagree with itself about which
names are the same.

`compare` returns `Equal` for two different strings that fold alike
(`README`/`readme`, `İ`/`i` + U+0307), which is the right answer to the
question asked but means it must be tie-broken with `a.cmp(b)` before it backs
an `Ord` impl — otherwise `cmp` and `==` disagree, which is exactly the bug
found in `explorer` below. That requirement is documented on the function.

### Sweep progress: `paint` 234 → 139, `indexing_slicing` 0 (2026-08-16)

Fourth crate, and the one the shared `Canvas` was extracted for. `apps/paint`
went from 89 `indexing_slicing` findings to **0**, 20 `float_cmp` to 0, and 234
total warnings to 139. Tests 155 → 158. Two reachable panics fell out, recorded
below.

The bulk of it was one deletion: `PixelBuffer` — 222 lines, the third
independent implementation of `Vec<u8>` + width + height in this tree — is gone,
replaced by `guitk::canvas::Canvas`. The conversion was almost mechanical (85
type references, 147 accessor calls, 16 field accesses the compiler found for us
because `Canvas`'s fields are private) and it took the crate's whole pixel-buffer
lint surface with it.

What remained after that were four smaller instances of the same two patterns:

- **Pattern 1, the bound stated twice.** `decode_bmp` proved `data.len() >= 54`
  in one statement and then restated it at twenty-two byte indexes; a
  `header_field::<N>(data, at)` helper states it at the read. `windows(2)`,
  `chunks_exact(2)` and `pts.iter().zip(pts.iter().cycle().skip(1))` replaced
  three hand-written index pairings in the polygon code, each of which had
  encoded "and the next one" as arithmetic guarded from a distance.
- **Pattern 2, parallel arrays.** The colour-picker dialog held
  `slider_labels`, `slider_values` and `slider_colors` plus a hard-coded
  `0..4` — four places that must agree on how many sliders exist, with nothing
  checking that they do. One array of `(label, value, colour)` rows makes
  adding a channel a single line that cannot half-happen. The OK/Cancel buttons
  had the same shape at two elements.

The 139 that remain are all `arithmetic_side_effects` and every one was read:
Bresenham and midpoint-ellipse integer stepping, brush-radius squares, and
run-length spans whose endpoints both come from the same loop. Each is guarded
by a proof within a few lines. None is reachable.

### Sweep progress: `imageviewer` 155 → 9, `indexing_slicing` 0 (2026-08-16)

Fifth crate, and the one the new `byteread` crate was written for. `apps/imageviewer`
went from 77 `indexing_slicing` + 54 `slicing` findings to **0**, and from 102
`arithmetic_side_effects` to 9. Tests 97 → 99. One reachable panic fell out,
recorded below.

This crate is where the "bound stated twice" pattern was densest in the whole
tree: 112 of the 131 index findings were in `src/video.rs` alone, in the MP4,
Matroska and AVI parsers — all of them reading at offsets taken *out of the file
being parsed*. `byteread` (see `byteread/src/lib.rs`) exists so each of those
reads states its own bound; the conversion removed 468 lines and added 345.

Three structural results worth naming, because they are the reason the count
fell so far rather than merely moving:

- **`box_payload` + `descend`.** The MP4 box tree was walked by hand at five
  levels (moov, trak, mdia, minf, stbl), each level repeating six lines of
  `offset.checked_add(8)` / `.checked_add(size)` / `.min(parent.len())` /
  `if start >= end { return None }` / `&parent[start..end]`. Two helpers replaced
  all five. `extract_stsd_codec` went from 45 lines to 5. The important part is
  not the line count: five hand-written clamps are five chances to clamp to the
  wrong parent, and now there is one.
- **`EbmlScan`.** The Matroska side had five near-identical byte-scan loops
  (`for i in 0..limit { if data[i] == ID && data[i+1] == ... }`), each re-deriving
  the payload offset from a VINT length and each with slightly different
  give-up behaviour — one `break`ed on the first hit even if it was garbage,
  one `return`ed `None` from the whole function if a length was not 4 or 8.
  One iterator replaced them, and the give-up behaviour is now uniform and
  strictly more forgiving: a hit whose length does not decode is *skipped*
  rather than ending the scan, because such a hit was never an element. That is
  a real behaviour improvement — a Matroska file with a stray `0x4489` byte
  before its real Duration used to report no duration at all.
- **`parse_jpeg_dimensions` walks instead of indexing.** JPEG is the one format
  here where offsets are genuinely sequential — each segment's length is read
  from the segment before it — so it now uses `byteread::Reader` rather than a
  hand-carried `idx`. This also fixed a latent non-panicking bug: a segment
  length below 2 (a length field counts itself) left `idx` unadvanced, and the
  loop only escaped by accident, because the next byte examined was the zero
  high byte of the bogus length. It now stops.

The 9 that remain are all `arithmetic_side_effects` and every one was read: two
playlist/gallery `(i + 1) % len` pairs behind `is_empty()` guards and their
`-= 1` mirrors behind `== 0` guards, a `/ timescale` behind `if timescale > 0`,
a `(x / 16) + (y / 16)` checkerboard, and two `min`-clamped list ranges. Each is
guarded by a proof within three lines. None is reachable.

Two arithmetic sites were *not* left alone, because rewriting them made the
proof shorter than the argument for it: `decode_ebml_vint`'s
`(1u8 << (8 - len)) - 1` is now `u8::MAX.checked_shr(len)` (a width of 8 leaves
no value bits, and shifting a `u8` by 8 is not a shift), and the slideshow's
`elapsed_ms += elapsed_ms` — the crate's only unbounded accumulator, fed by a
caller-supplied duration — is now `saturating_add`.

### Sweep progress: `connect4` 174 → 0, all lint classes (2026-08-16)

Sixth crate, and the first to reach **zero warnings of every class** —
101 `indexing_slicing`, 69 `arithmetic_side_effects`, 3 `unwrap_used` and 1
`slicing` all gone. Tests 100 → 108. Non-test code shrank 941 → 901 lines
*while gaining* the doc comments that explain the new invariants.

No **reachable** panic fell out of this one, and that is worth stating plainly
rather than dressing two latent defects up as live bugs. Both defects below are
real and both are now proven by tests that fail against the old code — but each
was reachable only from a caller that does not exist, so neither shipped:

- **`undo_drop` indexed `heights` with an unchecked column.** `can_drop(col)`
  tested `col < COLS`; `undo_drop(col)` opened with `self.heights[col] == 0`
  and tested nothing. Every caller passes a column from `valid_moves()`, so it
  was never reached. Reverting the fix makes
  `an_off_board_column_is_declined_rather_than_indexed` panic with `index out
  of bounds: the len is 7 but the index is 7`.
- **The AI dropped and undid as two unpaired statements**, discarding both
  `Option`s. Had `drop_piece` ever been refused, the following `undo_drop`
  would still have run and removed a piece a *different* move had put there.
  `valid_moves()` filters full columns, so it was never refused. Reverting the
  fix makes `with_move_on_a_full_column_runs_nothing_and_undoes_nothing` fail
  with the board's top-of-column-0 piece missing.

The structural work was pattern 1 in a form not seen in the earlier crates:
**the same bound written out eight times, four of those with the offsets
spelled into the indices.** `has_won` and `evaluate_board` each contained four
hand-written nested scans — horizontal, vertical, and both diagonals — with
bodies like `grid[row][col] == p && grid[row+1][col-1] == p && grid[row+2]
[col-2] == p && grid[row+3][col-3] == p`, guarded from a distance by
`for col in 3..COLS`. `find_winner` had a fifth copy of the guards, and
`check_line` a sixth of the stepping. One `DIRECTIONS` table plus a
`line_cells(row, col, dr, dc) -> Option<[(usize, usize); RUN]>` replaced all of
them; `all_lines()` yields the 69 runs that fit on a 7x6 board and every scan
now reads from it.

Two consequences beyond the lint count:

- **`has_won` and `find_winner` can no longer disagree.** They were independent
  hand-written scans answering the same question — one drives the AI's search,
  the other decides the game the player sees — and nothing checked that they
  agreed. They now share `all_lines`, and
  `has_won_and_find_winner_agree_across_a_played_out_game` walks a full game
  asserting it at every position.
- **`with_move(col, piece, f)` replaced the drop/undo pair** at all four AI call
  sites, so "these two calls must be paired" stopped being a convention the
  caller had to keep and became something the caller cannot get wrong. It also
  collapsed `ai_best_move`'s two identical win/block loops into one, and
  `minimax`'s duplicated maximizing/minimizing bodies into one.

The `arithmetic_side_effects` count reached zero here without contortion, which
is a useful data point for the open question of whether that lint earns its keep
tree-wide: the game-logic arithmetic was all genuinely `saturating_*` or
`checked_*` in meaning, and the float layout arithmetic the lint does not flag.

### Sweep progress: `rssreader` 112 → 0, all lint classes (2026-08-16)

Seventh crate, and the second to reach **zero warnings of every class**. Tests
152 → 162. Four defects fell out, all recorded below, and this is the crate
that named a **new failure class for the sweep: unbounded work driven by remote
data.**

The six patterns the sweep had been finding until now are all about a bound
that is *stated* somewhere and then not honoured at the point of use. This
crate has those too — but three of its four defects are the opposite shape:
**no bound was ever stated at all**, because the quantity being bounded is not
an index. A recursion depth, a loop trip count and a stack frame are not things
`indexing_slicing` looks at, and none of the three failures is a `Result` the
caller could have handled:

| What was unbounded | Set by | Failure |
|---|---|---|
| XML nesting depth | the feed's bytes | stack overflow → process abort |
| calendar year loop | a `<pubDate>` field | hang |
| `wrap_text` break loop | a word's length in an article body | quadratic time |

An RSS reader is the first crate in this sweep whose *entire input* is remote
and unauthenticated — the user subscribes to a URL, and everything after that
is the publisher's choice. That makes "how big can this get?" a security
question rather than a robustness one, and the answer was "as big as the
publisher likes" in three places.

Structurally, the parser rewrite is the same move that `byteread` was for
`imageviewer`: **six cursor primitives** (`rest`, `peek_at`, `looking_at`,
`skip`, `eat`, `take_past`) now carry every read of the input, each stating its
bound at the point of the read. The methods above them used to restate it —
`if self.pos + 3 < self.input.len() && self.input[self.pos] == b'<' && …`,
which is one bound written twice, several statements apart, and in two cases a
nine-byte slice guarded by `pos + 8 < len`. That is correct (it implies
`pos + 9 <= len`) but not in a form a reader can check against the slice beside
it. Five methods collapsed to one line each on top of the primitives.

The calendar is now two closed forms (Howard Hinnant's civil-from-days and
days-from-civil) instead of two year-by-year loops, pinned by
`the_two_calendar_directions_are_inverses_over_the_whole_year_range`, which
round-trips all 385,536 dates from 1970-01-01 to 9999-12-31 in 0.08 s and also
asserts monotonicity and the rejection of each month's `last + 1`. Removing the
loops fixed correctness as well as termination: `2023-02-29`, month 13, day 0,
hour 25 and minute 60 used to roll silently into the next real date, and now
each is refused.

Two `#[expect(clippy::arithmetic_side_effects, reason = "…")]` remain, both on
the Hinnant forms, and both reasons carry the proof: the operands are bounded
three lines above by the era decomposition and by `YEAR_RANGE`. Saturating
arithmetic there would return an `Option` no input can make `None`, which is a
worse thing to hand a reader than a stated bound.

### Sweep progress: `spreadsheet` 104 → 0, all lint classes (2026-08-16)

Eighth crate, and the third to reach **zero warnings of every class**, across
`--all-targets`. Tests 194 → 207. Four defects fell out — two of them
user-reachable crashes, both recorded below — and the crate confirms that the
class `rssreader` named (**unbounded work driven by input**) is not specific to
remote data: a spreadsheet's formulas are the *user's* text, and a stack
overflow takes the workbook down whoever typed it.

Five structural changes carried most of the 104:

| Was | Is | Warnings closed |
|---|---|---|
| `b'A' + col`, `col_char - b'A'`, `MAX_COLS = 26` — one fact, three statements | `COLUMN_LETTERS: &[u8; 26]`, read in both directions; `get`/`position` *are* the bound | 6 |
| `Vec<Sheet>` + `active: usize`, with "there is always one sheet" as a convention | `SheetBook { head: Sheet, tail: Vec<Sheet>, active }` — sheet 0 is a field, so it cannot not exist | 14 |
| `CellRange { pub start, pub end }`, ordering guaranteed only by whoever called `new` | `mod cell_range` + accessors; `col_count` may subtract because nothing else can write a corner | 26 |
| `text: String` + `cursor_pos: usize`, the number meaning bytes in four places and characters in one | `mod edit_buffer` — the caret is characters, `byte_of` is the only conversion | 6 |
| the on-screen rectangle of a cell range, written out three times | one `range_rect` closure | 12 |

The two module splits are the same lesson twice, and it is worth stating
plainly because it cost a wrong doc comment before it was noticed: **making a
field private inside a single-file crate changes nothing.** Privacy in Rust is
per-*module*, a `main.rs` is one module, and a module can always see its own
privates. The `mod { … } pub use …` wrapper is what actually enforces the
invariant — the first attempt at `CellRange` produced zero compile errors and a
comment claiming an enforcement that did not exist. Wrapping it produced the 26.

Other findings worth keeping:

- **`sort_by_column` recovered a column index by searching for a pointer.** The
  inner loop ran `row_data.iter().position(|(_, c)| std::ptr::eq(c, src_cell))`
  to find the index the enclosing `for` already had — quadratic, and with an
  `.unwrap_or(0)` that would have written the cell to column A on a miss.
- **`f64` integrality was tested as `n == n.floor() && n.abs() < 1e15`, twice.**
  The `1e15` was a hand-picked stand-in for "small enough that `as i64` will not
  saturate", which is off by three orders of magnitude from the real bound. Now
  one `whole_number(f64) -> Option<i64>`, which tests the range against 2^63
  explicitly — because `as i64` saturates, and the saturated value at the
  positive end round-trips back to the same `f64`, so a round-trip test alone
  would accept 2^63 and print it as 2^63 - 1.
- **One `#[expect(clippy::float_cmp)]` remains**, inside `whole_number`, and the
  reason carries the argument: whether a value survives the round trip through
  `i64` is an exact question, and an epsilon there would print `0.5` as `0`.
- **`CellError` gained `TooDeep` → `#DEPTH!`.** Cycles are detected exactly, by
  the path of addresses being visited, so a depth failure is never one; the
  depth backstop used to report `#CIRC!`, which pointed at a cycle that was not
  there.

### Sweep progress: `simon` 35 → 0, all lint classes (2026-08-16)

Thirteenth crate, eighth to reach **zero warnings of every class** across
`--all-targets`. Tests 106 → 110.

The table near the top of this file credits `simon` with **one** finding. It
had **35**, because the crate root carried five `#![allow(clippy::…)]` lines —
`unwrap_used`, `expect_used`, `panic`, `indexing_slicing`,
`arithmetic_side_effects` — and a lint that is allowed does not appear in a
count of lints. Twenty-two of the 35 were in the game logic rather than the
tests, and one of those twenty-two was the entire game (next entry). The allows
now sit on `mod tests`, where panicking on bad data is the point, and the crate
root enforces the workspace lints like every other crate.
