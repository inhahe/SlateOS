## TD-GUI-TEXT-COMMAND-DOES-NOT-WRAP — callers assume `max_width` wraps, but it clips

**Status.** Fixed for every prose caller found. `gui/**` first — the About
dialog's licence list (`7948cf8d5`), every `AlertDialog` (`46db88142`), the
notification toast body (`f559ea8b1`) and `InputDialog`'s prompt (`5a3a2e3d9`)
— then the app tree: `whiteboard` sticky notes (`658743045`), `contacts` notes
(`e48423a86`), `weather` alert descriptions (`4ca3cc4b1`), `dbviewer` result
messages (`9ace36e4c`), `reminders` and `podcast` prose fields (`42359f6fc`),
`vpnmanager` profile notes (`21813b691`), `partmanager` confirmation messages
and `netmanager` diagnostic details. Reopen it if a new prose caller turns up;
the survey below says how to find one.

**Re-run 2026-08-15 — the survey was not exhaustive, and found two more.**
Both had been missed, not introduced since:

- `apps/kanban` card detail (`cac3e2969`) — the card *description* and every
  comment *body*, both on the pane's one running cursor. The description
  advanced a flat 30 px and each comment card was filled at a flat 36 px
  *before* its body was drawn, so a wrapped comment overflowed a box sized
  without reference to it and the next comment stacked on the overflow.
- `apps/diskimager`'s write confirmation (`2df6e5b7f`) — the same shape as
  `partmanager`, and just as sharp: the warning is "All data on <drive> will
  be permanently destroyed. This operation cannot be undone", and the clip
  landed mid-sentence, dropping the half that says it is irreversible.

**Why the documented grep missed them.** It looks for a prose-named field
(`description`/`body`/`message`/`notes`/`content`) in a `Text` command, which
finds both of these — the problem is that it *also* finds ~30 status-bar and
table-cell sites where clipping is correct, and the earlier pass evidently
triaged the list and stopped. The distinguishing question is not the field's
name but **what happens to the cursor afterwards**: a site that follows its
`Text` command with a *constant* `cy += k` (or draws a container at a constant
height around it) is a wrapping bug regardless of what the field is called, and
a site whose text is the last thing in a fixed row is fine regardless. Grep for
`max_width: Some(` followed within a few lines by a literal `+= ` and triage
*that*; it is a much smaller and much higher-yield list.

Intersecting both signals (a prose-named field **and** a constant advance
within 14 lines) reduces the whole tree to 18 sites, listed here so the next
pass starts from a triaged list rather than re-deriving one:

~~*Probably real — the text is user- or device-supplied free prose:*
`apps/systemrestore` snapshot descriptions (`:2891`, `y += 20.0`),
`apps/sysmonitor` alert messages (`:1708`, `alert_y += 16.0`),
`apps/renamer` operation detail (`:1173`, `oy += 34.0`),
`apps/undelete` (`:2931`), `apps/passwordgen` pattern descriptions
(`:1546`).~~ — **all five done** (`ea8f2b468`, `ff2590275`, `eaadf231a`,
`8e64f088c`, `3b8bcd668`). Three were the predicted clipping bug and were fixed
as predicted: systemrestore's description now wraps to two measured lines,
renamer's operation details elide the *user's* substring inside a
developer-authored frame (so `"…" → "…"` still shows both halves and the arrow),
and undelete's metadata column elides from the **front** for `Original Path`,
because a path's identifying end is its filename.

**Two of the five were false positives for clipping — and both concealed a
different, real defect of the same family.** `sysmonitor:1708` and
`passwordgen:1546` draw bounded developer-authored strings into wide boxes, so
nothing was being cut. What both *were* doing was silently dropping whole items:
sysmonitor showed `.take(5)` of an unbounded alert list, and passwordgen's
pattern list had no bound at all (the number of patterns is a property of the
password — one per run of repeated characters — so an adversarial password draws
hundreds of rows straight through the bottom of the panel). passwordgen's
history list had a bound that tested the row's *top*, so the last row could
start one pixel inside the panel and be drawn 31 px outside it.

Three lessons worth carrying forward:

- **Triage that clears a site of the bug you were looking for is not triage
  that clears the site.** Both false positives were found by asking "what
  happens to the cursor afterwards" — the same question that finds the wrap bug
  finds the overflow bug, because both are a running cursor escaping its
  container. Finish reading the site.
