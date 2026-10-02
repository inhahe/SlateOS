## 1300. A partial frame is exactly a full frame: one buffer, a buffer-age contract, and a clipped, window-major repaint

**Date:** 2026-09-24
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** the compositor saves work by redrawing only the parts of the
screen that changed. That shortcut was broken in four ways at once, and a
test that compares every shortcut frame against a full redraw of the same
scene found all of them: content that had just changed flickered back to its
old state, a window could paint itself over the window above it, the frosted
"blur" effect never ran at all, and a moved window left a faint trail of
shadow. This entry records how the redraw now works so that it cannot drift
from a full redraw again — and the one real trade-off in it, which is that the
software renderer now keeps one screen's worth of pixels instead of two.

### What was wrong

`Compositor::compose_frame`'s partial path, measured against a full recomposite
of the same scene (`partial_frames_composite_exactly_what_a_full_frame_would`):

1. **The buffer drawn into was two frames stale.** `Framebuffer` was a
   front/back pair swapped on every present. A partial frame repaints only this
   frame's damage — into the buffer that last held the frame *before* the
   previous one — so every change the previous frame made outside this frame's
   damage reverted. A window updated in frame N showed its old content again in
   frame N+1 if anything else on the screen changed.
2. **A window reaching the damage was redrawn whole.** `render_damaged_windows`
   cleared each damaged rectangle, then re-rendered every overlapping window
   *unclipped*. A lower window that merely touched the damage painted itself over
   every window above it that the damage did not reach, and its translucent
   edges (shadow, rounded corners) were blended a second time onto their own
   last-frame copies.
3. **The backdrop blur never ran.** `fc54f3ac2` wired `blur_behind_window` into
   `render_all_windows`' branch for compositing *without* the occlusion cull,
   which production never takes. Its tests called the pass directly and so
   stayed green.
4. **Damage stopped short of the shadow.** `damage_window` used
   `Window::outer_rect`, but the shadow is cast 3 px down and right of the frame
   and so reaches 2 px past the outer rect on those sides.

### The decisions

**The software framebuffer is one buffer.** The alternatives, all of which
also fix fault 1:

| | cost per frame | memory at 4K | kept |
|---|---|---|---|
| front/back pair, copy the previous frame's damage forward after each swap | a `memcpy` of last frame's damage | 66 MB | no |
| front/back pair, repaint this frame's *and* last frame's damage (age 2) | re-rendering last frame's damage | 66 MB | no |
| **one buffer (age 1)** | **nothing** | **33 MB** | **yes** |

The pair bought nothing: every `Present` implementation copies the finished
frame out synchronously inside the loop iteration that composited it — the DRM
presenter into its own double-buffered scanout memory, the host window through
`StretchDIBits` — so nothing ever reads the presented frame while the next is
drawn. **Revisit if** a presenter ever reads the frame asynchronously (a
presenter thread, or a capture path that holds a reference across frames): that
presenter must then own a copy, or the compositor must go back to a ring.

**`RenderTarget::buffer_age` is part of the backend contract,** with the
compositor keeping a short `DamageHistory` (the `EGL_EXT_buffer_age` model): a
target reports how many presents ago the buffer about to be drawn into held the
current frame, and the compositor repaints this frame's damage plus that many
frames' worth of history — or everything, if it cannot answer. A ring survives
in `Framebuffer::with_ring` because the targets that genuinely are
multi-buffered are coming — a GPU swapchain, or composing straight into the DRM
scanout pair, which is the proper fix for `TD-COMPOSITOR-COPIES-EVERY-FRAME-TWICE`
— and the history is tested against real pixels over rings of two and three.
What is *remembered* is each frame's own change, not its repaint; remembering
the repaint would make each record contain the one before it, and on a ring the
region would grow without bound.

**The repaint walks windows outermost, rectangles innermost, and clips every
draw** to a disjoint region (`gui/compositor/src/repaint.rs`). Rectangle-major
order is the obvious shape and is wrong for a blurred window, whose blur reads
the backdrop across its whole frame — possibly several rectangles, some not yet
repainted. Disjointness is load-bearing: each translucent layer is blended once
per rectangle, so overlapping rectangles would double-blend it.

**Damage that touches a blurred window repaints all of it** (to a fixed point,
since one blurred frame can reach another), and **no opaque cover culls the
backdrop inside a blurred window's frame** if the cover belongs to that window
or one above it. The blur rewrites the whole frame from what is beneath it, so
it can only run over a frame repainted entirely and fully drawn underneath. The
cost is repainting a menu's or a taskbar's whole rectangle when anything under
it changes — small surfaces, which is what blur is used on. The own-cover half
of this was found by the property test in the *full* path: a blurred window
with opaque content culled the backdrop under itself, so its blur read its own
previous frame and every full frame drifted with nothing changing.

**Damage is `window_drawn_extent`,** the same bound the repaint trusts to
contain a window's drawing, so the two agree by construction.

**A shared buffer's opacity is measured from its pixels,** not assumed from its
format. Needed because direct scanout now requires an opaque buffer (it hands
the display the client's pixels as they are, so a translucent one would lose
its alpha), and an `Argb8888` buffer that never uses its alpha — the ordinary
case for video and games — was being treated as translucent everywhere: never
an occluder, never the memcpy blit, never scanned out.

**The first frame after direct scanout is drawn whole.** A bypass frame drops
its damage without drawing it, so the framebuffer stops describing the scene;
`framebuffer_stale` records that without also making an idle fullscreen window
re-present every frame, which folding it into `full_recomposite` would have.

### How it is held

- `partial_frames_composite_exactly_what_a_full_frame_would` and its two ring
  siblings: two compositors, one random scene script (windows created, drawn,
  moved, resized, raised, faded, hidden, minimized, destroyed; some translucent,
  undecorated or blurred), one repainting only what changed and one repainting
  everything; every pixel of every frame must agree. The long form,
  `partial_frames_match_full_frames_over_many_scenes` (ignored by default; run
  it in release after touching the repaint), is 48,000 frames over ring depths
  one to four.
- One small test per fault, so a regression names its cause.
- `a_frame_repaints_only_its_damage`, which counts the repaint region's pixels
  rather than timing it. (A `FrameStats::repainted_pixels` field did this for
  a day and was removed: nothing in production read it, and
  `check-fields-written-never-read.py` refused the push, rightly.)

### How to reverse

Each piece is separable. Going back to a front/back pair is
`RenderBackend::software` constructing `Framebuffer::with_ring(w, h, 2)` — the
history makes it correct, at the costs in the table above.
