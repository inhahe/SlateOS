## §488 — The compositor's rendering seam is cut at the primitive, not at the pixel and not at the scene

**Date:** 2026-08-20
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The compositor draws every pixel with the CPU. To ever use the
graphics card it needs a *seam* — one named boundary where "decide what the
screen should look like" ends and "actually put colour on it" begins, so a
graphics-card implementation can be slotted in below without rewriting
everything above. The question was where to cut that boundary. Cutting it too
low (one call per pixel) describes something no graphics card can do; cutting it
too high (one call per whole frame) makes every future implementation
re-calculate window borders, shadows and title-bar layout for itself, which is
how two implementations end up drawing subtly different desktops. The cut was
made in the middle: the compositor hands down *shapes* — a filled rectangle, a
line, a letter's coverage stencil, a client's image — which is exactly the
vocabulary a graphics card already speaks.

### The three candidate seams

| Seam | The backend is asked to… | Why it was or wasn't chosen |
|---|---|---|
| **Per-pixel** (`blend_pixel`) | blend one pixel at coordinates | Rejected. A GPU that services one pixel per call is not being used as a GPU; the round-trip dwarfs the work. Any GPU backend would immediately have to *re-batch* the pixels back into shapes it was just handed apart. |
| **Per-scene** (`backend.compose(&scene)`) | take the window list and produce a frame | Rejected. Decoration geometry (shadow inset, border width, title-bar height, button rects), the occlusion cull, damage tracking and the clip stack would all have to be reimplemented per backend. Two implementations of that arithmetic is two answers to "where is the close button", and the existing code comments already treat that drift as the hazard to guard against. |
| **Per-primitive** (chosen) | fill this quad; stroke this line; stamp this coverage mask; blit this texture | A GPU executes precisely these. The CPU backend also executes precisely these — it is what the rasterizer was already doing internally, just not behind a name. |

### What the cut puts on each side

Above the seam, written once for every backend that will ever exist: window
management, damage tracking, the occlusion cull, decoration geometry, the clip
and translate stacks, and text **shaping**. Shaping stays above deliberately —
a GPU backend wants the same `GlyphMask`es, uploaded to an atlas; re-shaping
text per backend would mean two implementations of kerning and line breaking.

Below the seam: only putting colour on the surface.

### The consequences that had to be accepted

- **`fill_rect` takes no per-call clip.** For an axis-aligned quad the
  intersection with the clip *is* the clip, so it is computed above the seam and
  a fully-clipped fill never reaches a backend at all. Lines and glyphs do carry
  their clip down, because clipping them is not a rectangle intersection.
- **There are two different clips, and they had to be named apart.** The *frame
  clip* is backend state (the compositor's damage/occlusion cull, set once per
  region); the *draw clip* is the client's clip stack, passed per call. Conflating
  them would have let a client's clip survive into the next window's drawing.
- **`RenderBackend` is an enum, not `dyn RenderTarget`.** The pipeline is generic
  over `T: RenderTarget + ?Sized` — monomorphised, zero dispatch — and the enum
  contributes one predictable branch per primitive at the single point where the
  compositor selects its backend. This matches the codebase's standing "enum
  dispatch on hot paths, no `dyn Trait`" convention and CLAUDE.md's
  anti-pattern list.
- **`as_software()` returns an `Option`, on purpose.** Pixel-readback tests and
  the headless present path genuinely need the CPU framebuffer. Making that an
  `Option` keeps it an *escape hatch a caller must acknowledge* rather than an
  assumption the compositor is free to make everywhere.
- **`Compositor.framebuffer` was renamed to `Compositor.backend`,** not merely
  retyped. The name is the enforcement: a field called `framebuffer` invites the
  next reader to assume the pixels are in local memory, which on a GPU backend
  they are not.

### Why a one-variant enum is not a no-op

A trait with exactly one implementation, forwarded to by an enum with exactly
one variant, proves nothing on its own — the pipeline above could still be
quietly software-specific, and nobody would find out until a GPU backend was
half-written and wedged. So the seam ships with a **second** implementation of
`RenderTarget` in its test module: a `Recorder` that owns no pixels at all and
records the primitives it is handed. The real `RenderEngine` is driven through
it. That is what actually pins down the properties claimed above — that the
client-command pipeline names no `Framebuffer`, that a clipped-away quad never
crosses, that a stroke crosses as four quads rather than as an outline each
backend must interpret, and that a line crosses *whole* (a client asking for
`x2 = 2_000_000_000` cannot make any backend walk two billion steps). Deliberately
*not* a `RenderBackend` variant: it is a test double, and making it a shipped
variant would put a do-nothing backend one typo away from being selected.

### Alternative not taken: build the GPU backend first and extract the seam after

Tempting, because the seam's shape would then be derived from a real second
consumer rather than from anticipation. Rejected because the GPU backend is
blocked below lane C — nothing under `kernel/` issues a rendering command to the
virtio-gpu render node (`virtgpu_uapi.rs` is byte-exact ABI and nothing else), so
"build it first" means "build nothing". The seam is the part that was unblocked,
it is a prerequisite whichever way the stack below is eventually built, and its
correctness is testable today against a non-pixel backend. The lane-A dependency
is filed as `requests/c-a-virtgpu-render-ioctl-dispatch-blocks-compositor-gpu.md`.
