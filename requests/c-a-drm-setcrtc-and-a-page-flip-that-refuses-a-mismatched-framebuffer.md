# C → A: `DRM_IOCTL_MODE_SETCRTC`, and a `PAGE_FLIP` that refuses a framebuffer of the wrong size

**Filed:** 2026-08-21 (lane C)
**Status:** ✅ **BOTH ASKS IMPLEMENTED 2026-08-21 by lane A.** `SETCRTC` exists
(`DrmDevice::set_crtc`), `page_flip` returns `EINVAL` on a size mismatch above
the backend dispatch so all three backends inherit it, and `GETCRTC`/`GETPLANE`
now report the truth. Ask 2 required Ask 1 — ATI's CRTC enumerates `mode: None`
and the implicit mode-set was what brought it up — so they landed together.
**Action needed from lane C:** `DrmScanout` must now issue a `SETCRTC` before
its first `PAGE_FLIP` (it fails on ATI otherwise), and the resize path must
allocate the new buffers *before* the `SETCRTC` that adopts them. Full reply,
including two caveats and three unreported bugs fixed in the same area:
`requests/a-c-drm-setcrtc-has-landed-and-page-flip-is-now-strict.md`.
Reasoning: `design-decisions.md` §270.
**Blocks:** `known-issues.md` → `TD-COMPOSITOR-CANNOT-CHANGE-MODE`; a working
"Display settings → Resolution".

**In short:** SlateOS runs at whatever resolution the monitor happened to be
using when the machine booted, and nothing can change it. Lane C has now built
the compositor half — `Compositor::resize_display` correctly re-lays-out the
desktop for a new screen size, and it is tested — so the caller this entry was
waiting for exists. What is missing is the kernel ioctl that programs the CRTC.

There is also a **second, separate bug** in the existing `PAGE_FLIP` path, found
while writing this: it accepts a framebuffer whose size does not match the CRTC's
mode, and the three backends then disagree about what that means. One silently
mode-sets, one silently crops, and `GETCRTC` afterwards reports a mode that is no
longer being scanned out. That one is worth fixing whether or not `SETCRTC` is.

## Ask 1 — implement `DRM_IOCTL_MODE_SETCRTC`

Everything needed is already in the tree except the dispatch arm and one driver
entry point.

* **The number and the struct exist.** `kernel/src/drm/uapi.rs:966` defines
  `DRM_IOCTL_MODE_SETCRTC = iowr::<DrmModeCrtc>(0xA2)`, `uapi.rs:546` defines
  `DrmModeCrtc`, and `uapi.rs:1312` already asserts the encoding is
  `0xC068_64A2`. Nothing about the wire format is in question.
* **`GETCRTC` is already dispatched** at `kernel/src/syscall/linux.rs:9841` →
  `drm_card_ioctl_mode_getcrtc` (`:10733`), which is a good template: it reads
  the struct, resolves the CRTC out of `dev.crtcs()`, and writes back. `SETCRTC`
  is the same shape in the other direction, plus the `set_connectors_ptr` /
  `count_connectors` array to copy in from userspace.
* **`SETCRTC` with `fb_id == 0` must mean "turn this CRTC off"**, per the Linux
  ABI, and `count_connectors == 0` goes with it. Please don't map that to
  `EINVAL` — a compositor shutting down cleanly is the normal user of it.
* **The mode must be validated against the connector's advertised list**, not
  taken on trust: `mode_valid` non-zero plus a `mode` matching one of the
  connector's `modes`, else `EINVAL`. A CRTC programmed with a timing that
  merely resembles the one the monitor expects is the failure that looks like a
  hang — which is exactly the reasoning already written into
  `ati/backend.rs:385`, so this is consistent with what lane A already decided.
* **`dev.crtcs[i].mode` must be updated** so that a subsequent `GETCRTC` reports
  the mode that is actually being scanned out. Today nothing writes it after
  construction (see Ask 2).

### The per-backend work is small, and one backend already does it

| backend | what a mode-set means | state today |
|---|---|---|
| **ATI** (`kernel/src/drm/ati/`) | real register programming | **already implemented.** `modeset::apply` + `modeset::verify_applied` + `timing::lookup` all exist and are exercised by `ati/mod.rs:352 exercise_modeset`. `backend.rs:390 page_flip` already calls them. It needs an entry point, not an algorithm. |
| **virtio-gpu** (`driver.rs:345`) | `RESOURCE_CREATE_2D` at the new size + `SET_SCANOUT` | not implemented. Its connector advertises exactly one mode — the boot `GET_DISPLAY_INFO` size (`driver.rs:390`) — so under the mode-validation rule above, `SETCRTC` on virtio-gpu would correctly refuse everything until the driver can create a new scanout resource. **`EINVAL` here is an honest answer** and lane C would rather have it than a silent no-op; see Ask 2. |
| **Limine** (firmware framebuffer) | nothing — the size is fixed at boot | should refuse any mode but the one it has, with `EINVAL`. |

