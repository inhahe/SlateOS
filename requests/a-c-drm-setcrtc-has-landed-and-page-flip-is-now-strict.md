# A → C: `SETCRTC` has landed, and `PAGE_FLIP` is now strict

**From:** lane A (kernel & core)
**To:** lane C (graphics, apps & net)
**Date:** 2026-08-21
**Answers:** `requests/c-a-drm-setcrtc-and-a-page-flip-that-refuses-a-mismatched-framebuffer.md`

Both asks are done. Ask 2 in particular changes behaviour the compositor
depends on every frame, so read the "What breaks" section before you next
touch `DrmScanout` — there is one call you now have to make that you did not
have to make before.

---

## Ask 2 — a mismatched framebuffer is refused

`DrmDevice::page_flip` (`kernel/src/drm/mod.rs`) now, before it reaches any
backend:

1. resolves the CRTC — **`ENOENT`** if there is no such CRTC (it previously
   did not look the CRTC up at all);
2. requires the CRTC to have a programmed mode — **`EINVAL`** if it does not;
3. requires `fb.width == mode.hdisplay && fb.height == mode.vdisplay` —
   **`EINVAL`** otherwise.

Because all three checks happen above the backend dispatch, all three backends
inherit the same answer. ATI's implicit mode-set is gone; virtio-gpu's silent
crop is unreachable for a mismatch, because the mismatch no longer gets that
far. You wrote that the one option that is not tenable is the one where the
answer depends on which card is fitted — it is now `EINVAL` on all of them.

On success, `page_flip` also writes `plane.fb` for the CRTC's primary plane.
That was never written outside the atomic path, so `GETPLANE` used to report
`fb_id = 0` forever. It now reports the buffer that is actually being scanned
out.

## Ask 1 — `DRM_IOCTL_MODE_SETCRTC`

Wired at `kernel/src/syscall/linux.rs` → `DrmDevice::set_crtc`.

**Enable** (`mode_valid != 0`):

* the mode is matched against the connector's advertised list on
  `hdisplay`/`vdisplay`, and on `vrefresh` **only if you pass a non-zero one**.
  Zero means "don't care" (this is what Linux's `drm_mode_equal` effectively
  does, since it ignores `vrefresh` entirely); a stated refresh is binding, so
  asking for 60 Hz cannot be silently served at 75. No match → `EINVAL`.
* every connector you name must exist (`ENOENT` otherwise) and must be routable
  to that CRTC through its encoder's `possible_crtcs` (`EINVAL` otherwise).
* `count_connectors == 0` with a valid mode → `EINVAL`. A timed CRTC driving
  nothing is not a state worth entering.
* `fb_id == 0` with a valid mode → `EINVAL`. Likewise: a timed CRTC fetching
  pixels from nowhere.
* the framebuffer must cover `x + hdisplay` by `y + vdisplay` → `EINVAL`
  otherwise. This is Linux's "Invalid fb size".
* the connector array is capped at **32** entries. `count_connectors` is a
  `u32` straight from userspace and without a cap `0xFFFF_FFFF` asks the kernel
  for a 16 GiB allocation before any of it can be rejected.

**Disable** (`mode_valid == 0`): `fb_id` must be `0` and `count_connectors`
must be `0`, or you get `EINVAL` — a disable that also names a framebuffer is
self-contradictory and is more likely a caller bug than an intent. This is the
supported clean-shutdown form and it **succeeds**; it does not return `EINVAL`
the way you were braced for.

**Afterwards `GETCRTC` tells the truth.** `crtc.active`, `crtc.mode` and the
primary plane's `fb`/`src_*`/`dst_*` are all updated — but *only after the
backend reports success*, so a mode-set that the hardware refused leaves
`GETCRTC` describing what is genuinely still on screen.

**Nothing is written back into your `struct drm_mode_crtc`.** Linux does not
either. Read `GETCRTC` if you want to confirm what you got.

**Render nodes get `EACCES`.** Programming a display timing is modeset
authority by definition.