- **A bounded surface needs a *counted* overflow, not a silent one.** A panel
  showing 5 of 12 alerts and saying nothing tells the user there are 5 problems.
  Every one of these now spends its last fitting row on a `+N more` marker. The
  trade is right: one fewer item plus an accurate count beats one more item plus
  a silent lie.
- **Derive the row count and the container height from one calculation.**
  sysmonitor's card height and its `.take(5)` were two constants free to drift;
  passwordgen's pattern list now lives in a function that *returns the cursor it
  ended at* rather than advancing a caller's `&mut cy`. Same
  two-calculations-for-one-quantity shape as the notes below. Count rows with a
  loop (`rows_that_fit`) rather than dividing — no float-to-int cast to get
  wrong at the boundary, and a negative gap yields 0 instead of a wrapped count.

~~`gui/desktop/src/notification_settings.rs` (`:1137`)~~ — **done** in
`19ee5234e`. It was the interesting one, and it did count bytes. Chasing it
turned up two more copies of the same helper in the same shell —
`window_rules.rs`'s `truncate_string` and `notif_pane.rs`'s `truncate_body` —
so all three went in one commit. Four notes worth keeping:

- All three compared `s.len()` (**bytes**) against a **character** budget, and
  all three picked that budget (24/28/60/60) independently of the width the
  text is drawn in. Both halves have to be wrong together for the bug to stay
  invisible, which is why they survived so long: on ASCII of average width the
  two errors roughly cancel.
- `window_rules.rs` was the one that could actually *mislead* rather than
  merely look bad: all three of its call sites passed `max_width: None`, so
  the home-grown elision was the only thing keeping a user-typed rule name out
  of the adjacent column. **Check `max_width` first when triaging one of
  these** — a helper backed by a real `max_width` is cosmetic; one without it
  is a correctness bug.
- `notif_pane.rs`'s copy also underflowed (`max_chars - 3` panics for a budget
  under 3, where the other two used `saturating_sub`) — the usual outcome of a
  utility being copy-pasted between files instead of shared.
- `window_rules.rs`'s column widths existed *twice* — once in the `headers`
  array, once as `cx += 70.0/180.0/200.0` in the row loop — and were free to
  drift. Now one set of constants both sides read. That is the same
  two-calculations-for-one-quantity shape as the wrap defect above, so other
  hand-drawn tables are worth grepping for it.

*Probably fine — a fixed-height row whose subtitle is a developer-authored
enum string, where clipping is the intended behaviour:*
`gui/desktop/src/{focus_assist,power_settings,privacy_settings,
backup_settings,default_apps}.rs`, and the `"Description"`/`"Notes"` section
*headings* in `apps/{podcast,reminders,contacts}` (the headings are one word;
the bodies beneath them were already converted).

Confirm each against the three questions above before changing it — the point
of the list is to make the triage cheap, not to pre-judge it.

**Next thread from this one, and it is bigger than the list above:
`RenderTree::text` cannot bound anything.** The whole triage above searched for
`max_width: Some(`, i.e. for callers that construct `RenderCommand::Text`
literally. But most drawing goes through the `RenderTree` helper, and its
signature is `text(&mut self, x, y, text, color, font_size)` — it hardcodes
`max_width: None` (`gui/toolkit/src/render.rs:272`). **There are ~249 `.text(`
call sites across `apps/` and `gui/`, and not one of them can express a
bound.** Most draw static labels and are fine; the ones that draw
variable-length content into a column are unbounded text running into whatever
is drawn next, with no clip and no marker — the `window_rules.rs` failure mode,
except the callers cannot fix it locally because the API has no width
parameter.

Confirmed instance: `apps/procexplorer`'s per-process thread table
(`:1907`) draws `thread.name` — a *process-supplied* string — with
`tree.text(...)` into a 200 px column, straight over the Status column beside
it. The fix is structural: `RenderTree` needs a width-taking variant, and the
dynamic-content call sites move to it. Do not convert all 249 — convert the
sites whose text is variable-length and has a neighbour.

