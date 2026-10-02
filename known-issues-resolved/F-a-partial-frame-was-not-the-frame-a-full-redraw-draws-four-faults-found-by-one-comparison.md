### [F] A partial frame was not the frame a full redraw draws: four faults, found by one comparison -- 2026-09-24

**Status:** ✅ FIXED 2026-09-24 (lane F) — `gui/compositor/src/{lib,render,repaint,buffer}.rs`; design-decisions §1300.

**In short:** the compositor redraws only what changed on the screen, and that
shortcut was wrong four ways at once. Something you had just changed could
flicker back to how it was, a window could paint itself over the window on top
of it, the frosted-glass blur on menus and the taskbar never ran at all, and a
moved window left a faint two-pixel trail of its shadow. None of it was caught
because no test ever compared a shortcut frame against a full redraw of the same
scene. One does now, and it found all four — plus a fifth fault in the blur that
the fix itself introduced and the full redraw exposed.

**How it was found.** Building the mouse pointer (C-Q18), whose every move would
be a small partial frame, meant first asking whether partial frames were right.
A probe answered in one step: window A drawn red in frame 2 was white again in
frame 3 because frame 3 changed only window B.

| fault | where | what a user saw |
|---|---|---|
| the buffer drawn into was two frames stale | `Framebuffer` swapped a front/back pair on every present, so a partial frame drew into the frame before last | an update made one frame earlier reverted whenever anything else on screen changed |
| a window reaching the damage was redrawn whole | `render_damaged_windows` re-rendered overlapping windows unclipped | a lower window painted over a higher one; translucent edges darkened |
| the backdrop blur never ran | `fc54f3ac2` wired it into the no-occlusion-cull branch, which production never takes; its test called the pass directly | menus, notifications and the taskbar were never blurred |
| damage stopped short of the shadow | `damage_window` used `outer_rect`; the shadow is cast 3 px down-right | a moved window left the last 2 rows/columns of its shadow behind |
| *(introduced by the first fix, caught by the test)* a blurred window with opaque content culled the backdrop under itself | `StackPlan::occluders_above` excluded covers *above* a blurred window from culling its backdrop, but not its own | its blur read its own previous frame, so every **full** frame drifted with nothing in the scene changing |

**The fix** (design-decisions §1300): a one-buffer software framebuffer; a
`RenderTarget::buffer_age` contract with a damage history, so a multi-buffered
target (a GPU swapchain, or the scanout pair `TD-COMPOSITOR-COPIES-EVERY-FRAME-TWICE`
wants to compose into) is correct by construction; a repaint that walks windows
outermost over a disjoint region and clips every draw; damage spread to the
whole frame of any blurred window it reaches; damage equal to the drawn extent.
Two further corrections rode along: direct scanout now requires an opaque buffer
(it was handing translucent ones to the display with their alpha dropped), and a
buffer's opacity is measured from its pixels rather than assumed from its format.

**What holds it:** `partial_frames_composite_exactly_what_a_full_frame_would`
and its ring-of-two and ring-of-three siblings (two compositors, one random
scene script, every pixel of every frame compared), plus
`partial_frames_match_full_frames_over_many_scenes` (ignored; 48,000 frames in
release, green) and one small test per fault.

**The lesson worth keeping.** The shortcut and the long way round were two
implementations of one function, and nothing compared them. The comparison is a
dozen lines once both exist — and this time it also caught the oracle being
wrong, which is what an oracle that is merely *trusted* never does.
