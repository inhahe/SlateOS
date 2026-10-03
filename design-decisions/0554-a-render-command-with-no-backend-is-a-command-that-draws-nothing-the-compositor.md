## 554. A render command with no backend is a command that draws nothing; the compositor now owns an image store

**Date:** 2026-08-25
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The GUI toolkit has always let a program say "draw picture number
7 in this rectangle." The compositor — the program that actually paints the
screen — never learned how, so every such instruction was quietly thrown away
and nothing appeared. That is why the file manager's icon view is blank. The
compositor can now be handed a picture along with a number to call it by, and it
paints it, scaled, wherever the instruction asks. The alternative was to keep
routing around the hole, which would have meant shipping a view that is known
not to work.

### What was wrong

`RenderCommand::Image { x, y, width, height, image_id }` has existed in
`gui/toolkit/src/render.rs` since the command list was designed. Three places
emit it (`gui/toolkit/src/grid.rs` twice, `apps/explorer/src/thumbs.rs` once).
The compositor's executor arm for it read, in full:

```rust
RenderCommand::Image { .. } => {
    // Image rendering requires an asset store — stub for now.
}
```

So the command was *accepted* — no error, no log line, no failed frame — and
drew nothing. This is Lesson 45 ("a feature with no production caller is a
feature that does not exist") at a level below the caller: here there *were*
callers, and a seam that swallowed them. A missing implementation that returns
an error is a bug someone finds in a minute; one that silently succeeds is a
bug that survives until somebody looks at a blank pane and wonders why.

### The decision

Give the compositor an image store, at the same trust boundary as the existing
shared-buffer path, and implement the command.

**Alternative considered: carry the pixels in the command.** `RenderCommand`
would gain the bytes, and no store would be needed. Rejected because **upload
and draw have opposite frequencies.** A thumbnail is produced once and drawn on
every frame the folder is visible; a protocol that carried its megabytes
alongside the draw would re-send them at 60 Hz. This is the same split as a
texture upload versus a textured quad, and it is a split for the same reason.

**Alternative considered: route the explorer around it** — have the icon view
draw only `render_placeholder` and emit `Image` commands that provably draw
nothing. Rejected outright: it ships a known-broken view and leaves the defect
in place for the next caller to trip over. `CLAUDE.md`: *"Never defer a
fundamental fix for a convenient hack."* Both `gui/toolkit` and `gui/compositor`
are lane C, so nothing about the fix was out of reach.

### The four calls that shape it

**1. `ImageAsset` is a distinct type from `SharedBuffer`, but they share their
validator.** The two hold the same thing — validated, normalised ARGB8888 pixels
from an untrusted client — so `SharedBuffer::import`'s entire body became a
free function `normalize()` that both call. A second copy of that function would
be a second place for the stride check to be got wrong, and only one of the two
would have the tests. What they do *not* share is the contract around the
pixels, which differs in every respect: one per window versus many-with-ids;
lifetime-until-next-attach versus until-unregistered; placed by the window's own
geometry versus by a render command; never scaled versus scaled; a per-frame
release protocol versus none. Merging them would make the release notification —
whose absence deadlocks a client waiting to draw — ride on a field half the
callers must remember to ignore.

**2. Image ids are scoped to the window, not the compositor.** The id is the
*client's* to choose, so two independent programs that both number from 1 must
not collide — and a compositor-wide map would make them collide. Window
ownership also means a client that exits without unregistering does not strand
its pixels: they drop with the window. `HashMap` costs nothing until used, and
almost every window has none.

**3. The destination rectangle crosses the seam unclipped, with the clip
alongside.** `RenderTarget::draw_image(image, dest, clip, opacity)`. Shrinking
`dest` to the visible part first would resample the image at a scale that
depended on how much of it happened to be on screen, so a window sliding off the
edge would visibly squash and ripple rather than slide.

**4. The software rasteriser iterates the destination and samples the source.**
Cost is then proportional to pixels actually painted — an icon-view thumbnail is
a 256×256 image drawn at 64×64, and the source walk would read sixteen pixels
for every one it kept. It is also the only order that fills without gaps when
the image is drawn *larger* than itself. All the coordinate arithmetic is in
`i64`: `dest` reaches the compositor from a client render command by way of an
`f32` cast, and `py - dest.y` in `i32` overflows outright for a `dest.y` near
`i32::MIN`, with the multiply by source height overflowing for far less.

### What an unresolved id does

Nothing, silently. A client whose upload is still in flight, or whose asset was
dropped to reclaim memory, is in an ordinary transient state rather than a
fault — and a log line per unresolved id would be one *per frame* for as long as
it lasted. This is deliberate and is the one place the new code keeps the old
stub's behaviour; the difference is that it is now the documented handling of a
specific case rather than the handling of every case.

### Where it lives

`gui/compositor/src/buffer.rs` (`normalize`, `ImageAsset`),
`gui/compositor/src/render.rs` (`RenderTarget::draw_image` and the backend
forwarding arm), `gui/compositor/src/lib.rs` (`Window::images`, the
`Framebuffer` rasteriser, and `Compositor::{register_image, unregister_image,
image_count, image_bytes}`).

### What is still missing

`register_image` is reachable in-process only. The client-facing wire protocol
(`gui/remote/src/control.rs` → `RequestBody`) has no image-upload request, so a
program in a *separate process* still cannot upload one. That is the next step
and is logged in `known-issues.md`.
