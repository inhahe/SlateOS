## §438 — The font engine — own the bytes, reject CFF loudly, no hinting, signed-area rasterization

**Date:** 2026-08-13
**Lane:** C

**Decided by:** Claude (autonomous)

**Context.** Nothing in the tree could open a `.ttf`. The entire UI — desktop,
toolkit, compositor, every app — was capped at one procedurally generated 8x16
bitmap face, which means one size, no anti-aliasing, and no script outside the
handful of ranges that face was written to cover. The `[C]` roadmap item "2D
drawing library: Vello + HarfBuzz" has a GPU-dependent half (blocked on lane
A's DRM/KMS and GPU driver) and a GPU-independent half — reading font files and
turning glyphs into coverage — which is not blocked on anything. That half is
`gui/font/src/{sfnt,raster,scaled}.rs`. Four decisions inside it had real
alternatives.

### 86a. `Face` owns its bytes rather than borrowing them

**Decision.** `Face::parse(data: Vec<u8>)` takes ownership; the table spans are
offsets into the owned buffer, not `&[u8]` slices.

**Rationale.** The natural callers — a font cache, a `ScaledFont`, a per-display
font set — all outlive whatever read the file. A borrowing parser would push a
lifetime parameter into every one of them, and into every struct that holds one,
for no benefit: nobody has a font's bytes already resident and wants to avoid the
copy.

**Alternative considered.** `Face<'a>` borrowing a `&'a [u8]`, as `ttf-parser`
does. Genuinely better for a caller that mmaps a font file and wants zero copies,
and we may want exactly that once there is a real filesystem-backed font
directory. Rejected for now because the lifetime infects the entire GUI stack and
the copy is one memcpy per font per boot.

**How to reverse.** Add a borrowing `FaceRef<'a>` beside `Face` sharing the same
table-offset logic; `Face` becomes a thin owning wrapper over it. The parsing
code is already written against offsets, so this is mechanical.

### 86b. CFF/Type 2 outlines are a hard error, not an empty glyph

**Decision.** A face whose outlines live in `CFF `/`CFF2` rather than `glyf` is
rejected at `parse` time with `SfntError::CffOutlinesUnsupported`.

**Rationale.** The alternative — open the face and return an empty outline for
every glyph — produces a font that *appears* to work and draws nothing. That
failure surfaces far from its cause and reads at the call site as a rasterizer
bug. Failing at `parse` names the actual limitation, and the caller can fall back
to another face or tell the user.

**Cost, measured.** 18 of the 556 fonts installed on the dev host are CFF (~3%),
but that 3% is most Adobe faces and much of what a user installs by hand.
Tracked as `TD-FONT-NO-CFF-OUTLINES` in `known-issues.md` with the shape of the
real fix.

**How to reverse.** Implement the Type 2 charstring interpreter; the error
variant then becomes unreachable and can be deleted.

### 86c. No TrueType hinting interpreter

**Decision.** `fpgm`/`prep`/`glyf` instruction streams are ignored entirely.
Outlines are scaled and filled as the designer drew them.

**Rationale.** Hinting is a full stack-machine bytecode interpreter — a large,
security-sensitive attack surface (it has historically been a rich source of
remote-code-execution bugs in FreeType and in Windows' font driver) executing
attacker-supplied programs, in exchange for sharper stems at small sizes on
low-DPI displays. FreeType's own default has been the unhinted/auto-hinted path
for years, and the displays this OS targets are high-DPI, where the benefit
largely evaporates. Not writing an interpreter is both the safer and the cheaper
choice.

**Alternative considered.** A vertical-only autohinter (snap horizontal stems to
the pixel grid without running font bytecode) — no attacker-controlled execution,
most of the small-size benefit. Worth doing if small text on a 1080p panel looks
soft. Deliberately deferred, not rejected.

**How to reverse.** Additive: an autohinter is a pass over the `Outline` between
`Face::outline` and `rasterize`, so it needs no change to either.

### 86d. Signed-area accumulation rather than a scanline/active-edge rasterizer

**Decision.** One `f32` accumulator per pixel; each segment deposits *signed*
area only into the cells it crosses; a per-row prefix sum recovers coverage;
`abs().min(1.0)` gives non-zero-winding fill. After Raph Levien's `font-rs`.

**Rationale.** It is branch-light, allocation-free after one buffer, trivially
correct for the non-zero winding rule TrueType requires (counter-wound contours
cancel to zero and become holes without any special casing), and it produces
true analytic anti-aliasing rather than a supersampled approximation. A classic
active-edge-table rasterizer needs per-scanline edge lists, sorting, and explicit
winding bookkeeping — more code and more allocation for the same output.

**Divergences from `font-rs`, both because our input is untrusted.** Every
accumulator write goes through a bounds-checked `add()` that *clips* rather than
clamps (folding an off-left edge's area onto column 0 would paint a spurious
vertical bar), and `into_coverage` resets the running sum per row rather than
sweeping linearly (so a clipped row cannot bleed a solid bar into the next).

**How to reverse.** `rasterize()` is a pure function from `&Outline` and a scale
to a `GlyphMask`; swapping the algorithm touches nothing else.

**Where it lives.** `gui/font/src/sfnt.rs` (86a, 86b, 86c),
`gui/font/src/raster.rs` (86d), `gui/font/src/scaled.rs` (the caching layer
above both), `gui/font/tests/host_fonts.rs` (the 556-font sweep that measured
86b).
