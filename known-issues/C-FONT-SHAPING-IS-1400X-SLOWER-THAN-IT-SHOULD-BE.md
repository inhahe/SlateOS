## C-FONT-SHAPING-IS-1400X-SLOWER-THAN-IT-SHOULD-BE (lane C, 2026-08-17)

**Status: OPEN, but 38-45x recovered — root-caused 2026-08-17, three fixes
landed the same day and a fourth on 2026-09-02. Shaping now costs ~0.7
us/character where it cost ~29, and a 1,000-character line is 0.70 ms where it
was 30 ms. That is still an order of magnitude off HarfBuzz, so the entry stays
open — but it is no longer a cliff, and the editor work it was blocking can
proceed. What remains is named under "What is left" below, and the phase table
that says which pass to attack is now a permanent instrument rather than a
patch someone has to rewrite.**

**Read "How the numbers here were taken" before comparing any two tables in
this entry.** Every figure recorded on the day of the fixes was a *median*, and
the median turned out to be measuring the machine as much as the code — it
inflated everything by roughly 2x and moved by up to 1.9x between runs of the
identical measurement. The instrument was corrected the following day and the
whole A/B ladder re-taken in one comparable set; the older tables are kept for
the history but their absolute figures are all about twice life size.

**What.** `ScaledFont::shape` cost **~29 microseconds per character** on a
real outline face (reported at the time as ~64; see the statistic note below),
and the cost was *linear* in the string — so it was a huge per-character
constant, not an algorithmic blow-up over the string. For scale, HarfBuzz
shapes a short Latin run in single-digit microseconds *total*. We were roughly
three orders of magnitude off.

**Measured** on the dev host, release build, by
`gui/toolkit/src/text.rs` → `mod shaping_cost` (two `#[ignore]`d timing tests,
kept in the tree so the numbers can be re-taken elsewhere):

```
cargo test --release -p guitk --lib shaping_cost \
    --target x86_64-pc-windows-gnu -- --ignored --nocapture --test-threads=1
```

| chars | system face | built-in bitmap face | ratio |
|---:|---:|---:|---:|
| 80 | 5,033 us | 3.0 us | 1678x |
| 200 | 12,365 us | 5.6 us | 2208x |
| 1,000 | 64,269 us | 31.5 us | 2040x |
| 5,000 | 323,435 us | 159.7 us | 2025x |
| 20,000 | 1,289,777 us | 900.1 us | 1433x |

Both columns are linear; the system face's slope is ~64 us/char and the
built-in face's is ~0.045 us/char. Note the built-in (bitmap) backend
**bypasses the shaping pipeline entirely** — `SystemFont::shape_lang` builds
the run straight from `char_indices` — so that column is not "the same work
without layout tables", it is "no pipeline at all". It is included as a floor,
not as an apples-to-apples comparison.

**The A/B ladder, all four configurations measured with one instrument.**
Taken 2026-08-17 after the statistic was fixed, so unlike the day-of tables
these five columns are directly comparable to each other. Each configuration
is the shipping tree with the *filters* neutralised in place — the digests are
still computed at parse time, which costs nothing per shape — so this is an A/B
of the skipping and not of four different revisions. Each figure is the best of
three separate process invocations, none of them sharing a run with a compile.

| chars | no digests | + run digest | + per-subtable | + `GPOS` | cumulative |
|---:|---:|---:|---:|---:|---:|
| 80 | 2,313.0 us | 110.8 us | 78.8 us | **64.2 us** | **36.0x** |
| 200 | 5,816.6 us | 271.5 us | 190.7 us | **154.2 us** | **37.7x** |
| 1,000 | 30,047.7 us | 1,334.6 us | 925.3 us | **733.9 us** | **40.9x** |
| 5,000 | 170,978.8 us | 7,330.6 us | 5,067.1 us | **4,152.2 us** | **41.2x** |
| 20,000 | 742,798.2 us | 31,084.8 us | 21,263.7 us | **17,188.7 us** | **43.2x** |

Step by step, and strikingly consistent across a 250x range of line lengths —
which is what one expects of a fix that removes a per-glyph-per-lookup constant:

