## 556. A program in another process can hand the compositor a picture, and is refused rather than throttled when it hands over too many

**Date:** 2026-08-26
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The compositor could draw pictures, but the only way to give it
one was to be part of the compositor program itself — so no ordinary
application could show an icon, a thumbnail or a photo. It can now: a program
sends the pixels once, under a number of its own choosing, and afterwards its
drawing instructions just name that number. Because the pixels stay in the
compositor's memory until the program says otherwise, a program could otherwise
keep sending pictures until the machine ran out of memory, so each connection
gets a memory allowance and an upload that would exceed it is turned down. Four
choices had a real case on both sides; this records them.

### 1. Upload is a separate request from drawing

A window's picture — its *render tree* — is re-sent whenever anything about the
window changes, which for an animation or a text cursor is every frame. A
protocol that carried the pixels alongside the drawing instructions would
therefore push a megabyte down the socket sixty times a second to keep one
still photograph on screen. So the pixels are uploaded once and given an id,
and each frame names the id.

The cost is that the compositor now holds state on the client's behalf between
frames, which is exactly what creates the budget problem below. The alternative
— stateless frames — has no budget problem at all, and would have made a
picture unaffordable to display. Not a close call, but worth writing down,
because the memory limit that follows exists *only* because of this choice.

### 2. The budget is per connection, not per window

*What the alternative would look like:* each window gets, say, 64 MiB of image
memory, and a program with three windows gets 192 MiB.

That is trivially defeated: a program that wants more memory opens another
window. Windows are cheap and a client may have as many as it likes, so a
per-window limit is a limit on nothing. A connection is the unit that has an
identity the compositor can hold responsible — it is already the unit that owns
windows (design-decisions.md → 458), and it is the thing that goes away, taking
its memory with it, when the program exits.

The cost is real: one program with twenty windows gets the same allowance as
one with a single window, so a document editor with many open files may hit the
ceiling sooner than a photo viewer with one. That is the correct direction to
be wrong in — the alternative is a ceiling that no program can hit.

The limit is a **field on the link** (`ClientLink::set_image_budget`), not a
bare constant, because the right number depends on the machine and on what the
connection is: a trusted desktop shell running the wallpaper is not a web page's
worth of thumbnails. It is the *server's* to set and not the client's — nothing
on the wire can change it, which is what keeps it a limit rather than a
suggestion. The default is 256 MiB.

### 3. An upload over the budget is refused; nothing is evicted

*What the alternative would look like:* the compositor drops the connection's
least-recently-drawn image to make room, and the upload succeeds.

Eviction sounds kinder and is worse, for one specific reason: **it succeeds.** A
drawing instruction naming an id with no pixels behind it renders nothing —
silently, by design, because that is what makes a window that drops an image
mid-frame not a crash. So an evicted thumbnail is a picture that stops appearing
with no error anywhere: not at the upload (it worked), not at the draw (drawing
nothing is legal), and not in any log the application can see. The application
has no way to learn that it needs to re-upload, and no event to learn it from.

A refusal, by contrast, arrives at the call that caused it, names the numbers,
and leaves everything exactly as it was. The client can then decide — downscale,
drop its own cache, or show a placeholder — which is a decision it can actually
make and the compositor cannot.

This is also what `design.txt` already says about memory generally: *committed
memory by default, no silent overcommit.* An eviction policy is overcommit with
a cleanup crew. And it would put least-recently-used bookkeeping on the draw
path, which runs per image per frame.

The cost: a client that legitimately needs more than its budget fails rather
than degrading. Accepted, because "fails visibly" is the whole point.

### 4. The bytes travel inline, not by shared mapping

The `known-issues.md` entry that asked for this feature suggested a shared
mapping instead, on the grounds that "the surface path already maps rather than
copies". **That premise is false of the wire.** `SharedBuffer` and
`attach_buffer` appear nowhere in `gui/remote/`: there is no surface-attach
request on the protocol at all, and the compositor's own module doc says the IPC
layer is stubbed and `SharedBuffer::import` takes bytes someone else already
mapped. In-process, "mapping" is a `&[u8]` that was never copied.

The transport is a TCP connection (design-decisions.md → 460) whose far end need
not be on this machine. There is nothing to map across it. A mapping fast path
would work over loopback and silently not exist otherwise — which is worse than
no fast path, because the performance of the display protocol would then depend
on where the client happens to be running, invisibly.

So the bytes go inline, bounded by `MAX_IMAGE_BYTES` (7680×4320×4 ≈ 126 MiB —
the compositor's largest framebuffer). If a local fast path is wanted later it
belongs at the transport, not in this request: the request would be unchanged
and only the bytes' journey would differ.

### 5. `BufferFormat` moved to the wire crate

The compositor declared its own `BufferFormat` enum. That was tenable while
pixels could only be handed over in-process — the enum and its only callers were
compiled together. The moment an upload can arrive from another process, the
pixel format is part of the *wire*, and two enums would mean two orderings for
the byte-per-variant mapping with nothing to keep them agreeing but a reviewer
noticing.

This is the same rule that left `guiremote::control::CursorShape` the tree's
only cursor enum (design-decisions.md → f2): **the wire owns shared
vocabulary.** The compositor re-exports it, so all 61 existing references
compiled unchanged — the move cost nothing and removed a whole class of future
divergence.

### Where it lives

`gui/remote/src/control.rs` (`RequestBody::UploadImage`/`DropImage`,
`BufferFormat`), `gui/remote/src/lib.rs` (`MAX_IMAGE_BYTES`, `write_bytes`),
`gui/remote/src/reader.rs` (`read_bytes`), `gui/remote/src/client.rs`
(`Connection::upload_image`/`drop_image`), `gui/compositor/src/wire.rs`
(`MAX_IMAGE_BYTES_PER_LINK`, `ClientLink::image_budget`,
`Compositor::image_budget_refusal`), `gui/compositor/src/lib.rs`
(`CompositorRequest::RegisterImage`/`UnregisterImage`, `window_image_bytes`,
`image_size_bytes`).

**Numbering note.** Lane C's allocated band ended at 499; this continues the
documented overflow past it.
