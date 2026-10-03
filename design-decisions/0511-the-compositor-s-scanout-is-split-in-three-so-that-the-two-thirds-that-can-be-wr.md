## 511. The compositor's scanout is split in three, so that the two-thirds that can be wrong is testable on a machine with no graphics card

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The compositor can now put pixels on a real screen. Talking to a
graphics card means sending it several dozen precisely-shaped requests in a
precise order, and almost every way to get that wrong produces a black screen
rather than an error message. None of it can be run on the Windows machine this
tree is developed on, so the obvious way to write it — one file of `unsafe` code
that talks to the card — would have been a few hundred lines that nobody could
execute until SlateOS booted on hardware. It is instead split into a part that
knows the request *shapes*, a part that knows the *conversation*, and a
fifty-line part that actually calls the kernel. The first two run here, against
a fake card, and are covered by 57 tests. Only the third is unrunnable.

### What was decided

`gui/compositor/src/present/drm.rs` and its two submodules implement `Present`
for SlateOS by driving the kernel's Linux-compatible DRM/KMS ioctls. Four
decisions inside it had real alternatives.

**(a) The wire structs are hand-written byte encoders, not `#[repr(C)]`
structs.** `uapi.rs` builds each ioctl payload with a `Writer<N>` that appends
little-endian fields into a fixed-size array, and parses replies with a matching
`Reader<N>`.

*The alternative* is the usual one: declare `#[repr(C)] struct DrmModeCardRes {
… }` and hand the kernel `&mut res as *mut _`. It is shorter and it is what
every DRM binding in existence does.

*Why not.* The kernel encodes `sizeof(struct)` into the ioctl request number —
`(request >> 16) & 0x3FFF` — and refuses anything else. With `#[repr(C)]` the
layout is the compiler's opinion, and a padding or alignment disagreement is
discovered on hardware, as a rejected ioctl, with nothing pointing at the field
that moved. With explicit encoders the layout is *data*, and a test asserts that
each struct's encoded length equals the size its own request constant declares —
so the constant proves the layout, on Windows, at `cargo test` time. That test
found nothing, which is the point: it is now impossible for it to find something
later without saying exactly what.

The cost is real: ~930 lines of mechanical field-shuffling, and a field added in
the wrong place in *both* the writer and the reader would be self-consistent and
wrong. The size assertions do not catch a transposition of two same-width
adjacent fields. That is the residual risk and it is accepted, because the
alternative's residual risk is the same transposition plus every alignment
question.

**(b) The syscall layer is a trait with two methods, and the protocol never
forms a pointer.** `KmsSys::ioctl(request, payload, arrays)` and
`KmsSys::map(offset, len)` are the entire surface between the conversation and
the machine. Everything above them is ordinary safe Rust that runs anywhere.

The hard part is that DRM's enumeration ioctls carry `u64` fields holding
*addresses of userspace arrays* the kernel fills in — so a naive trait would
have to pass raw pointers, and a fake card cannot honour a pointer it did not
create. The seam is `OutArray { ptr_at, buf }`: a buffer named by the *byte
offset within the payload* of the pointer field that should point at it. The
real implementation writes an address there; the fake matches on the offset and
copies into the buffer directly. Only ~50 lines — the `asm!("syscall")` block,
`open`, `close`, `mmap`, `munmap` — are `#[cfg(target_os = "linux")]`.

*The alternative* was to gate the whole module on the target and test nothing.
*Why not* is the arithmetic: of the ten defects later reintroduced to prove the
tests, nine live above the seam. Gating the module would have left nine
unrunnable bugs and saved the indirection of one trait.

Those ~50 lines are still not *executed* anywhere, but they are compiled and
clippy-clean under `--target x86_64-unknown-linux-gnu`, which is an installed
rustup target — so they are checked source rather than hopeful source.

**(c) The front buffer index advances only on a flip the kernel accepted, and
`EBUSY`/`EAGAIN`/`EINTR` drop the frame instead of closing the display.**

A page flip can be refused because the previous one has not retired. Treating
that as an error would take the desktop down whenever a monitor was slow — under
load, which is exactly when a desktop must not vanish. Treating it as success
would be worse: the two buffers would get permanently out of step and every
subsequent frame would be composed into the buffer currently being scanned out,
producing visible tearing forever after one moment of contention.

So a refused flip is a dropped frame and the *same* back buffer is used again.
Any other errno — `ENODEV`, a card unbound — closes the display, and
`Present::is_open` returns false, which stops `Server::run_with` in the same way
a closed Win32 window does.

*The alternative* is to retry the flip in a loop until it lands. *Why not:* it
converts a contended display into a busy-wait inside the compositor's frame
loop, which is a worse failure than the dropped frame it avoids, and the next
composed frame is fresher than the one being retried anyway.

**(d) The compositor is not constructed until the display's mode is known.**
`main()` previously built the `Compositor` and then chose where to put its
frames. It now binds the socket, then calls a per-platform `run()` which finds a
display *first* and builds the compositor at whatever size that display turns
out to be.

This is forced by the hardware: the kernel implements no `SETCRTC`, so the mode
the firmware lit at boot is the mode we get (`known-issues.md` →
`TD-COMPOSITOR-CANNOT-CHANGE-MODE`). A compositor built at a size the display is
not running would compose frames that cannot be shown.

The visible consequence is an asymmetry in `--size`: it sets the window size
under the Win32 harness, and on SlateOS it is *reported as ignored*, naming the
mode the display is actually running. Printing that rather than silently
overriding it is the decision — a flag that is quietly disobeyed is worse than
one that explains itself, and the explanation is the only place a user would
learn that the mode is fixed.

### How this is known to work

The fake card is deliberately hostile rather than accommodating. On every call
it re-checks the payload's length against the size the request number declares,
rejects `ADDFB2` unless the six load-bearing zero fields are zero, rejects a
`PAGE_FLIP` naming an unknown CRTC or framebuffer, and panics outright on any
ioctl the SlateOS kernel does not implement. It models **two** CRTCs and lists a
*disconnected* connector first, because both of the two subtlest bugs in this
area — reading `possible_crtcs` as a mask over CRTC ids rather than over array
indices, and taking `connectors[0]` — are invisible against a one-CRTC card with
one monitor.

A permissive fake would let the protocol drift and still report green, which
would make the 34 tests worse than none: they would be a reason to believe
something untrue.

Each of the 34 was then proved to be a regression test rather than a
description, by reintroducing the defect it names — via the guarded reversible
patcher, one at a time — and confirming a deterministic failure that names the
test back. All ten reintroductions failed loudly; the count-clamp one aborted
the process with a 292 GB allocation failure, which is precisely the denial of
service the clamp exists to prevent.
