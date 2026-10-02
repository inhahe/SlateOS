## `TD-C-AN-IMAGE-CAN-ONLY-BE-UPLOADED-IN-PROCESS` (lane C, 2026-08-25) — **FIXED** 2026-08-26

**In short:** the compositor could draw pictures, but the only way to hand it
one was to be *inside* the compositor program. Every real application is a
separate program, so none of them could show a picture at all — the file
manager's icons would simply not appear, with no error to say why. There is now
a request on the wire (`UploadImage`, and `DropImage` to take it back again), a
`Connection::upload_image` / `Connection::drop_image` pair for applications to
call, and a per-connection memory limit so that a program which uploads without
end is refused rather than allowed to exhaust the machine.

**What was built.**

| Piece | Where |
|---|---|
| `RequestBody::UploadImage` / `DropImage` (tags `0x15`/`0x16`) | `gui/remote/src/control.rs` |
| `MAX_IMAGE_BYTES`, `write_bytes`, `Reader::read_bytes`, `DecodeError::{ImageTooLarge, BadBufferFormat}` | `gui/remote/src/{lib.rs, reader.rs}` |
| `BufferFormat` moved to `guiremote::control` and re-exported by the compositor | `gui/remote/src/control.rs`, `gui/compositor/src/buffer.rs` |
| `CompositorRequest::{RegisterImage, UnregisterImage}` and the two byte-count accessors the budget needs | `gui/compositor/src/lib.rs` |
| `ClientLink::image_budget` / `set_image_budget`, `MAX_IMAGE_BYTES_PER_LINK`, the refusal gate | `gui/compositor/src/wire.rs` |
| `Connection::upload_image` / `drop_image` | `gui/remote/src/client.rs` |

**How the two open questions were answered** — see design-decisions.md → 556.
The budget is **per connection, not per window** (a per-window one is bypassed
by opening a second window) and an upload over it is **refused, not evicted**
(eviction "succeeds": a draw naming an id with no pixels behind it renders
nothing, silently and by design, so an evicted thumbnail is a picture that stops
appearing with no error anywhere). The bytes travel **inline**.

**The entry's own premise about mapping was wrong.** It said "the surface path
already maps rather than copies", and offered that as a reason an image might
too. It does not: `SharedBuffer` and `attach_buffer` appear nowhere in
`gui/remote/` — there is no surface-attach request on the wire at all, and
`buffer.rs`'s own module doc says the IPC layer is "currently stubbed" and
`SharedBuffer::import` takes bytes already mapped by someone else. In-process,
"mapping" is a `&[u8]` that was never copied. Over a TCP connection
(design-decisions.md → 460) whose far end need not be on this machine, there is
nothing to map, and a fast path that only exists over loopback would be a fast
path that silently is not there.

**Original entry follows.**

**What it is.** The compositor now has an image store and paints
`RenderCommand::Image` for real (design-decisions.md → 554). But the only way to
get pixels *into* that store is `Compositor::register_image`, a Rust method — so
only code linked into the compositor process can upload one. A GUI program in a
separate process, which is every real GUI program, still cannot.

**Why it did not block the fix.** The compositor is the thing that draws, and it
could not draw an image at all before this; the in-process path is what the
rasteriser and the store are tested through, and it is what the desktop shell
(which *is* in-process today) can use immediately. The wire request is a
separate, smaller piece of work with its own trust-boundary questions.

**Where it lives.** `gui/remote/src/control.rs` → `RequestBody` is the client
request enum; the compositor's handler dispatches on it. An
`UploadImage { window, image_id, width, height, stride, format, bytes }` variant
and a `DropImage { window, image_id }` beside it are what is missing.
`ImageAsset::import` already performs the whole validation the untrusted path
needs, so the handler is a decode and a call.

**What the proper fix must decide, and does not yet.**

1. **A per-window byte budget.** `Compositor::image_bytes()` exists and reports
   the total, but nothing enforces a ceiling. Over a wire request, a client can
   upload until the compositor is out of memory — the per-image
   `MAX_BUFFER_PIXELS` cap bounds one asset, not their number. A budget, and a
   defined behaviour on exceeding it (refuse the upload, versus evict the
   window's least-recently-drawn asset), is required before the request is
   exposed.
2. **Whether the bytes travel inline or by shared mapping.** The surface path
   already maps rather than copies. A thumbnail is small enough that inline is
   tolerable; a full-resolution photo in an image viewer is not.

**How you would notice.** `apps/explorer`'s icon view will draw correctly under
the in-process compositor and draw nothing under a remote one, with no error
either way — the unresolved-id case is silent by design.
