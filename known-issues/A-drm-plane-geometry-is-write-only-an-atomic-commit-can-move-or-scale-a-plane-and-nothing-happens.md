### [A] DRM plane geometry is write-only: an atomic commit can move or scale a plane and nothing happens -- 2026-09-17

**Status:** OPEN

**In short:** a graphics "plane" is a layer the display hardware can place
and scale on screen -- how a cursor or a video overlay gets positioned. The
kernel accepts the rectangle a compositor asks for, stores it, reports
success, and never uses it. Moving or scaling a plane does nothing.

The second dd-950 cluster, found the same way as the power family: group
`kernel/`'s 130 unread fields by file and ask what one absent consumer would
read a cluster. `drm/plane.rs` held 6 of them, and they are a single
coherent set -- `src_w`, `src_h`, `dst_x`, `dst_y`, `dst_w`, `dst_h`: the
source rectangle in framebuffer coordinates and the destination rectangle in
CRTC coordinates.

**Every occurrence, enumerated.** For `dst_w`:

| site | kind |
|---|---|
| `drm/plane.rs:51` | the declaration |
| `drm/atomic.rs:423` | `plane.dst_w = dst.w` -- the atomic commit path |
| `drm/mod.rs:616` | `p.dst_w = kernel_mode.hdisplay` |
| `drm/driver.rs:195`, `:487` | struct initialisers |
| `drm/ati/backend.rs:232` | struct initialiser |

Five writes, one declaration, **zero reads** -- confirmed tree-wide, not just
within `drm/`: `grep` for `.dst_w` across `kernel/src` returns 2 occurrences,
both assignments, and 0 that are not.

**Why the atomic path makes this worse than dead state.** `atomic.rs:423` is
the commit handler storing the rectangle a client requested. So the
userspace-visible sequence is: set a plane's position or scale via the atomic
API, receive success, observe no change. That is dd-945's shape again -- the
result is read by a compositor that believes the commit took effect -- and it
is reached through an API rather than a `/proc` file, so there is no header
to put a disclosure in. The honest fix is either to honour the rectangle in
the scanout path or to refuse a commit that sets one, and refusing is the
smaller change.

**Not fixed here.** Unlike the power family, where a `/proc` disclosure is
cheap and correct, this needs a decision about the atomic API's contract:
silently accepting a no-op is wrong, but returning an error for a field
compositors routinely set may break callers that currently "work". Recorded
with the evidence; the fields stay, per dd-950 -- they are the shape of the
plane composition that was never wired, and `src_*`/`dst_*` is exactly what
that code would read.