~~Also queued from the same survey, `apps/torrent`'s three hand-drawn tables
(peers `:3054`, files `:3212`, trackers `:3301`): every column width is written
**three** times — in the `headers` array, in the row cell's `max_width`, and
again as a literal in the row's `cx +=` (300.0 / `Some(300.0)` / `308.0`). They
agree today and nothing keeps them agreeing. Same
two-calculations-for-one-quantity shape as `window_rules.rs`, one copy worse.
The cells are also wire-supplied — peer client names, torrent file paths,
tracker URLs — so they clip unmarked, and the file path clips from the *end*,
losing the filename that identifies it (`text::elide_start`, as used for
`undelete`'s `Original Path`).~~ **Done** (`c6c4e9ba4`): each table is one
`&[Column]` at module scope that the header and the body both walk, with
`table_header` returning the x of each column so the body positions cells at
exactly the offsets the header used. A shared `table_cell` elides with a marked
cut; a `Fit::{Start,End}` enum picks which end survives, and paths/URLs use
`End`. Seven tests, each column-fit one guarded by a checked-count and
rendering its panel *directly*; verified non-vacuous by dropping the elide
(six of seven fail, overruns of 218–262 px).

Worth generalising from it: the three-copies-of-a-width shape is not merely
redundant, it is what *hides* the clipping bug. When the width lives in three
places, no single place is obviously "the column", so nobody asks whether the
text fits in it — the question has no home. Collapsing the copies to one
`&[Column]` did not just remove the drift risk; it made "does this cell fit
its column?" a question a test could ask, which is how the six real overruns
became visible at all. The remaining hand-drawn tables from the survey
(~~`apps/filesearch`~~, ~~`apps/logviewer`~~, ~~`apps/renamer`~~,
~~`apps/systemrestore`~~) should be assumed to be hiding the same thing until a
counted test says otherwise. That prediction paid out on all four, and every
time the hidden bug found was worse than the clipping it was predicted from.

**The abstraction now exists: `guitk::table`** (`gui/toolkit/src/table.rs`,
`528a01aba`…`1311d3e0a`). `Table::new(&[Column], x)` owns the geometry;
`header` and `cell` both ask it for positions, `cell` elides to the column with
the cut marked, and `Fit::{Start,End}` picks which end survives.
`spans()`/`left()`/`right()` exist so a *test* can ask where a column is
without re-deriving it. Convert a table to this rather than writing a sixth
private copy — `apps/torrent` was migrated onto it in the same pass that
proved it out.

- ~~`apps/filesearch`~~ **done** (`1311d3e0a`). Two cells (Size, Type) had no
  `max_width` at all. The interesting part was not the table: bounding the Type
  cell meant reading the extension parser, which derived the extension with
  `name.rsplit('.').next()` — and that yields the **whole name** when there is
  no dot. `readme` was indexed with extension `readme`, displayed as type
  "README", counted under `readme` in `extension_stats`, and categorised as
  though `readme` were a format; `.bashrc` became extension `bashrc`. The
  nine-character cap on extensions turned out to be a *band-aid for exactly
  this*, and it rejected genuine long extensions (`.properties`,
  `.appxbundle`) as collateral. Fixed at the root (`rsplit_once` + non-empty
  stem), after which the cap could be widened to 24 — safe now precisely
  because the cell that displays it is bounded.
- ~~`apps/logviewer`~~ **done**. Not a fixed-column table — a running cursor
  with conditional cells — but the same shape and the worst instance found so
  far. The `[source]` cell was clipped to 100 px yet advanced the cursor by the
  source's **full untruncated width**. So a long source vanished mid-name with
  no marker *and* pushed the message right by space nothing occupied; past
  ~1000 px of source the cursor ran off the row, `msg_width` went negative, and
  `text::elide` of a negative width returns `String::new()`. **The log message
  disappeared entirely**, leaving a row showing only a clipped source name. The
  regression test finds it drawn at x=8931 with text `""`.
- ~~`apps/renamer`~~ **done**. A six-column preview laid out with hand-written
  `cx += 38.0 / 258.0 / 28.0 / 258.0 / 88.0` increments and no eliding
  anywhere; a long name overran its 250 px column by 164 px in the regression
  test, straight through the arrow and into the next name. This is the worst
  *consequence* of the three even though the mechanism is the mildest, because
  the list is a **rename preview** — the one screen whose entire job is to let
  the user check what is about to happen before committing. Both name columns
  now use `Fit::End`: a name cut the usual way loses the extension and any
  numeric suffix, which are exactly the parts a rename changes, so a batch of
  long names would all render identically and the user would be confirming a
  rename they could not actually see. And, as in filesearch, reading the app to
  fix the table turned up the same extension-parser bug next door:
  `rsplit('.').next()` paired with a `len() < name.len()` guard rejects a
  dotless name but still accepts a **leading** dot, so `.bashrc` was given
  extension `bashrc` — shown as its type and swept up by a `bashrc` extension
  filter alongside real `x.bashrc` files. Same root fix.
- ~~`apps/systemrestore`~~ **done**. The survey entry was stale — the ancestry
  chain's per-link elide-and-advance had already been fixed in an earlier pass
  — but that fix was one level too low. **Capping each link said nothing about
  the chain.** Every link was capped at 150px and the cursor advanced by what
  was drawn, correctly, once per ancestor, with *no reference to the panel's
  right edge at all*. Twelve long-named ancestors cost `12x154 + 11x20 =
  2068px` against a 986px budget; the regression test finds link **5 of 12**
  already ending at x=1059.8 past the 1038px edge, so seven links — **including
  the selected snapshot itself, the one the whole panel exists to describe** —
  were drawn off-window. Fixed with `ancestry_first_visible`, a free function
  (so a test can ask it directly) that drops links from the *front* and
  reserves the cost of the leading `...` marker it forces the caller to draw.
  Front, not back, because the tail is the selected snapshot and its near
  ancestors say where it came from; the distant root is the least informative
  part.
- ~~`apps/habits`~~ **done** (`55372701b`). The statistics table: eight columns,
  hand-written pushes, and a habit name — the one user-entered value on the row
  — clipped with no marker, so "Read" and "Read thirty minutes before bed"
  rendered the same. Converted to `Table::with_gap`. Worth recording what the
  regression test taught: both long names elided to the *identical* string, so
  the comment I had written claiming the marker disambiguates a shared prefix
  was wrong and had to be rewritten. **An ellipsis does not disambiguate; it
  only says "there is more".** Do not justify eliding on the grounds that it
  tells two similar values apart — it does not, and where telling them apart
  matters, the fix is to cut at the *other* end (see partmanager below).
- ~~`apps/partmanager`~~ **done**. The partition list's `cols: &[(&str, f32)]`
  array held column *pitch*, so every cell subtracted its own padding
  (`max_width: Some(col_w - 8.0)` drawn at `col_x + 4.0`) while the **header**
  was drawn at `+4.0` but bounded by the *full* pitch — a header could overhang
  the next column by 4px. `Table::with_gap` removes the discrepancy by
  construction: the width is what a cell may use and the 8px difference is the
  gap. A long partition label overran its column by 75px in the regression test.
  The instructive part was the mount-point column. I first wrote it `Fit::Start`
  with a comment arguing that "a mount point's leading directories distinguish
  `/mnt/...` from `/media/...`" — while *citing* `/mnt/backup-2026` vs
  `/mnt/backup-2027` as the motivating failure, which differ at the **tail**.
  The comment contradicted its own example and I did not notice; the test did,
  by failing. A mount point is a path and a path's leaf is what names it, so it
  is now `Fit::End`, and the test asserts the cut mount points are **pairwise
  distinct** rather than merely marked. That assertion is the one worth copying:
  "is it marked as cut?" passes on a column that renders every row identically,
  which is the failure that actually misleads a reader.
- ~~`apps/procexplorer`~~ **done**. Three `cx +=` sites; two (the process table
  and the thread table) had already been single-sourced in an earlier pass and
  were genuinely fine. The third — the **Network tab's connection table** — was
  the worst instance in the sweep so far, because its cells were not clipped
  *at all*: `tree.text(cx + 6.0, ry + 4.0, field, color, 11.0)` with **no
  `max_width` argument**, then `cx += col_w`. Nothing bounded the value, so the
  regression test finds an IPv6 local address drawn 71px past its column, over
  the Remote Address beside it.
  This is the shape to watch for next: the survey looked for `max_width` values
  that disagree with a cursor advance, but a cell with **no width argument at
  all** does not appear in that grep. `tree.text(...)` is five arguments and
  `tree.text_in(...)` is six; the unbounded one is the shorter, more natural
  call, and it is invisible to a search for a mismatched bound.
  Addresses use `Fit::End` — the port lives on the tail, and `:443` vs `:22` is
  the whole difference between two rows to the same peer. Converting also
  required `Table::header_weighted`, because this app marks headings by colour
  alone and `Table::header` forces bold; "my headings are the wrong weight" was
  otherwise a reason to leave a table hand-drawn and unfitted.
- ~~`apps/defrag`~~ **done**. Found by the new search rather than the `cx +=`
  one: the fragmented-file list held four column *positions*
  (`x + w*0.55/0.70/0.85`) restated in the header loop and again in every row,
  with three of the four cells carrying `max_width: None`. The path cell did
  have a bound — `Some(w * 0.50)` — but it was a **fifth number agreeing with
  none of the others**: the path column actually runs to `w*0.55 - PADDING`, so
  the clip fell ~25px short of its own column *and* had no marker. The
  regression test finds a path 140px past its column edge.
  Two things generalise. First, **a proportional layout hides the missing bound
  better than a fixed one**, because there is no width array to notice is
  missing an entry — the columns are positions, and a position cannot be
  overflowed, only passed. `file_list_columns(w)` returns widths precisely so
  "does the last column end inside the panel?" becomes answerable, and there is
  now a test asking it at three panel widths. Second, a fractional width minus a
  constant **goes negative on a narrow panel**, and `text::elide` of a negative
  width returns the empty string — so the column would silently blank rather
  than shrink. Clamped with `.max(0.0)` and tested at w = 0, 1, 20, 60.
  Paths use `Fit::End`: the list is sorted by severity, so its rows are
  typically siblings under one deep directory, and cut at the end they collapse
  to one repeated prefix with every filename gone.
- ~~`apps/undelete`~~ **done**. The widest drift found: the recovered-file list
  wrote each column width **twice, differently**. The heading row bounded Name
  at `width * 0.35`, Type at `0.15`, Deleted at `0.18`; the row drew the same
  three cells at `0.33`, `0.14`, `0.16`. Neither number was the column — the
  column is the distance to the *next* heading's `x`, which no line of code
  mentioned — so no cell was ever measured against the space it actually had,
  and all of them clipped unmarked. A path overran its column by 91px in the
  regression test. This is the screen whose entire job is choosing which
  carved-off-a-damaged-disk files to restore.
  Reading it to fix the table turned up a second, independent bug: the
  confidence badge was a flat 64px pill at `x + width * 0.83 + 32` — a fixed
  size at a proportional position. That is inside the panel only while the
  panel is wide; at width 240 the test finds it spanning 271.2..335.2 against a
  panel ending at 280, i.e. drawn 55px outside the list entirely. It is now
  clamped to its column. **A fixed size at a proportional position is the same
  class of bug as a fractional width minus a constant** (`apps/defrag`): one
  overflows the container on a narrow panel and the other underflows to
  negative, but both are a constant mixed with a fraction, and both need a
  clamp plus a test at small widths. Worth grepping for the shape directly:
  a literal `+ 32.0` or `= 64.0` in the same expression as a `width * 0.`.
  Names use `Fit::End`, and here that is not a judgement call but a fact about
  the app: `RecoverableFile::from_signature` *generates* names as
  `recovered_{offset:08x}.{ext}` and `from_inode` as `inode_{n}`, so a deep
  scan's results share a ten-character prefix by construction. Cut the usual
  way, the whole column reads `recovered_00…`. The test asserts the cut names
  are pairwise distinct, per the partmanager lesson.
  Two structural changes came out of it. The fractions now live in one
  `FILE_COLUMNS: [(SortField, f32); 5]` array and **sum to 1**, which a test
  asserts — a layout summing to less leaves dead space, one summing to more
  runs off the panel, and neither of the old two sets of numbers summed to
  anything meaningful. And each heading's text is taken from
  `SortField::display_name()` rather than written out again, so the label and
  the field that clicking it sorts by cannot come to disagree.
  Needed one new toolkit primitive: `Table::fitted(cmds, x, width, y, …)`,
  which fits text to an explicit box with the width clamped at zero.
  `Table::cell` is now defined in terms of it (a test asserts the two produce
  byte-identical commands). It is the escape hatch for a cell that shares its
  column with a decoration — undelete needs it twice (the colour swatch before
  the type name, the badge's interior padding) and `apps/diskanalyzer` will
  need it for its per-row tree indent. The clamp belongs in the toolkit rather
  than at each call site precisely because the call site's arithmetic is
  `column_width - decoration`, which is the expression that already went
  negative once in `apps/defrag`.
- ~~`apps/diskanalyzer`~~ **done**. The header loop was literally
  `for (label, cx, _cw)` — the column width was in scope, bound, and
  **explicitly discarded**, with the heading then drawn `max_width: None`. Of
  the four body cells only Name had any bound; Size, % and Type had none. This
  is the clearest instance of the general point: a width that exists but is
  not the *thing the cell is measured against* is not a column, and a `_`
  binding is the shape to grep for.
  Two bugs fell out that the table itself did not cause. First, the columns
  ended at x=660 in a 960px window — a third of every row was blank while the
  one column holding variable-length data, and the only one that ever clipped,
  was the narrowest it could be. Name now takes the slack and a test asserts
  the table's right edge mirrors its left inset.
  Second, and worse: the Name cell's bound was `360.0 - indent - PADDING`
  where `indent = depth * 20.0`. **Tree depth is data and has no bound.** Past
  depth 33 that expression is negative, and an elide to a negative width is
  the empty string — so a deeply nested row would render with *no name at
  all*, not a short one. The test finds −24px at depth 33 with the cap removed.
  The indent is now capped so a name always keeps 120px, i.e. deep rows stop
  indenting rather than stop existing. Generalising: **any layout quantity
  derived from tree depth, list length or nesting is unbounded input**, and
  subtracting it from a fixed width needs a floor, not just a `.max(0.0)` —
  clamping to zero merely converts "negative, blanks" into "zero, blanks".
  The expand/collapse chevron is now drawn in its own box rather than
  prepended to the name (`format!("{prefix}{}", row.name)`). Prepended it
  shares the name's fate, and the name wants a front cut — so the chevron, the
  only thing on the row saying whether it opens, would be the first thing
  removed. A decoration that must survive cannot be concatenated onto text
  that will be cut; give it its own box.
- ~~`apps/netscan` host table~~ **done**. The clearest case yet of *two
  independent copies of one layout*: `render_table_header` held a list of
  `(x, label)` pairs with **no widths at all** and every heading
  `max_width: None`, while `render_host_row` hand-wrote an `x:` and, several
  lines away, a `max_width:` that came from nowhere in particular. Nothing
  connected a cell's bound to the distance to the next column — Ports and
  Latency simply had no bound, and Hostname had `Some(160.0)`, a bare clip
  with no marker, on the **one string in the row this program does not
  choose**: it is reverse DNS. Both are now one `guitk::table` whose seven
  shares sum to 1.0, so the row ends at the table's right edge.

  Two things worth carrying forward. First, this row cuts in **both**
  directions, and which end is right is a fact about the data, not a taste:
  the IP address and the MAC keep their *tails* (a subnet shares its leading
  octets, a vendor shares its OUI prefix), while the hostname keeps its
  *head* (`alpha.engineering.example.com` and `bravo.engineering.example.com`
  differ at the start). A single house style of "always elide the end" would
  have rendered every host on a /24 as `192.…`.

  Second, and the sharper lesson: **a distinctness test is only as strong as
  the fixture's shared prefix is long.** The first version of the hostname
  fixture used `workstation-alpha.engineering.corp.example.com` and friends,
  and the test *passed with the cut deliberately flipped to the wrong end* —
  because the shared suffix was a little shorter than the column, so the
  visible tail still carried one character of the distinguishing label
  (`…a.engineering…` vs `…o.engineering…`). The assertion was true for both
  cut directions, i.e. it was measuring nothing. The fixture now uses a shared
  suffix comfortably longer than the column can show, and the flipped-cut run
  fails as it should. When a test's premise is "these values are
  indistinguishable once cut", check that they really are — by breaking the
  code and watching it fail, not by reading the strings and assuming.

  A related trap in the same tests: a heading shares its column's left edge
  with the cells beneath it, so gathering "a column" by x picks up the heading
  as an extra row and a `len() == 4` assertion fails at 5 for a reason that
  has nothing to do with the layout. Gather rows and headings separately.
- ~~`apps/jsonviewer` statistics view~~ **done**. Found by the grep the
  undelete badge suggested — a literal constant in the same expression as a
  fraction of the width — and it is the worst instance of that shape so far
  because of the size of the constant:

  ```rust
  let bar_max_width = width * 0.5 - 250.0;
  ```

  The panel is the window minus a 320px sidebar, so `width < 500` is an
  ordinary window, not an extreme. Below it the value is **negative**, and it
  was used twice: as a `FillRect` width — a negative-width rect, which is not
  a small bar but an ill-formed drawing command — and as a *position*, in
  `x: 180.0 + bar_max_width + 8.0` for the percentage label. So the label did
  not merely sit on a stunted bar; it walked **left** of the bar's own origin
  and landed on top of the count cell at `x: 120.0`. Two cells rendering the
  same pixels, with no clipping to hint at it.

  The deeper mistake is that the bar was *anchored* at an absolute `x: 180.0`
  while being *sized* from `width * 0.5`: its width had no relationship to the
  space actually left beside it. That is the general form of this bug —
  **mixing an absolute coordinate system with a proportional one in the same
  element.** A fraction of the panel is a valid width only if it is measured
  from a position that is also a fraction of the panel. Grep shape: an
  absolute `x:` literal in a rect whose `width:` mentions `width *`.

  Both sections are now `guitk::table`s: label/value inside the general-stats
  card, and label/count/bar/percentage across the type-distribution row, every
  column a fraction of the room that is really there (`TYPE_FRACTIONS` sums to
  1.0, asserted). The percentage is a column, so it is right of the bar by
  construction rather than by arithmetic that can invert.

  Two notes for the next conversion. First, **`Table`'s origin sits before the
  leading gap** — `left(0)` is `x + gap`, and `total_width` counts a leading
  *and* a trailing gap. Passing the desired inset straight to `with_gap` shifts
  every column by one gap, which is invisible in a same-table comparison
  (all columns move together, header and rows still agree) and shows up only
  as a wrong margin at the far end. `the_type_rows_fill_the_panel` caught it;
  a test that only checks cells against their own columns never would.
  Second, **a percentage/short-numeric column legitimately elides to nothing**
  on a very narrow panel, so a test that finds those cells by matching a
  trailing `'%'` finds zero of them and fails for the wrong reason. Find such
  cells by *position* (`table.left(col)`), which is the property under test
  anyway, and add a separate assertion that the text survives once the panel is
  wide enough — otherwise "the percentage is right of its bar" passes vacuously
  for a column that always renders empty.

The lesson that generalises past the cursor pattern: **a per-element bound is
not a bound on the row.** Eliding each cell to its own width makes every cell
individually correct and still lets the row as a whole run off the panel, and
because each element passes its own local check, nothing in the code reads as
wrong. Any layout that repeats an element a data-dependent number of times —
ancestry chains, tag pills, breadcrumbs, filter chips — needs a budget against
the container's edge as well as a cap per element, and a test that renders a
deep case rather than one element.

And the one the partmanager mount column added: **"was it marked as cut?" is
too weak a test for a column of near-identical values.** A cut marker satisfies
that assertion while every row still renders as the same string. Where the
values in a column share a long prefix (paths, mount points, versioned names),
assert the drawn cells are pairwise **distinct**, and let that drive which end
`Fit` keeps.

The other pattern to take forward: wherever a cursor-laid-out row draws a cell
clipped to one width and advances by another, the advance is the bug, and its
blast radius is everything drawn *after* it on that row — not just the cell
that overflowed. Grep for `cx +=` near a `max_width:` that is not the same
expression. The fix is always the same: elide first, then advance by what was
actually drawn.

But do not let that grep define the search, because **the unbounded cell is the
one it cannot find.** A `max_width` that disagrees with a cursor advance is at
least *present*; `tree.text(x, y, s, c, size)` and a bare `RenderCommand::Text`
with `max_width: None` carry no bound to disagree with, and they are the
shorter, more natural calls. procexplorer's Network tab was written that way
and drew 71px over its neighbour. Search for the *absence*: `tree.text(` inside
a loop over a column array, and `max_width: None` on anything whose text is not
a literal.

**What it is.** `RenderCommand::Text` carries a `max_width`, and the obvious
reading is that the compositor wraps to it. It does not. `Compositor::draw_text`
walks the string one glyph at a time and `break`s at the limit:

```rust
if let Some(mx) = max_x && pen + advance > mx as f32 {
    break;
}
```

So `max_width` is a **clip**, and it produces exactly one line. Any caller that
hands a paragraph to a single `Text` command is showing only its first line's
worth of characters — silently, with no marker that anything was dropped.

**What it broke.** Four found so far, all user-visible:

- `gui/desktop/src/about.rs` — each open-source licence went out as one command,
  so the About dialog's Licences tab showed roughly the first line of each
  licence and nothing else. It compounded with the byte-count defect above: the
  item height was reserved as `text.len() / 80` lines, so the list also left
  gaps or overlapped, depending on the licence.
- `gui/toolkit/src/modal.rs` — every `AlertDialog` in the system. The message was
  one command, and `compute_height` reserved a flat `FONT_SIZE * 3.0` for it
  regardless of length, so a long error message was cut to one line inside a box
  sized for three.
- `gui/notifications` — every toast body. `toast_height` did not depend on the
  body at all, so the fix had to grow the toast as well as wrap the text, or a
  two-line body would have drawn over the toast stacked beneath it.
- `modal.rs`'s `InputDialog` — the prompt got a flat one-line allowance
  (`FONT_SIZE + 12.0`) in both the height and the running `content_y`, so the
  input field was drawn over the second line of any longer prompt.

**Proper fix.** `guitk::text::wrap(text, max_width, size, weight)` (added in
`7948cf8d5`) breaks a string into the lines it will actually be drawn as; emit
one `Text` command per line and derive any reserved height from that same list,
never from a second calculation. It is a thin wrapper over the `SystemFont::wrap`
that already existed at the font layer, so it measures with the cache the
compositor draws with. `menu.rs`'s tooltip wrapper was repointed at it rather
than left as a second implementation.

Two things the fixes had to get right, and any further one will too:

- **Reserve height from the lines you drew.** The whole defect class is two
  calculations for one quantity. `about.rs` and `modal.rs` both now call
  `wrap` once and use its `len()` for the height.
- **A clamped box still needs a clip.** `AlertDialog`'s height is clamped at
  `DIALOG_MAX_HEIGHT`, so a message can be longer than any box it can be given.
  Wrapping alone would then draw the overflow straight through the button row —
  text on top of the controls that dismiss the dialog. The render loop breaks
  once a line would reach the buttons.
- **A bounded surface caps the lines and says so.** A toast is a glance, not a
  reader, so wrapping it without limit would push the rest of the stack off
  screen. It takes three lines and elides the last, because a body cut without
  a mark reads as a complete sentence — the reader cannot tell there was more.

**`text::Paragraph` — the fix the app tree actually uses.** After the same
wrap-and-advance block had been hand-written at five call sites, the convention
it rests on stopped being worth trusting to memory, so `guitk::text::Paragraph`
(added in `42359f6fc`) makes it structural: `Paragraph::draw` emits the
commands *and returns the height it used, measured from those commands*, so a
caller that advances its cursor by the return value cannot reserve a height
that disagrees with what was drawn. `.max_lines(n)` caps a bounded surface and
elides the last line. Prefer it over calling `wrap` by hand; `wrap` is still
right when a caller needs the lines themselves rather than a drawing.

It takes `&mut impl Extend<RenderCommand>`, because half the app tree collects
into a bare `Vec<RenderCommand>` and half into a `RenderTree`; `RenderTree`
implements `Extend<RenderCommand>` so both work without a caller reaching past
it into `commands`.

**Each site needs three questions asked before anything changes:** *what is the
text's box, what reserves that box's height, and does anything downstream of it
move.* The answers split the app sites into four shapes, and the fix differs:

- **A running cursor** (`contacts` notes, `reminders` description and notes,
  `podcast` description and show notes) — wrap and advance the cursor by the
  height drawn. These were the dangerous ones: the field below was drawn on top
  of the one above the moment either ran past a line.
- **A card that sizes itself** (`weather` alert descriptions) — wrap, then grow
  the card to the wrapped height, floored at the old size so the common
  one-line case looks unchanged.
- **A fixed box the user drew** (`whiteboard` sticky notes) — wrap in *canvas*
  units, not screen pixels, so zooming moves and scales the note without
  reflowing it; drop lines that fall past the bottom rather than spilling onto
  the canvas.
- **A pane or dialog with fixed furniture below** (`dbviewer` result messages,
  `partmanager` confirmation dialogs) — wrap, move what follows down, *and* cap
  the lines so the prose cannot crowd out the results table or run through the
  button row. `partmanager` was the sharpest case: its messages are whole
  sentences about destroying a disk, and the first one in the toolbar
  ("This will destroy ALL data on the disk. Choose GPT (default) or MBR.")
  was already losing its second half.

**Where it is *not* a bug.** `max_width` on a single-line label — a title, a
column cell, a status-bar message — is doing exactly what it should. Clipping is
the intended behaviour there, and the fix for an over-long one is `text::elide`,
not wrapping. `netmanager`'s diagnostics rows are the example: a row is a fixed
two-line cell in a list meant to be scanned, so the detail line is elided to the
row width rather than wrapped. Only reach for `Paragraph` where the text is
prose the user is meant to read in full.

**Where to look next.** Prose callers are found by grepping for a `Text` command
whose body is a `description` / `body` / `message` / `notes` / `content` field
with `max_width: Some(..)`. The survey turned up no remaining prose sites, but
it is not a proof — a new app can reintroduce one, and the grep will not catch a
prose field under an unusual name. Status-bar messages are *not* in scope —
truncating those to one line is the intended behaviour, and the survey turns up
many of them (`automator`, `dictionary`, `diskimager`, `battleship`, `reversi`).

**Follow-on debt this exposed:** see `TD-GUI-CLIPPED-TEXT-IS-NOT-MARKED` below.

Each fix has the same three questions: what is the text's box, what reserves
that box's height, and does anything downstream of it move. Where the answer to
the third is "yes" — a stacked list, a following field — the height and the
drawn lines must come from one call, not two.