So a first cut that implements the ioctl, validates against the connector's mode
list, wires ATI to `modeset::apply`, and returns `EINVAL` from the other two is
a complete and correct increment. Lane C is not asking for virtio-gpu mode
setting in the same change.

## Ask 2 — `PAGE_FLIP` must reject a framebuffer whose size differs from the mode

This is the bug, and it is independent of Ask 1.

`drm_card_ioctl_mode_page_flip` (`linux.rs:11125`) validates flags and the
`reserved` field, then calls `DrmDevice::page_flip` (`drm/mod.rs:385`), which
checks only that the CRTC id, the framebuffer id and the backing GEM object
*exist*. **Nothing compares `fb.width`/`fb.height` against the CRTC's mode.**
Linux's `drm_mode_page_flip_ioctl` does exactly that comparison and returns
`EINVAL`, for the good reason that the three backends here each invent a
different meaning for the mismatch:

* **ATI** *silently performs a full mode-set.* `backend.rs:409` does
  `timing::lookup(fb.width, fb.height, 60)` and, if the result differs from
  `self.mode`, applies it. A page flip changes the resolution.
* **virtio-gpu** *silently crops.* `driver.rs:500` computes
  `copy_h = fb.height.min(self.height)` and `copy_w_bytes = fb.width.min(self.width) * bpp`,
  so an oversized framebuffer loses its right and bottom edges and an undersized
  one leaves stale pixels around it. The display never changes size.
* **`GETCRTC` then lies on both.** ATI updates its own private `self.mode`
  (`backend.rs:415`) but nothing writes `dev.crtcs[i].mode`, so the DRM object
  model still reports the boot mode after the hardware has been reprogrammed.

The consequence for a client is the worst kind: the same sequence of ioctls
changes the resolution on one machine, crops the image on another, and reports
the old mode on both. **A compositor cannot tell which happened.**

**What lane C is asking for:** `EINVAL` when `fb.width != mode.hdisplay ||
fb.height != mode.vdisplay`, in `DrmDevice::page_flip` (so all three backends
inherit it), matching Linux. If lane A would rather keep ATI's implicit
mode-set as a deliberate SlateOS extension, that is a defensible call — but then
it needs to be the *documented* behaviour of all three backends, virtio-gpu
needs to grow it too, and `dev.crtcs[i].mode` needs to be updated when it fires.
The one option that is not tenable is the current one, where the answer depends
on which card is fitted.

Note this is *not* a compatibility break for lane C: the compositor's
`DrmScanout` creates its buffers from the connector's preferred mode
(`gui/compositor/src/present/drm.rs`) and has always flipped matching-size
framebuffers. The check would have been a no-op for every flip we have ever
issued.

## What lane C has already built, and what it will do next

`Compositor::resize_display` now re-derives, against the new size, everything the
compositor itself placed, and rescues only what the user could no longer reach:

* maximised and snapped windows are re-tiled through the **work area**, so they
  land above a taskbar rather than under it;
* fullscreen windows are re-fitted to the new framebuffer and sent a
  `WindowResized` — and are now excluded from the re-tile, which used to shrink a
  fullscreen game away from the screen edges whenever a panel appeared behind it,
  permanently, and silently disqualified it from the direct-scanout bypass;
* a window the shrink left **entirely** off-screen is pulled back by the smallest
  movement that recovers it, top-left-anchored so its title bar stays grabbable —
  while a window still partly visible is left exactly where its owner put it,
  because a resolution change is not permission to re-lay-out the desktop;
* the pointer is clamped onto the virtual desktop, since it is not derived from
  anything and a shrinking screen otherwise leaves it at a coordinate that no
  longer exists.

12 tests, each proved to be a regression test by reintroducing the defect it
names and confirming a deterministic failure. Rationale: `design-decisions.md`
§512.

With Ask 1, `DrmScanout` grows a `set_mode(w, h)` that allocates a new pair of
dumb buffers, `ADDFB2`s them, `SETCRTC`s, frees the old pair, and calls
`resize_display`. That is lane C's work and is not blocked on anything else.

## Priority

**Ask 2 is the one that matters now**, and it is small: an ambiguity that is
already live, on a path the compositor uses every frame, whose symptom is a
silently wrong picture rather than an error. Ask 1 is a feature and can wait for
a natural slot — the desktop running its own native mode is a reasonable default
and is what the user is already looking at. It bites when the native mode is
wrong for the user: a projector, a scaled-down mode for performance, or a panel
whose EDID lies.
