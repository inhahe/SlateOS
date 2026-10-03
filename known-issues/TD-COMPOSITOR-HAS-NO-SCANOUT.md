## TD-COMPOSITOR-HAS-NO-SCANOUT (lane C, 2026-08-17) - **fixed 2026-08-21**

> **Read the Status note at the end of this entry first.** The description
> below is the state *before* the `Present` trait and the Win32 host window
> landed, and its flat claims — "there is no `present()`", "no window on the
> host", "the screen was never involved" — are no longer true of the hosted
> development build. They remain true of SlateOS itself, which is what this
> entry is actually about, so the description is kept rather than rewritten:
> it is the statement of the problem that is still open.

**What.** The compositor composes a complete frame and then drops it.
`Compositor::compose_frame()` blends every visible window, the cursor and the
desktop furniture into a back buffer, flips it, and `front_buffer()` hands out
a `&[u32]` of finished ARGB pixels — which `server::run` looks at only to
count. Nothing in this tree puts those pixels in front of a human. There is no
`present()`, no framebuffer device, no DRM plane, no window on the host.

So the whole pipeline is real up to the last step: an app can connect, be given
a window, submit a `RenderTree`, have it rasterised and composited with correct
z-order, damage and opacity — and the result exists only as memory that is
overwritten by the next frame.

**Why it is like this.** Scanout is the one part that cannot be written
portably. It needs either (a) SlateOS's display driver — a framebuffer the
kernel maps for us, which is the target and does not run yet in a form the
compositor can reach from the hosted build — or (b) a host window, which means
a platform dependency (Win32 `BitBlt`, X11, Wayland, or a crate like
`softbuffer`/`minifb`) that lane C cannot add to the workspace-root
`Cargo.toml`. `gui/compositor/src/display.rs` models displays, modes and
refresh rates faithfully and has no code that touches hardware, which is the
honest state: the *policy* of scanout is written, the *mechanism* is not.

**How to see the gap.** Run the compositor and connect a client. Everything
reports success — `frame_stats().last_frame_time_us` is non-zero, the client
gets its window id, `window_count()` is 1 — and the screen is unchanged,
because the screen was never involved.

**The proper fix.**

1. **The real one: a SlateOS framebuffer.** The compositor asks the display
   driver for a mapping of the scanout buffer and, after each `compose_frame`,
   copies (or page-flips, which is why the back/front buffer pair already
   exists) into it. `TD-NO-WRITE-COMBINING` is a prerequisite for this being
   fast rather than merely correct — an uncached framebuffer makes the copy
   roughly an order of magnitude more expensive than it needs to be.
2. **For the hosted build: a host window behind a trait.** `Present` with one
   method — take `&[u32]`, width, height — implemented once per platform and
   once as a no-op. This is the shape that keeps `server::run` unchanged
   between the two, and it is small; what it needs is the workspace dependency,
   which is a cross-lane request rather than a design problem.
3. **A useful intermediate that needs nothing:** dump `front_buffer()` to a PPM
   or PNG on a key chord or a signal. It does not make the desktop visible, but
   it makes "did the compositor draw what I think it drew" answerable without a
   display at all.

**Severity.** High as a *blocker*, low as a *defect*, in the same sense as
`TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR`: nothing that exists is wrong, and the
missing piece is additive. It is the last link in the chain from an app's
`RenderTree` to a photon, and it is the only one still missing.

**Status 2026-08-17: (2) is done; (1), the actual target, is not.**
`gui/compositor/src/present.rs` defines the `Present` trait — `show(pixels,
width, height)`, `input()`, `is_open()` — with `Headless`, a `Recording` test
double, and, under `#[cfg(windows)]`, `present::host::Window`: a raw Win32
window that blits with `StretchDIBits` and turns `WM_*` messages into
`InputEvent`s. `Server::run_with` drives any implementor; `server.run()` is
that same loop over `Headless`. The compositor binary opens one by default on
Windows, and `--headless` opts out. The paragraph above about the workspace
dependency turned out to be a non-problem: the whole thing is `extern "system"`
declarations against `user32`/`gdi32`, so it adds no entry to any
`Cargo.toml` — see `design-decisions.md` §461.

