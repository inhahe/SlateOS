### TD-C-APPS-CARRY-1812-CLIPPY-FINDINGS-AND-588-OF-THEM-ARE-IN-PRODUCTION — 2026-09-03 — OPEN

**In short.** Running the project's own lint settings across every application
turns up 1,812 complaints. Two thirds are in test code and are noise a
three-line annotation silences. The remaining **588 are in real code**, and
most of them are arithmetic that could overflow or an array read that could run
off the end — the kind that turns a malformed input file into a crash or a
hang. They are spread over 26 of the 143 applications, so this is a pile in a
few places rather than a thin film everywhere.

**Why this was measured.** Three applications were wired to the compositor on
2026-09-03 and each was already failing `cargo clippy -- -D warnings` before it
was touched: a handful in `fontmanager`, 31 in `procexplorer`, 71 in
`musicplayer`. Three for three looks like a tree-wide condition, and I wrote it
up as one. **It is not**, and the difference matters for planning:

| | |
|---|---|
| crates under `apps/` | 143 |
| with any finding | **29** |
| with a *production* finding | **26** |
| **clean** | **114** |

The impression came from a biased sample. The apps left to convert are the ones
nobody has been into recently, and "nobody has been into it recently" is the
same property that predicts lint debt. Measuring is what separated the two.

**The numbers.** `cargo clippy -p <every app> --all-targets` on 2026-09-03:

| | count |
|---|---|
| total findings | 1,812 |
| in `#[cfg(test)]` modules | 1,163 |
| **in production code** | **588** |

Production findings by lint:

| lint | count |
|---|---|
| `arithmetic_side_effects` | 410 |
| `indexing_slicing` (indexing) | 133 |
| `indexing_slicing` (slicing) | 15 |
| everything else | 30 |

**Thirteen crates are done** as of 2026-09-04 — `paint`, `soundrecorder`,
`habits`, `markdowneditor`, `regextester`, `explorer`, `installer`, `finance`,
`weather`, `reminders`, `flashcards`, `mindmap` and `logviewer` — taking **522
production findings out of the 588 (89%)**. All but one were converted to
`oswindow::app` in the same pass, since the trigger below says to take a
crate's debt when converting it; `installer` is a CLI tool with no window, so
it got the lint half alone.

Worst crates by *production* findings: ~~`paint` 135~~, ~~`soundrecorder` 55~~,
~~`habits` 43~~, ~~`markdowneditor` 40~~, ~~`regextester` 39~~, ~~`installer`
33~~, ~~`explorer` 33~~, ~~`finance` 23~~, `metronome` 21, ~~`weather` 21~~,
~~`reminders` 21~~, ~~`flashcards` 18~~, ~~`mindmap` 17~~, ~~`logviewer` 14~~.

**`installer` had been reporting a count that was not its count.** Its
`build.rs` embeds a Windows manifest and did so with an `expect`. Under
`-D warnings` clippy stops at the *build script*, before it compiles the crate
at all — so `cargo clippy -p installer -- -D warnings` failed on one line in
`build.rs` and never analysed `lib.rs` or `grub.rs`. The 33 above came from the
survey's `--all-targets` run; a bare per-crate check made the crate look like it
had a single trivial finding. **Worth checking elsewhere:** any crate with a
`build.rs` that trips a lint is hiding its whole body behind that one line.


**Note that `metronome` is on that list**, and it is one of the three apps that
were *already* converted before today. Wiring an app to the compositor does not
imply anyone looked at its lints; the two jobs are independent.

**Why the test-module two thirds are not the interesting part.** A test that
indexes out of range should fail loudly and point at the line that did it —
that is the diagnosis, and `CLAUDE.md` says as much. Those 1,163 are closed by
adding the standard allow block to each test module, which is mechanical and
carries no risk.

**Why the 588 are worth real work.** They are not stylistic. The three fixed on
2026-09-03 are the argument:

- `musicplayer`'s WAV chunk loop advanced by `offset += 8 + chunk_size`, with
  `chunk_size` read out of the file. A crafted size wraps the cursor back to a
  small number, which passes the loop guard, and the parser reads the same
  chunks **forever**. `parse_id3v2` had the identical shape. A media player
  that hangs on a malformed file is a denial of service delivered as an email
  attachment.
