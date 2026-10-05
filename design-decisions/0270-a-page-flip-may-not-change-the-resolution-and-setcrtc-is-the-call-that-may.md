## §270 — A page flip may not change the resolution, and `SETCRTC` is the call that may

**Date:** 2026-08-21
**Decided by:** Claude (autonomous), on a request filed by lane C
(`requests/c-a-drm-setcrtc-and-a-page-flip-that-refuses-a-mismatched-framebuffer.md`).
Lane C described the symptom and named the two options; the choice between them
was not put to the operator because one of them was not tenable — see below.
**Lane:** A

**In short:** A program that draws the screen hands the kernel a picture and
says "show this one now". Until today, if the picture it handed over was a
different size than the screen, the kernel did something different depending on
which graphics card was fitted: an ATI card silently changed the screen
resolution to match the picture, and a virtual machine's card silently showed
only the top-left corner of it. Neither told the program anything had happened,
and afterwards, when the program asked "what resolution am I in?", both
answered with the resolution the machine booted at — which by then was wrong on
one of them and right on the other, for reasons the program had no way to see.
Now a wrong-sized picture is refused outright with an error, and there is a
separate call — the one Linux programs already use, `SETCRTC` — for a program
that actually *wants* to change the resolution.

### The option that was not tenable

There were three ways out, and only two of them were real:

| | What changes | |
|---|---|---|
| **A. Refuse the mismatch; add `SETCRTC`** | A wrong-sized flip returns `EINVAL`; changing resolution needs an explicit call. | **chosen** |
| **B. Make the implicit mode-set official** | All three backends retime on a mismatched flip; documented as a SlateOS extension. | rejected |
| **C. Leave it** | Behaviour keeps depending on the fitted card. | not an option |

C is not a conservative choice, it is the absence of a choice. The current
behaviour is not one behaviour with a rough edge; it is two incompatible
behaviours selected by hardware the program cannot query. A compositor
developed against virtio-gpu and shipped to a machine with an ATI card changes
the user's resolution as a side effect of a routine frame. Lane C put this
plainly and correctly: *"the one option that is not tenable is the current one,
where the answer depends on which card is fitted."*

Between A and B, A wins on three grounds:

1. **It is what every Linux client already expects.** `DRM_IOCTL_MODE_PAGE_FLIP`
   returning `EINVAL` on a size mismatch is `drm_mode_page_flip_ioctl`'s
   behaviour, and clients are written against it. B would mean a client that
   works everywhere else silently behaves differently here — the worst kind of
   incompatibility, because it produces a wrong picture rather than an error.
2. **B cannot actually be implemented on all three backends.** The Limine
   backend scans out a framebuffer the bootloader programmed and has no register
   access to retime it; virtio-gpu creates its scanout resource once at probe
   from `GET_DISPLAY_INFO` and never replaces it. "All three backends retime"
   would in practice be "one backend retimes and two return an error from a call
   that is documented not to fail" — the same divergence, moved.
3. **A mode change is not free and should not be invisible.** It reprograms a
   PLL, blanks the panel for tens of milliseconds and invalidates every cursor
   and overlay position. A call that may do that should be a call the program
   knowingly makes.

### The cost A carries, and why it is paid up front

Strictness has a real price: the ATI backend's CRTC enumerates with no mode
programmed at all (`active: false, mode: None`), and the implicit mode-set
inside `page_flip` is what used to bring it up. Making `page_flip` strict
without more would mean the ATI card's *first ever* flip fails instead of
lighting the display.

So `SETCRTC` is not an addition alongside the fix — it is a prerequisite of it,
and this change lands both. `DrmDevice::ensure_crtc_configured` performs the
first mode-set explicitly, and `ScanoutBuffer::new` calls it before its first
flip. That has the useful secondary effect of putting `SETCRTC` on the boot
path, so the code that replaced the implicit mode-set is exercised on every
boot rather than only when a client happens to call it.

### Four smaller calls made inside this one

**A stated refresh rate is binding; an unstated one is a wildcard.** `SETCRTC`
matches the requested mode against the connector's advertised list on
`hdisplay`/`vdisplay`, and on `vrefresh` only when the caller supplied a
non-zero one. Linux's `drm_mode_equal` ignores `vrefresh` entirely, because it
is a field derived from the timing rather than part of it — but ignoring it
means a client that explicitly asks for 60 Hz can be silently given 75, which is
the class of silence this whole entry is about. Treating zero as "don't care"
keeps the Linux behaviour available to clients that don't fill the field in,
without making an explicit request unenforceable.

**The kernel stores its own copy of the mode, never the caller's.**
`drm_mode_to_uapi` writes zeros for `hsync_start`, `hsync_end`, `hskew`,
`vsync_start`, `vsync_end`, `vscan` and `flags`. A client that reads a mode from
`GETCONNECTOR` and hands it straight back to `SETCRTC` is therefore *not*
returning what it was given, and honouring the struct as written would program a
timing with no sync pulses — a blank display. So the caller's struct selects a
mode; the kernel's own copy of that mode is what gets programmed and what
`GETCRTC` subsequently reports.

**`disable_crtc` is a no-op that reports success on the shared-console
backends.** On `limine-fb` and `virtio-gpu` the only way to darken the display
is to zero the framebuffer — and that surface is the kernel console's. A
compositor exiting cleanly would therefore erase the panic output explaining
why it exited. Returning an error instead is worse: turning the display off is
the normal last act of a well-behaved client, and a client that gets an error
from it has no correct response. So it succeeds and does nothing, which is
exactly true of what the *display* does, and the object model is updated so
`GETCRTC` reports the CRTC as off.

**The object model is updated only after the hardware agrees.** Every write to
`crtc.mode`, `crtc.active` and `plane.fb` happens after the backend returns
`Ok`, so a failed mode-set leaves `GETCRTC` describing what is genuinely still
being scanned out. On ATI this goes one step further: `self.mode` is recorded
only after `modeset::verify_applied` has read the registers back, because a
`self.mode` taken from the plan rather than the hardware would make a failed
mode-set look successful to the very next `page_flip` — which, seeing a mode it
believes is already programmed, would take the single-register fast path.

### What was left half-done on purpose

`atomic_commit`'s `active` flag remains cosmetic: it updates `crtc.active` in
the object model without touching hardware, tracked as
`TD-DRM-ATOMIC-ACTIVE-IS-COSMETIC` in `known-issues.md`. Making it real was
attempted and backed out. The reason is that DPMS-off has to be reversible, and
re-enabling a CRTC means re-programming a timing, which needs a framebuffer that
a bare `active: true` commit does not carry. The existing atomic self-test
deactivates and reactivates the *primary* CRTC during boot with `mode: None` in
both commits; a real disable there would clear `crtc.mode`, leave the CRTC
untimed after the supposed reactivation, and end the boot on a black screen when
the compositor's first flip was refused. Half-fixing it would have traded a
harmless inaccuracy for a boot failure. For the same reason `page_flip`
deliberately does *not* require `crtc.active`.