**What that closed, precisely, and what it did not.** It closed *seeing*: the
rendering pipeline — text, alpha blending, window decorations, shadows, the
cursor — had never been looked at by a human eye, and the editor's title bar,
tab strip, gutter and caret have now been observed drawn by the compositor over
a real TCP socket. It also closed the input half, which was the same gap wearing
a different hat: a hosted build has no keyboard driver, so `handle_input` was
reachable only from tests, and `route_input` had nothing to route. It did
**not** close scanout. A Win32 window is a *development harness* — it runs on
the wrong OS, against a window manager we do not own. Item (1) is untouched and
remains the real fix; when it lands it will be another `impl Present` with
nothing else in this crate changing, which is the argument for the trait having
been worth defining before the driver exists. This entry therefore stays open,
and the sentence in `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR` calling this the last
link to a photon stays true of SlateOS, not of the harness.

**Status 2026-08-21: (1) is done. This entry is CLOSED.**

**In short:** the compositor now puts its frames on a real SlateOS screen. It
opens the graphics card, asks it which monitor is plugged in and what resolution
that monitor is already running, gets two chunks of video memory it can write
pixels into, and tells the card to display them alternately. The chain from an
application's window to a photon is complete.

`gui/compositor/src/present/drm.rs` implements `Present` as `DrmScanout`, the
third implementor beside `Headless` and the Win32 harness, and — as this entry
predicted — **nothing else in the crate changed**: `Server::run_with` drives it
unmodified. The binary's `run()` gained a `#[cfg(target_os = "linux")]` arm
(`x86_64-slateos` reports `target_os = "linux"`) that opens `/dev/dri/card0` and
falls back to headless with a printed reason if there is no display.

The pipeline it drives is Linux's, because our kernel implements Linux's DRM
ioctls: `GETRESOURCES` → `GETCONNECTOR` → `GETENCODER` → `CREATE_DUMB` →
`MAP_DUMB` → `mmap` → `ADDFB2` → `PAGE_FLIP`. Two dumb buffers are allocated at
the connected display's preferred mode, mapped, and page-flipped alternately;
the front index advances only on a flip the kernel accepted, so a refused flip
drops a frame instead of putting the pair permanently out of step.

**The three things that were not obvious**, recorded because each is a defect
that would have shipped:

* **`SETCRTC` is absent from this kernel and is not needed.** The first reading
  of `kernel/src/drm/` said scanout was blocked on lane A adding it. Reading
  `DrmDevice::page_flip` and all three backends showed otherwise: the flip
  validates the CRTC id, the fb id and its GEM object, all three backends ignore
  `crtc_id`, and the ATI backend self-modesets inside the flip. Driving the mode
  the firmware already lit needs no `SETCRTC`. No cross-lane request was filed,
  because the premise was false. What it *does* cost is the ability to change
  resolution — see `TD-COMPOSITOR-CANNOT-CHANGE-MODE` below.
* **Pitch is not width × 4.** Dumb buffers are 64-byte aligned, so a 1366-wide
  screen has a 5504-byte pitch, not 5464. Every row copy and the `ADDFB2`
  registration use the driver's returned pitch. Getting this wrong skews every
  row after the first, and is invisible at any width divisible by 16.
* **`possible_crtcs` is a bitmask over the CRTC array *index*, not over CRTC
  ids.** This hides completely on a single-CRTC machine, where index 0 and the
  only id both make bit 0 look right — which is why the test fake models two.

**How it is tested without hardware.** The protocol half is not `cfg`-gated and
runs on the development machine against a strict in-memory fake card; only the
~50 lines of raw `syscall` mechanism are target-only, and those are compiled and
linted via `--target x86_64-unknown-linux-gnu`. 57 tests: 23 pinning the wire
structs against the `_IOC`-encoded sizes in the request numbers, 34 driving the
scanout end to end. The fake is deliberately hostile — it re-checks every
payload's length against the size its request number declares, rejects `ADDFB2`
unless the load-bearing zero fields are zero, and panics on any ioctl the kernel
does not implement — because a permissive fake lets the protocol drift and still
reports green. All 34 were then proved to be regression tests by reintroducing
the ten defects they name, one at a time, and confirming a deterministic failure
that names the test back. Design rationale: `design-decisions.md` §511.

**What this closed and what it did not.** It closed *scanout*. It did not close
input: `DrmScanout` inherits the default `Present::input`, which returns nothing,
because the kernel exposes no evdev-style device — see
`TD-COMPOSITOR-HAS-NO-LOCAL-INPUT`. A SlateOS desktop now draws and cannot be
typed at, which is the exact mirror of where the Win32 harness started.