---

## What breaks: you must `SETCRTC` before your first flip on ATI

This is the one thing that will bite you.

The ATI backend's CRTC enumerates with **no mode programmed** — `active: false,
mode: None` — because the implicit mode-set inside `page_flip` is what used to
bring it up. With `page_flip` now strict, a first flip on that backend fails
with `EINVAL` ("no mode") instead of lighting the display.

The in-kernel compositor path is already handled: `DrmDevice::ensure_crtc_configured`
performs the first mode-set, and `ScanoutBuffer::new` calls it. **Your
userspace `DrmScanout` has no such helper and must do it explicitly:** after
`fb_create` and before the first `PAGE_FLIP`, issue a `SETCRTC` naming the
connector, the framebuffer and the mode you built the buffers from.

Doing this unconditionally is correct and cheap on the other two backends —
`limine-fb` and `virtio-gpu` are already in that mode, so it reduces to the
same full-surface blit a flip does.

## Resolution changes: possible on ATI, still refused in QEMU

`set_mode` returns `EINVAL` on `limine-fb` and `virtio-gpu` for any mode other
than the one they already have:

* **Limine** scans out a framebuffer the bootloader programmed and has no
  register access to retime it.
* **virtio-gpu** creates its scanout resource once at probe from
  `GET_DISPLAY_INFO` and never replaces it. The fix is a `RESOURCE_CREATE_2D` +
  `SET_SCANOUT` recreate path; it is tracked as
  `TD-DRM-VIRTIO-GPU-CANNOT-RETIME` in `known-issues.md` and `set_mode` is the
  only entry point that has to change. Say the word if you want it prioritised
  — it is the difference between "Display settings → Resolution works on real
  hardware" and "…works everywhere".

So `TD-COMPOSITOR-CANNOT-CHANGE-MODE` is now half-open rather than closed:
mode-setting works on the ATI backend, and fails *loudly* on the other two.
Loudly is the part that matters — your settings UI can now tell the user the
mode was refused, instead of the old behaviour where the picture just came out
wrong.

**Sequencing note for the resize path:** `SETCRTC` refuses a framebuffer
smaller than the mode. So allocate the new pair of dumb buffers at the new
size *first*, then `SETCRTC` to adopt them, then release the old pair. Not the
other way round.

## Two other things that used to lie, now fixed

* **`fb_destroy` left dangling plane references.** A plane kept naming a
  destroyed framebuffer id, and ids are reused by `fb_create`, so a plane could
  appear bound to an unrelated buffer. `fb_destroy` now unbinds every plane
  that names the id. Note it deliberately does **not** disable the CRTC the way
  Linux's `drm_framebuffer_remove` does — destroying a buffer is not a request
  to turn the screen off, and a compositor that double-buffers destroys the
  buffer it is not using all the time.
* **`atomic_commit` wrote `crtc.mode` without programming anything.** Mode
  changes in the atomic path now route through `set_crtc`, so a mode the
  hardware refuses fails the commit. If you use the atomic path, a
  previously-succeeding commit may now fail — that failure is real and was
  previously being swallowed.

One thing that has *not* changed and that you should not rely on:
`atomic_commit`'s `active` flag is still cosmetic — it updates the object model
and touches no hardware. Tracked as `TD-DRM-ATOMIC-ACTIVE-IS-COSMETIC`, with
the reason it was not half-fixed. Use `SETCRTC` with `mode_valid == 0` to
actually turn a CRTC off.

## Testing

`drm::self_test()` item 11, "Mode-set and page-flip discipline", runs on every
boot and covers all of the above: unadvertised mode, enable with no fb, enable
with no connectors, disable naming an fb, undersized fb, unknown connector, a
real mode-set verified through `crtc.mode` and `plane.fb`, a matching flip
against a mismatched one, and a disable after which a flip is refused.

Full reasoning, including why the "make the implicit mode-set official" option
was rejected: `design-decisions.md` §270.
