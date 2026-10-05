## 552. A mode-set the kernel refuses does not decline the display; the page-flip that follows decides

**Date:** 2026-08-25
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** To put a picture on a monitor the compositor has to tell the
graphics card which timing to run — the resolution and refresh rate — before it
hands over the first image. It never used to do this, because the kernel used
to fill the gap in on our behalf; the kernel stopped doing that, so a first
image on some cards now fails and the screen stays black. The fix is to send
the missing instruction. The question this entry settles is the smaller one:
what to do when *that* instruction is itself refused — give up on the monitor,
or carry on and let the image we send next be the judge. We carry on.

Lane A's `requests/a-c-drm-setcrtc-has-landed-and-page-flip-is-now-strict.md`
records the kernel side. In its terms: `DrmDevice::page_flip` now resolves the
CRTC, requires it to carry a programmed mode, and requires the framebuffer's
dimensions to equal that mode's — all above the backend dispatch. The ATI
backend's implicit mode-set inside the flip, which is what used to bring an
unprogrammed CRTC up, is gone. `gui/compositor/src/present/drm.rs` therefore
issues `DRM_IOCTL_MODE_SETCRTC` once per head, in `make_head`, between
registering the second framebuffer and the first flip.

### The decision: `set_mode`'s `Result` is discarded

*What changes:* a card whose mode-set fails but whose display is already timed
correctly keeps working, and reports no error. A card whose mode-set fails and
which is *not* already timed reports `Ioctl { request: PAGE_FLIP, errno:
EINVAL }` — naming the flip, not the mode-set that actually went wrong.

**The argument for it.** Nothing can hide behind the discarded error, because
the flip immediately afterwards is a strictly stronger test of the same
property. Exactly two things can follow a failed mode-set:

* the CRTC is already timed at the size we allocated — the mode-set was
  redundant, which is the normal case on `limine-fb` and `virtio-gpu`, where
  the only mode a connector advertises is the one it is already in — and the
  flip succeeds, correctly; or
* it is not — and the flip is refused, because the kernel's new checks are
  precisely "has a mode" and "the mode is the framebuffer's size". The head is
  then dropped through the path that already exists for a head whose first flip
  fails.

There is no third case. A mode-set that "half worked" is not a state the ioctl
can leave behind: the kernel updates `crtc.mode` only after the backend reports
success.

**The argument against it, and why it loses.** Diagnosis gets worse: the errno
the user sees names `PAGE_FLIP` when the real refusal was the `SETCRTC` a
moment earlier, so a bug report points one ioctl too late. That is a real cost
and it is the reason this is a decision rather than an obvious call.

It loses to a concrete regression on the other side. Treating the mode-set as
fatal declines the connector on any kernel that does not implement `SETCRTC`
at all — an older SlateOS, or any other DRM implementation the compositor is
pointed at — and declining the connector there blanks a screen that works
today. Trading a working display for a better error message is the wrong way
round, and the better error message is available anyway from the ioctl log.

### The strict fake immediately found a second bug

Making `FakeCard` strict was meant to give the new tests teeth. It also turned
two *existing*, previously-green tests red, and both were right to go red.

`resolve_crtc` prefers the CRTC a connector is already routed to — the one the
firmware lit at boot — and returned it without consulting the encoder's
`possible_crtcs`. A card can report a binding its own bitmask forbids, and until
now that was harmless: the CRTC really was scanning out, so a flip against it
worked. Under the new kernel it is not harmless, because `SETCRTC` refuses a
connector that is not routable to the named CRTC. The compositor would have
picked a CRTC it could never program, failed the mode-set, failed the flip, and
dropped a head that a legal CRTC would have driven perfectly. So the boot-bound
preference now applies only to CRTCs the bitmask agrees with — the same
reasoning that already declines to believe a binding naming a CRTC the card does
not list at all. The two tests' fixtures were self-contradictory cards; they
were rewritten to be internally consistent, and the contradiction they used to
contain became a test of its own.

This is the argument for strict doubles in one paragraph: the bug was not in the
code the change touched, and no amount of care writing the mode-set would have
surfaced it.

### Two smaller calls made along the way

* **The mode-set names the back buffer, not the front one.** A head starts at
  `front: 1`, meaning buffer 1 is notionally on screen and the first composed
  frame is drawn into buffer 0. Naming buffer 1 keeps that story true and makes
  the first flip a genuine change. Naming buffer 0 would have the CRTC scanning
  out the buffer the compositor is about to draw into, which is a tear on the
  very first frame — invisible in practice, because both buffers are zeroed,
  but wrong in a way that would become visible the moment anything pre-filled
  them.
* **The test double was made strict rather than the production code tested
  against a lenient one.** `FakeCard` now refuses a flip on a CRTC with no
  mode, and refuses a mode-set naming a mode the connector never advertised.
  Without that, every mode-set test would have been asserting that we *send*
  an ioctl, not that sending it is *necessary* — and deleting the `set_mode`
  call would have left the suite green. The cost is that the fake now has to
  track programmed modes and be kept in step with the kernel's checks; the
  benefit is that it fails the way the kernel fails.

### What is still not done

Changing to a mode *other* than the display's native one. `set_mode` is called
with the mode `best_mode` chose from the connector's advertised list, and
nothing above this module can ask for a different one. Lane A reports that
`limine-fb` and `virtio-gpu` refuse a retime outright (`TD-DRM-VIRTIO-GPU-CANNOT-RETIME`)
while ATI can now do it, so `TD-COMPOSITOR-CANNOT-CHANGE-MODE` is half-open
rather than closed. The sequencing for when it is done is already known and
recorded there: allocate the new pair of buffers at the new size *first*, then
`SETCRTC` to adopt them, then release the old pair — because `SETCRTC` refuses
a framebuffer smaller than the mode.