- `procexplorer` indexed `visible[0]` and computed `visible.len() - 1` under a
  guard three lines away.
- `fontmanager`'s `uninstall` indexed `self.fonts[idx]` the same way.

Each was safe *on the day it was written*, because of a check somewhere nearby.
That is precisely the guarantee that stops holding when someone moves the check,
and it is why the workspace turns these lints on at all.

**Proper fix.** Per crate, in this order, because the ratio is roughly 2:1 in
favour of the cheap half:

1. Add the standard allow block to the crate's `#[cfg(test)]` modules. Closes
   about two thirds of any crate's count for three lines.
2. Fix the production findings properly — `checked_add`/`saturating_*`/`get`,
   with the bound stated in the operation rather than in a guard a few lines
   away. Do **not** blanket-allow these; a crate-wide allow is what
   `C-CREDMANAGER-ALLOWS-DEAD-CODE-CRATE-WIDE` is about, one lint over.
3. Look hardest at anything parsing a file or a network response. That is where
   the 410 arithmetic findings stop being theoretical.

**Reproduce**, and re-measure rather than trusting the numbers above:

```bash
pkgs=$(for d in apps/*/; do printf -- "-p %s " "$(basename "$d")"; done)
cargo clippy $pkgs --all-targets --target x86_64-pc-windows-gnu 2>&1 \
  | tee /tmp/clippy-all.log | grep -c '^warning:'
```

Splitting production from test needs the first `#[cfg(test)]` line of each file;
the throwaway script that did it is not kept, because a script kept without a
caller is the other defect this file is full of.

**Trigger:** take a crate's debt when converting that crate to `oswindow::app`
— the two passes touch the same files and the lint tail is most of the work
either way (see `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR`). `paint` at 135 is the
one worth doing on its own.

### `apps/paint` done 2026-09-03 — 136 to 0, and the pass found a 687 GB allocation

**Not by adding 136 `saturating_*` calls.** Ninety-eight of them were in the
rasterizer — Bresenham ellipse steps, rounded-rect corners, line drawing — and
saturating there would be *worse than the lint*: a step that saturated would
draw a wrong shape in silence, where an overflow at least announces itself in a
debug build. The fix for a rasterizer is to bound its inputs and then let the
arithmetic be total.

So: `MAX_CANVAS_DIMENSION` (16384/side, chosen as the GPU texture limit) plus
`MAX_CANVAS_PIXELS` (64 Mpx), enforced in `PaintApp::set_canvas_size`, which is
now the **only** writer of `canvas_width`/`canvas_height`. The eleven rasterizer
functions carry `#[allow(clippy::arithmetic_side_effects)]` with the headroom
stated: at 16384 the largest intermediate any of them forms is `2 * r * r` =
5.4e8 against `i32::MAX` of 2.1e9. `the_canvas_bound_leaves_the_rasterizer_inside_i32`
computes that in `i64` and fails if the constant is ever raised without
re-reading the rasterizer — so the suppression has a test under it rather than
a comment.

**Two bounds and not one, because they answer different questions.** Per-side is
what the rasterizer's arithmetic needs. It does not bound the *memory*: 16384 x
16384 is 268 Mpx, a gigabyte per layer. The area cap is what bounds that, and
the per-side bound cannot do its job without being set so low it refuses an
ordinary 600 dpi scan.

**The test found a real defect while being written.** `new_canvas` clamped the
two fields and then allocated its layer from its own *arguments* — so
`new_canvas(u32::MAX, u32::MAX)` asked the allocator for **687 GB** and killed
the process. Bounding the record of a size is not bounding the memory.
`set_canvas_size` now *returns* the size it settled on, and `new_canvas` and
`resize_canvas` allocate from that; returning it rather than leaving callers to
read the fields back is what makes the mistake hard to repeat. The other 38
findings were ordinary logic arithmetic and are `checked_*`/`saturating_*`.

Worth generalising, because it is the second time today the same shape has
appeared: **a bound that is not on the path the memory is allocated from is not
a bound.**

**If never fixed:** 143 applications ship with 588 unchecked operations, mostly
arithmetic, in code that reads user files. Most will never be reached. The ones
that are will be reached by whoever is looking for them.