| step | 80 | 200 | 1,000 | 5,000 | 20,000 |
|---|---:|---:|---:|---:|---:|
| run digest (fix 1) | 20.9x | 21.4x | 22.5x | 23.3x | 23.9x |
| per-subtable (fix 2) | 1.41x | 1.42x | 1.44x | 1.45x | 1.46x |
| `GPOS` (fix 3) | 1.23x | 1.24x | 1.26x | 1.22x | 1.24x |

Against the built-in bitmap face the shipping configuration measures **49x** at
80 characters and 51x at 1,000, where the bug sat at 1,400-2,200x.

The headline was reported as 33.0x on the day. That was median-against-median
and it was close to right for the wrong reason: the median inflated the
baseline (5,033 us at 80 chars against a true 2,313) *and* the result, and the
two errors largely cancelled.

**Why this is severe.** A single 200-character line took **5.8 ms** to shape.
A 60 Hz frame is 16.7 ms. So one line of text was 35% of a frame, and a
50-line screen roughly **290 ms** — 17 frames' worth — if anything shapes
per frame. Every `text::measure` call shapes; the toolkit measures constantly
(button labels, list rows, `draw_tokens` once *per syntax token*). Whatever is
saving us today is caching further up, not speed here, and any path that
misses that cache falls off a cliff.

**What has been ruled out.**

* **Not the font-cache mutex.** One `with_font()` — lock plus cache lookup —
  measures **0.045 us**, i.e. 0.06% of a single character's shaping cost then,
  and 5.6% of one now.
* **Not test-harness contention.** The first run of the instrument reported
  4.5 ms for 80 characters *with* two tests sharing the global font mutex; with
  `--test-threads=1` and the font fetched once outside the timed loop, the
  number barely moved. The instrument now documents both requirements.
* **Not cold caches.** Every measurement takes a warm-up shaping outside the
  sample set before any sample is kept.
