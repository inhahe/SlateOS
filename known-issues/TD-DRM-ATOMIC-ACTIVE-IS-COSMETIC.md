## TD-DRM-ATOMIC-ACTIVE-IS-COSMETIC (lane A, 2026-08-21)

**In short:** the newer of the two ways to configure a display has a
"turn this screen on/off" switch that does not actually turn anything off. It
updates the kernel's record and nothing else, so a program that flips it sees
the record change but the monitor keeps showing the same picture.

**Where.** `atomic_commit` in `kernel/src/drm/atomic.rs`: the `cs.active` arm
sets `crtc.active` on the object and never calls the backend. Two code comments
point at this entry by name.

**Why it was left this way rather than half-fixed.** Making it real was
attempted and deliberately backed out. Turning a CRTC off has to be reversible,
and turning it back on means re-programming a display timing — which needs a
framebuffer that a bare `active: true` commit does not carry. The atomic
self-test (Test 8) deactivates and then reactivates the *primary* CRTC during
boot, with `mode: None` in both commits. A real disable there would clear
`crtc.mode`, leave the CRTC untimed after the supposed reactivation, and end the
boot on a black screen when the compositor's first `page_flip` was refused for
having no mode. Trading a harmless inaccuracy for a boot failure is not a fix.

For the same reason `DrmDevice::page_flip` deliberately does **not** require
`crtc.active` — only `crtc.mode`. A cosmetic bit must not be able to gate a real
operation.

**Proper fix.** Give `CrtcState` a way to express "off, and here is what to
restore when you come back": either the commit carries the framebuffer for the
re-enable, or the backend caches the last programmed timing and `active: true`
means "re-apply it". Then `active: false` calls `disable_crtc`, `active: true`
calls `set_mode`, and Test 8 is rewritten to supply a framebuffer on the way
back up. Also worth doing at the same time: real DPMS, which is the same
mechanism.

**Severity.** Low. Nothing in the tree currently sets `active` expecting
hardware to respond; the atomic path's only real user is the compositor, which
uses the mode and plane fields. It becomes wrong the moment a client tries to
blank a screen for power saving.
