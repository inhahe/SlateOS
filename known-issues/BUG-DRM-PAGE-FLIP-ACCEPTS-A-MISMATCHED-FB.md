## BUG-DRM-PAGE-FLIP-ACCEPTS-A-MISMATCHED-FB (found by lane C 2026-08-21; lives in lane A's tree)

**In short:** the kernel lets a program hand the graphics card a picture that is
the wrong size for the screen, and then each of the three supported cards does
something *different* with it — one quietly changes the resolution, one quietly
crops the picture, and both then report the old resolution when asked. The same
sequence of calls therefore produces a different result on different hardware,
and the program that made them cannot tell which happened.

**Where.** `DrmDevice::page_flip` (`kernel/src/drm/mod.rs:385`), reached from
`drm_card_ioctl_mode_page_flip` (`kernel/src/syscall/linux.rs:11125`). It checks
that the CRTC id, the framebuffer id and the backing GEM object *exist*, and
nothing else. Linux's `drm_mode_page_flip_ioctl` compares the framebuffer's
dimensions against the CRTC's current mode and returns `EINVAL`; we do not.

**The three divergent behaviours:**

* **ATI** silently performs a full mode-set. `ati/backend.rs:409` does
  `timing::lookup(fb.width, fb.height, 60)` and applies the result if it differs
  from `self.mode`. A page flip changes the resolution.
* **virtio-gpu** silently crops. `drm/driver.rs:500` computes
  `copy_h = fb.height.min(self.height)` and
  `copy_w_bytes = fb.width.min(self.width) * bpp`, so an oversized framebuffer
  loses its right and bottom edges and an undersized one leaves stale pixels
  around itself. The display never changes size.
* **`GETCRTC` lies afterwards on both.** ATI updates its own private
  `self.mode` (`backend.rs:415`), but nothing writes `dev.crtcs[i].mode`, so the
  DRM object model keeps reporting the boot mode after the hardware has been
  reprogrammed.

**Severity.** Medium, and latent. No current client triggers it — the
compositor's `DrmScanout` builds its buffers from the connector's preferred mode
and has only ever flipped matching-size framebuffers, so the check would have
been a no-op for every flip we have issued. It becomes live the moment anything
tries to change resolution, which is precisely what
`TD-COMPOSITOR-CANNOT-CHANGE-MODE` is about; and it is the sort of defect that,
left alone, gets *depended on* — a client that discovers ATI's implicit mode-set
will use it, and then be silently cropped in QEMU.

**Proper fix.** `EINVAL` in `DrmDevice::page_flip` when the framebuffer's size
differs from the CRTC's mode, so all three backends inherit one answer. Keeping
ATI's implicit mode-set as a deliberate SlateOS extension is also defensible —
but then it must be the documented behaviour of *all three* backends, virtio-gpu
must grow it, and `dev.crtcs[i].mode` must be updated when it fires. The one
option that is not tenable is the present one, where the answer depends on which
card is fitted.

**Not lane C's to fix** — `kernel/**` is lane A's. Filed as Ask 2 of
`requests/c-a-drm-setcrtc-and-a-page-flip-that-refuses-a-mismatched-framebuffer.md`.

**FIXED 2026-08-21 (lane A).** `DrmDevice::page_flip` now resolves the CRTC,
requires it to have a programmed mode, and returns `InvalidArgument` when
`fb.width != mode.hdisplay || fb.height != mode.vdisplay` — before any backend
is reached, so all three inherit the one answer. ATI's implicit mode-set was
removed and replaced by a real `set_mode`; virtio-gpu's crop is now unreachable
for a mismatch because the mismatch never gets that far. Lane C's "the answer
depends on which card is fitted" is closed: the answer is `EINVAL` on all of
them. The option debate is recorded in `design-decisions.md` §270.

Three further object-model lies in the same area were found and fixed while
here, none of which lane C had reported:

* **`plane.fb` was never written by `page_flip`** — only by the atomic path — so
  `GETPLANE` reported `fb_id = 0` forever on the legacy path. It is now updated
  on every successful flip and mode-set.
* **`fb_destroy` left dangling plane references.** A plane kept naming a
  destroyed framebuffer id, and `fb_create` reuses ids, so a plane could appear
  bound to an unrelated buffer it had never seen. `fb_destroy` now unbinds every
  plane that names the id. (Linux's `drm_framebuffer_remove` also disables the
  CRTC; ours deliberately does not — see the doc comment for why.)
* **`atomic_commit` wrote `crtc.mode` without programming any hardware.** That
  was merely inaccurate before; it became dangerous the moment `page_flip`
  started trusting `crtc.mode` for its size check, because a fabricated mode
  would wave through a buffer the display engine reads with a different stride.
  Mode changes in the atomic path now route through `set_crtc`, so a mode the
  hardware refuses fails the commit instead of being recorded as fact.

**Regression test.** `drm::self_test()` item 11, "Mode-set and page-flip
discipline", runs on every boot: nine sub-tests covering an unadvertised mode,
an enable with no framebuffer, an enable with no connectors, a disable that also
names a framebuffer, an undersized framebuffer, an unknown connector, a real
mode-set followed by verifying `crtc.mode` and `plane.fb` describe it, a
matching flip against a mismatched one, and a disable after which a flip is
refused. Every assertion fails if the old behaviour is reintroduced.