* **Not per-shape re-parsing of the big OpenType tables.** `GSUB` and `GPOS`
  both decode through `otl::ByScript`, built once per face — `otl.rs:409`
  already documents that doing that walk per shape is too slow ("re-reading
  1874 subtable offsets for every string drawn"). `MarkPositioning::parse` and
  the kerning tables are likewise parsed once, at `Face` construction
  (`sfnt.rs:821`).
* **Not `cmap` lookups**, which this entry originally named as "the leading
  hypothesis and the cheapest to test". They are not the problem, and the
  reasoning that made them a suspect was wrong twice over: `Face::glyph_index`
  → `cmap_format4` (`sfnt.rs:1295`) already **binary-searches** the segment
  array rather than walking it, and the A/B below charges the whole cost
  elsewhere anyway. Recorded here because the plausible-sounding hypothesis
  survived a careful reading of the code and was only killed by measurement.

**The root cause, measured.** An A/B that environment-gates the two layout
passes in `ScaledFont::shape_with` (`gui/font/src/scaled.rs:677`) settles it
outright, at 80 characters:

| configuration | median | share of cost |
|---|---:|---:|
| baseline | 4,272 us | — |
| `GPOS` disabled (`position_segments` skipped) | 4,538 us | ~0% |
| `GSUB` disabled (`face.substitute` skipped) | 80.3 us | **98.1%** |

So **`GSUB` is essentially the entire cost**, and `GPOS` is free. Instrumenting
the substitution pass says why. On the development host's monospace face a run
of plain Latin selects **147 of 147 lookups over 768 subtables**, because the
script selects them — `latn`/`DFLT` reaches every one. `gsub::apply_lookup`
then walks *every glyph* for *every* selected lookup, and `apply_at` walks
*every subtable* of that lookup doing a coverage search. For a 200-glyph run
that is **29,400 `apply_at` calls and ~153,000 coverage searches per shape**,
and almost every one answers "no" — the lookups belong to features and scripts
the run does not contain. The ~64 us/char was the cost of asking.

**The fix that landed.** HarfBuzz's `hb_set_digest_t`, in a new
`gui/font/src/digest.rs`. A digest is a *conservative* membership summary of a
glyph-id set: three 64-bit masks indexed by the id shifted right by 4, 0 and 9,
`|`-ed together. It may answer "yes" for a glyph that is not in the set, and is
never allowed to answer "no" for one that is — that one-sidedness is what makes
it safe to skip on, since a false "yes" costs a search that was going to happen
anyway and a false "no" cannot occur.

* At parse time, `otl::ByScript::parse` gives every `Lookup` the union of its
  subtables' **input** coverage. "Input" is load-bearing: for the contextual
  types in format 3 the coverage is not the second field, and for a chaining
  context it sits after the backtrack array. An unreadable subtable widens the
  lookup's digest to `Digest::full()` rather than narrowing it.
* At shaping time `gsub::apply_stages` keeps a digest of the run, rebuilt
  behind a `dirty` flag whenever a lookup actually substituted something. A
  lookup whose digest cannot intersect the run's is skipped in **O(1)**; within
  a lookup, a glyph the digest excludes skips the subtable walk.
* The rebuild lives in `apply_stages`, not `apply_lookup`. Putting it in
  `apply_lookup` costs O(n) x 147 and gives most of the win straight back —
  measured at 276.9 us against the 218.3 us the flag version reaches. It also
  has to be the caller's job because the per-syllable path applies a lookup to
  a *slice*, and rebuilding from the slice would describe the syllable rather
  than the run.

**The second fix: per-subtable digests** (HarfBuzz's `hb_applicable_t`, which
lives inside its `hb_ot_layout_lookup_accelerator_t`). The run digest excludes
only 52 of the 147 selected lookups; the other 95 genuinely share glyphs with
the run *somewhere*, and each survivor was still walking all of its subtables
per admitted glyph at ~5 coverage searches each. So `otl::Lookup::subtables`
became `Vec<Subtable>` — an offset and its own digest as **one** type, not two
parallel `Vec`s, because a digest paired with the wrong subtable is not a slow
filter but a *wrong* one that silently drops substitutions. `gsub`'s six apply
functions filter through `admitting(subtables, glyph)` before each coverage
search. A free side effect: one unreadable subtable used to force the whole
lookup's digest to `full()` and kill all skipping for it, and now widens only
itself while its readable neighbours keep their narrow digests.

At the time the filter was **GSUB-only**, deliberately, with the reason
recorded at each of the three call sites that did not use it: `gpos.rs` (the
A/B charged ~0% of the cost to positioning, so filtering there was complexity
bought with no evidence), `kern.rs` (kerning asks about a *pair*, and a digest
summarises only the left glyph) and `would.rs` (`would_apply` runs once per
candidate pair, not once per glyph per lookup). The `gpos.rs` half of that did
not survive the day — see "the third fix" below, which is what happens when the
evidence a decision was waiting for finally arrives. The other two still hold.

**Guard against regression.** `guitk`'s
`text::shaping_cost::the_layout_tables_do_not_dominate_shaping` is **not
`#[ignore]`d** and runs in the normal suite. It asserts the system face costs
under **500x** the built-in face for 80 characters — a ratio rather than a
microsecond figure, so it survives a debug build, a loaded machine and slower
hardware without going so loose it stops catching anything. It measures **38x
in release and 71x in debug** today — seven times below the threshold and
nearly three times below the bug, which sat at 1400-2200x. (Those two figures
read 80x and 147x while the module reported medians. Nothing about the shaping
changed.)

**The first phase breakdown — SUPERSEDED, kept for the history.** Once `GSUB`
stopped dwarfing everything, the coarse A/B stopped being a useful guide, so
`shape_with` was temporarily instrumented to time each pass and the medians
taken over 51 shapings of 80 characters (release build). **Do not act on this
table** — see "The phase breakdown, re-taken" below, which contradicts two of
the three items this one was used to predict:

| phase | median | share |
|---|---:|---:|
| `gsub` (`substitute_runs`) | 89.1 us | **55.3%** |
| `gpos` (`position_segments`) | 32.0 us | **19.9%** |
| `norm` (`norm::pieces`) | 14.2 us | 8.8% |
| `advances` (`advance_at` per glyph) | 10.2 us | 6.3% |
| `glyphbuild` (the `SubGlyph` loop) | 7.4 us | 4.6% |
| `pre+script` (hangul, uvs, levels, `script::runs`) | 4.2 us | 2.6% |
| `joining::forms` | 2.2 us | 1.4% |
| `tail` (kerning, reorder, mark synthesis) | 1.5 us | 0.9% |
| `byte_levels` | 0.3 us | 0.2% |
| total | 161.1 us | |

The total exceeds the uninstrumented 129.3 us by the instrument's own cost, so
read the **shares**, not the absolute figures. (These are medians, and doubly
so — both the phase timings and the total they are divided by. The shares are
the more robust half of the table for that reason too: a factor that scales
every phase alike cancels in a ratio.)

Two warnings for whoever re-takes this, both learned the hard way here:

* **Print once, at the end.** The first version of the instrument `eprintln!`ed
  per phase and reported 490 us for a 129 us shape, giving four unrelated
  phases the same ~38 us — that was the cost of a line into a captured stderr,
  not the work. It was caught only because the uniformity looked wrong.
* **The old A/B's "`GPOS` is ~0%" was an artefact of scale**, not an error: 32
  us inside a 4,272 us shape really does round to nothing. A share measured
  against a dominant term says nothing about what happens once that term is
  gone.

**The third fix: the same digests, in `GPOS`.** The 19.9% above is what
justified it; the entry had previously declined it in a comment, for want of
exactly this evidence. `Positioning::apply` builds a run digest **once** — a
positioning pass never changes which glyphs exist, so unlike `gsub` it needs
no dirty flag and the summary is exact for every lookup — and skips any lookup
that cannot intersect it. `run_lookup` then skips an excluded glyph *before*
consulting the skipper, since the digest is three shifts against a glyph id
already in hand where `Skipper::considers` reads the glyph's `GDEF` class, and
`at` filters the subtables as `gsub`'s `admitting` does.

Worth **1.22x-1.26x**, flat across every line length — see the ladder above.

The gain was first published here as "noise at 80 characters, rising to 1.40x
at 20,000", from a pair of runs one of which had overlapped a full workspace
build. Both halves of that shape were wrong, and both for the same reason: the
median. There is no rise with line length, and there is no dead spot at 80
characters. **Do not benchmark alongside a build, and do not benchmark in the
same invocation as one** — a `cargo test` that compiles first inflates the
measurement that follows it by ~10%, which was enough on its own to fake a
1.14x effect.

**How the numbers here were taken — and why every day-one figure in this entry
is about twice life size.** This is the most transferable thing the entry
contains, so it is written out rather than assumed.

Two runs of the *identical* instrument — the cost of shaping a 1,000-character
line, each the only test running, nothing else on the machine — came back
**776 us and 1,313 us**. A quantity that moves by 1.7x between runs cannot
settle a 1.3x question, and by that point this entry was full of 1.2x-1.4x
questions. So the instrument was pointed at itself:
`text::shaping_cost::is_this_instrument_stable` repeats one workload twelve
times inside a single process and prints the median and the minimum of each
block side by side, with the built-in face alongside as a control.

Across the twelve blocks the **median spread 1.89x and the minimum spread
1.02x**.

That is not a surprise once said out loud: the noise is **one-sided**. Nothing
can make a shaping finish sooner than the code allows, but a scheduler stall or
a clock-speed change can make it finish later. Every sample is therefore the
true cost plus a non-negative unknown, and the *smallest* sample is the best
estimate of the cost. The median estimates the cost plus the *typical*
interference — a property of the machine, not of the shaping. Ranking
configurations by median is ranking them partly by how busy the box was.

Two things were needed before the minimum could be trusted:

* **A floor on total sampling time, not just a sample count.** This host stalls
  every process for a millisecond or two at a time, several times a second. 51
  shapings of the built-in face is 0.7 ms of work — it fits *inside* one stall,
  so every sample is stalled and the minimum is stalled with them. That is
  exactly why the built-in column still spread 1.84x when the system column had
  already settled to 1.02x. `timed()` now samples until both the requested
  count and 20 ms of wall time are reached.
* **Batching for anything near the clock's resolution.** The minimum of 200,000
  timings of one font-cache hit printed a flat `0.000us` — the clock saying it
  cannot see this, not the lookup being free. Timed in batches of a thousand it
  reads 0.045 us.

With both in place the instrument reproduces to better than 1%: four separate
tests in one process measured the 1,000-character line at 750.7 / 749.9 /
753.4 / 748.7 us during a run whose median column was visibly wobbling by
1.82x. Across separate process invocations it holds to ~1.03x, provided the
invocation did not also compile something.

**The correction factor, measured.** Comparing the original median baseline
against the same configuration re-measured by minimum: 5,033/2,313 = 2.18x at
80 chars, 2.13x at 200, 2.14x at 1,000, 1.89x at 5,000, 1.74x at 20,000. So the
median roughly **doubled** everything, and — because it doubled the before and
the after alike — the *relative* claims made on day one mostly survived while
every absolute figure did not. The factor shrinks with workload size, which is
the same fact as the sampling floor above: a 1 ms stall is 100% of a 1 ms
shaping and 5% of a 20 ms one.

**The phase breakdown, re-taken (2026-09-02) — and this time the instrument
lives in the tree.** The table above was taken by editing `shape_with` in place
and throwing the edit away, which is why it went stale in two ways at once and
why nobody could re-take it without rewriting the patch. The replacement is
`gui/font/src/phase.rs`, behind `osfont`'s `phase-timing` feature, read out by
`guitk`'s `text::shaping_cost::shaping_phases`. Off, it compiles to nothing at
all — a unit struct with no destructor. On, each pass charges its own lifetime
to a thread-local total.

```
cargo test --release -p guitk --features phase-timing --lib shaping_phases \
    --target x86_64-pc-windows-gnu -- --ignored --nocapture --test-threads=1
```

(`--target` is not optional: the workspace defaults to the freestanding
`x86_64-slateos` target and a host test built against it fails in the
thousands.)

Best of 101 shapings, taken **per phase** rather than by keeping the fastest
shaping whole — a shaping quiet in every pass at once is far rarer than a quiet
pass:

| phase | 80 chars | 200 chars | 1,000 chars |
|---|---:|---:|---:|
| `gsub` | 41.9 us / **67.3%** | 99.5 us / **68.5%** | 481.1 us / **69.4%** |
| `norm` | 7.9 us / **12.7%** | 18.9 us / **13.0%** | 92.6 us / **13.4%** |
| `glyphbuild` | 3.5 us / 5.6% | 8.3 us / 5.7% | 39.3 us / 5.7% |
| `pre+script` | 2.6 us / 4.2% | 6.2 us / 4.3% | 30.2 us / 4.4% |
| `gpos` | 1.6 us / 2.6% | 3.2 us / 2.2% | 13.9 us / 2.0% |
| `joining` | 1.0 us / 1.6% | 2.4 us / 1.7% | 12.2 us / 1.8% |
| `tail` | 0.7 us / 1.1% | 1.7 us / 1.2% | 7.8 us / 1.1% |
| `marks` | 0.5 us / 0.8% | 1.1 us / 0.8% | 5.5 us / 0.8% |
| `advances` | 0.4 us / 0.6% | 0.9 us / 0.6% | 4.2 us / 0.6% |
| `segprep` | 0.2 us / 0.3% | 0.3 us / 0.2% | 0.4 us / 0.1% |
| `byte_levels` | 0.0 us / 0.0% | 0.1 us / 0.1% | 0.5 us / 0.1% |
| unaccounted | 2.0 us / 3.2% | 2.7 us / 1.9% | 5.7 us / 0.8% |
| instrumented total | 62.3 us | 145.3 us | 693.4 us |

The `marks` row is a new phase, split out of what the old table counted inside
`segprep`, and the split is why: `segprep` is per *segment* and measures 0.1%,
while `marks` is per *glyph* and was 8.2-8.7% before the fix below. Lumped
together they looked like one 8.9% item whose cost scaled with something
unclear. The `unaccounted` row is the table's check on itself — the passes are
laid out so no two are timed at once, and a `0.0` there would mean two had been
allowed to double-charge the same wall time.

**Three things the old table got wrong**, all of which would have been wasted
work:

* **`advances` is 0.6%, not 6.3%.** The entry named it a "plain caching
  candidate". Caching it would buy six parts in a thousand.
* **`gpos` is 2.0%, not 19.9% minus "roughly 3 points".** The third fix took
  far more out of it than the entry predicted, and there is nothing left there.
* **`gsub` is ~68%, not ~55%,** and it is now larger than everything else in
  the pipeline put together. It is the only phase whose share *grows* with line
  length.

**The fourth fix: the same digests, in the mark test.** `marks` — one "is this
glyph a combining mark?" per glyph of every shaped run, asked by the pass that
decides which glyphs take no room — measured **8.2-8.7%**, spent almost
entirely on searches that were always going to answer no. `MarkPositioning`
now carries a `could_be_mark` digest built once at parse time: the union of
both routes `is_mark` can take, `GDEF` class 3 and the mark coverage of every
subtable. A glyph the union excludes is one neither route could claim, so the
whole search is skipped in O(1). Marks are a small, tightly-clustered corner of
a face's glyph space and ordinary text contains none, so on real text this
answers "no" for essentially every glyph.

The union widens to `Digest::full()` if any part of it could not be read —
including when a subtable's coverage format is one the reader declines, where
an empty digest would in fact have been exactly right. Widening there costs
the shortcut for that face and cannot cost correctness; the alternative
couples the digest to the searcher's opinion about which formats are readable,
invisibly, with a dropped mark as the failure mode.

Worth **1.044x-1.059x** of the whole shaping, by a same-day A/B in which the
only difference between the two binaries is that one line:

| chars | digest disabled | digest live | ratio |
|---:|---:|---:|---:|
| 80 | 63.0 us | 59.5 us | 1.059x |
| 200 | 150.9 us | 143.9 us | 1.049x |
| 1,000 | 727.0 us | 696.2 us | 1.044x |

(The 5,000 and 20,000 rows moved by more than 10% between two runs of the
*same* binary and are not reported. The instrument samples those far fewer
times.)

**A share is an upper bound on what deleting the pass buys — new, and worth
carrying to the next optimisation.** The `marks` phase went from 8.5% to 0.8%,
which is 55.7 us of the 1,000-character shaping; the whole shaping got only
30.8 us faster. Nothing was mismeasured. A pass that walks the coverage tables
leaves them in cache for the passes after it, so part of what it is charged is
work its neighbours would otherwise have done themselves. Take the *ranking*
from the phase table and the *win* from `shaping_cost_by_line_length`, with the
change and without it.

**Regression cover for the fourth fix.** `mark.rs` gained two tests, and they
were checked by mutation rather than assumed: deleting the `GDEF` half of the
digest makes both fail with `glyph 900 was classified wrongly`.
`the_mark_digest_never_hides_a_mark` sweeps **all 65,536 glyph ids** against a
fixture whose marks sit at 900-902, 4,000 and 60,000-60,010 — scattered on
purpose, because ids 1-5 land in nearly the same bits of all three masks and a
test built from small ids passes whatever the digest says. The other test
covers the unreadable-class-definition path. All 29 host-installed-font tests
also pass, including the two whose failure mode is exactly a false negative
here.

**What is left.** Both continuations this entry originally named are done, so
is the `GPOS` one the re-measurement turned up, and so is the re-take that was
item 1. What remains:

1. **`GSUB` at ~68%** is now the whole question. It has had two rounds of work
   and a third will be harder than either — but everything else in the pipeline
   put together is less than a third of the cost, so no other target can pay
   more than 1.5x even if it went to zero.
2. **`norm::pieces` at ~13%** (`gui/font/src/norm.rs`) is the only other item
   above 6%, and is unexamined. A first read — **not confirmed, do not act on
   it without measuring** — is that the ASCII fast path still makes three whole
   passes over the text (`needs_work`, `thai::present`, `khmer::present`), a
   `char_indices().collect()`, and one `has_glyph` (`cmap`) lookup per
   character in `fit_to_face` that duplicates the `glyph_id` lookup
   `glyphbuild` then does again.
3. **Nothing below `glyphbuild` at 5.7% is worth attacking** until one of the
   two above lands. Six phases share 6% between them.

**Do not guess** — this entry's original `cmap` hypothesis survived a careful
reading of the code and was killed only by measurement, and the first phase
table's `advances` and `gpos` predictions were both wrong by an order of
magnitude.

**How this was found.** Scoping `TD-EDITOR-IS-NOT-BIDIRECTIONAL`, whose item 3
asks whether shape-whole-line-and-clip is affordable. The measurement that was
supposed to answer that question instead showed that *nothing* is affordable at
the current rate, so that entry's tradeoff cannot be settled until this is
fixed. **This is a prerequisite for that work**, not a side quest — and it is
almost certainly worth more than the bidi correctness it was gathered for.

**Note on the editor question it was gathered for.** At 29 us/char the answer
was trivially "cache it" — but that was an artefact of the bug, not a real
finding, so the ratio was re-taken after the fix. Shaping a whole line against
shaping only the ~200 characters that fit on screen:

| line length | whole | visible window | ratio |
|---:|---:|---:|---:|
| 200 | 155.7 us | 155.0 us | 1.0x |
| 1,000 | 753.4 us | 154.4 us | 4.9x |
| 5,000 | 4,079.7 us | 154.0 us | 26.5x |
| 20,000 | 17,314.4 us | 153.0 us | 113x |

(The shape of the answer has not changed once across three re-takings —
0.9x/6.0x/25.6x/124.7x after one fix, 1.0x/5.0x/25.2x/107.6x after three, and
these — because both columns move together. The ratio is a property of the line
lengths, not of how fast shaping is or of which statistic reports it.)

**The answer did not flip: byte-slicing is still worth its complexity**, but
the reason is now honest rather than a symptom. Below ~200 characters the
window costs the same as the whole line, so slicing buys nothing and the
simpler code is free. From ~1,000 characters up it is the difference between
0.15 ms and 0.75 ms *per line*, and a 50-line screen of 1,000-character lines
is 38 ms whole against 7.7 ms sliced.

**Shaping in pieces costs more than shaping whole** —
`whole_line_against_one_run_per_token`, which measures what the editor's
`draw_tokens` actually does (one shaped run *per syntax token*, each measured
to place the next):

| chars | whole | 5 pieces | 10 pieces | 20 pieces | 40 pieces |
|---:|---:|---:|---:|---:|---:|
| 80 | 64.4 us | 1.24x | 1.50x | 1.78x | 2.30x |
| 200 | 154.5 us | 1.13x | 1.26x | 1.31x | 1.48x |
| 1,000 | 749.9 us | 1.05x | 1.10x | 1.16x | 1.26x |

Monotone in the number of pieces and decreasing in line length, which is the
signature of a fixed per-shape cost: from the 200-vs-1,000 pair the marginal
cost of a character is ~0.75 us and the fixed cost of a shaping ~3.6 us, so a
line cut into *n* pieces pays that fixed cost *n* times. A 40-token line of
80 characters — an ordinary line of code — costs **2.3x** what shaping it whole
would.

An earlier version of this table, measured under a concurrent build, said the
opposite: that 40 pieces of a 200-character line cost 0.83x the whole. It was
wrong, and it was the load, not a real inversion. The correct reading is the
comfortable one: **shaping the line whole is both what bidi correctness
requires and the cheaper thing**, so item 4 of
`TD-EDITOR-IS-NOT-BIDIRECTIONAL` costs nothing to adopt and pays for itself.

**What this settles for the editor work.** Whole-line shaping is required by
`TD-EDITOR-IS-NOT-BIDIRECTIONAL` items 3 and 4 regardless — once colour is a
per-glyph attribute the renderer needs the whole line shaped anyway — so the
question was never "slice or not", it was "is the whole-line cost bearable
behind a cache". At 0.75 ms for a 1,000-character line and 0.15 ms for a
typical one, **it is**: a cache keyed on document revision pays that once per
edit rather than once per frame, and an edit is a human keystroke. That is item
(b) of that entry, and it is now unblocked — though see that entry for why (b)
is not yet *urgent*: `apps/editor` has no frame loop, so nothing pays a
per-frame shaping cost today.
