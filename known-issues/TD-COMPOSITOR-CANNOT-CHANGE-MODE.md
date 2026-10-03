## TD-COMPOSITOR-CANNOT-CHANGE-MODE (lane C, 2026-08-21)

**In short:** on SlateOS the desktop runs at whatever resolution the monitor was
already using when the machine booted, and nothing can change it. There is no
"Display settings → Resolution" that can work. This is a missing kernel feature,
not a compositor bug.

**What.** `DrmScanout::new` reads the connected display's preferred mode and
builds the compositor at that size. It never sets a mode, because
`kernel/src/drm/` implements no `DRM_IOCTL_MODE_SETCRTC`.

**The Settings side, audited 2026-09-17 and not previously written down.** The
Display page draws a Resolution dropdown and a Refresh Rate dropdown, and
neither reaches anything at all -- not even a file. `resolution_index` and
`refresh_rate_index` occur in exactly four places in `apps/settings`: the
field, its default, the label the row shows, and the dropdown's own item list.
There is no save and no request; choosing 2560x1440 changes a number in memory
that is forgotten when the window closes. And `RESOLUTIONS` is a hardcoded
list of eight common modes, not the modes the display reports -- the same
invention the Network Status page had removed when it turned out this system
cannot enumerate its interfaces.

That is the standard the Mouse page sets in its own doc: a control drawn for
something with no consumer "would save a value to a file, look exactly as
though it had worked, and change nothing", which is what
`TD-C-THE-MOUSE-SETTINGS-PANEL-REACHES-NOTHING` was filed about. These two
rows are below even that bar, since they do not reach a file either.

Deliberately not removed here. The backend half below has moved -- `SETCRTC`
works on the ATI backend now -- so the choice is between deleting two rows and
finishing the path they need, and the remaining piece of that path (the buffer
re-allocation before `SETCRTC`) is lane C's own. Deleting them first would be
churn if that lands. `PAGE_FLIP` works
without it — the CRTC is already scanning out the mode the firmware programmed —
so this is a limitation rather than a blocker, but it means the mode we get is
the mode we keep. The compositor binary reports `--size` as ignored on this path
rather than silently obeying it, since obeying it would compose frames the
display cannot show.

**What is missing:** `SETCRTC` in lane A's `kernel/src/drm/`, a re-allocation of
the pair of dumb buffers at the new mode's size in `DrmScanout`, and the call
that sequences the two.

**Severity.** Low. A display running its own native mode is the right default,
and it is what the user is already looking at. This bites only when the native
mode is wrong for the user (a projector, a scaled-down mode for performance, a
panel whose EDID lies).

**Update 2026-08-21 — the compositor half is done.** What this entry used to
call "a second, separate piece of work" — `Compositor::new` takes a size and
everything downstream assumes it for its lifetime — is finished.
`Compositor::resize_display` now re-derives, against the new size, everything
that was placed *by a rule* rather than by the user: maximised and snapped
windows are re-tiled through the work area (so they land above a taskbar, not
under it), fullscreen windows are re-fitted to the new framebuffer and told
about it, any window the shrink left entirely off-screen is pulled back by the
smallest movement that recovers it, and the pointer is clamped onto the virtual
desktop. A window the user placed themselves and can still reach is left exactly
where it was — a resize is not permission to re-lay-out the desktop. 12 tests,
each proved a regression test by reintroducing the defect it names. Rationale
and the four separate failures it fixes: `design-decisions.md` §512.

**Filed to lane A?** Yes —
`requests/c-a-drm-setcrtc-and-a-page-flip-that-refuses-a-mismatched-framebuffer.md`
(2026-08-21). It was deliberately *not* filed before now: a `SETCRTC` with no
caller is worse than none, and the caller needed compositor resize first. Resize
has landed, so the caller exists. That request also carries a second, separate
bug found while writing it — see `BUG-DRM-PAGE-FLIP-ACCEPTS-A-MISMATCHED-FB`
below.

**Update 2026-08-21 — the kernel half is done too (lane A).**
`DRM_IOCTL_MODE_SETCRTC` exists: `DrmDevice::set_crtc` in `kernel/src/drm/mod.rs`
validates the requested mode against the connector's advertised list, checks the
connector is routable to that CRTC through its encoder, checks the framebuffer
covers `mode + (x, y)`, dispatches `set_mode` to the backend, and updates
`crtc.active`, `crtc.mode` and the primary plane's `fb`/`src_*`/`dst_*` only
after the backend reports success. `fb_id == 0` with no connectors is the
supported "turn this CRTC off" form and succeeds rather than returning `EINVAL`.
The ioctl is wired at `kernel/src/syscall/linux.rs` and is refused with `EACCES`
on a render node, since programming a display timing is modeset authority.

Two caveats for the compositor half:

* **Only the mode the backend already has is accepted on `limine-fb` and
  `virtio-gpu`.** Both return `EINVAL` for any other mode — Limine cannot retime
  a framebuffer the bootloader programmed, and virtio-gpu creates its scanout
  resource once at probe and never replaces it (see
  `TD-DRM-VIRTIO-GPU-CANNOT-RETIME` below). So "Display settings → Resolution"
  becomes genuinely possible on the ATI backend now, and remains impossible in
  QEMU until virtio-gpu grows a resource-recreate path. It fails loudly in both
  cases, which is the change that matters: the compositor can now *tell*.
* **The buffer re-allocation is still lane C's.** `SETCRTC` refuses a
  framebuffer smaller than the mode, so `DrmScanout` must allocate the new pair
  of dumb buffers at the new size *before* the `SETCRTC` that adopts them, not
  after.

Details and the reasoning: `design-decisions.md` §270. Reply request filed as
`requests/a-c-drm-setcrtc-has-landed-and-page-flip-is-now-strict.md`.

**Update 2026-08-25 — the compositor now issues the mode-set, and had to.**
Lane A's second caveat above understated the urgency: with `page_flip` strict,
*not* mode-setting is no longer a limitation, it is a black screen on the ATI
backend — that backend's CRTCs enumerate with `active: false, mode: None`, and a
first flip onto a CRTC with no mode is `EINVAL`. `gui/compositor/src/present/drm.rs`
now issues `DRM_IOCTL_MODE_SETCRTC` once per head in `make_head`, after both
dumb buffers are registered and before the head's first flip, naming the
connector, the head's back buffer, and the mode `best_mode` picked from the
connector's advertised list. `uapi.rs` gained `SETCRTC` (`0xC068_64A2`) and
`ModeCrtc` (104 bytes), pinned by the same offset and `_IOC`-size tests as every
other struct there.

The mode-set's error is deliberately discarded — the flip that follows is a
strictly stronger check of the same property, and treating it as fatal would
decline the connector on any kernel with no `SETCRTC` at all. Reasoning, and
the diagnosis cost it accepts: `design-decisions.md` §552.

The test double was made strict in the same change, which is the part that
makes the tests mean anything: `FakeCard` now tracks the mode programmed on
each CRTC, refuses a flip on a CRTC that has none or whose mode is not the
framebuffer's size, and refuses a mode-set naming a mode the connector never
advertised or a connector its encoders cannot route. Six new tests, including
one that asserts the fake's own strictness so that a future permissive fake
fails loudly rather than silently making `set_mode` deletable.

**Still open:** changing to a mode other than the display's native one. Nothing
above this module can ask for one, and two of the three backends refuse it
anyway. This entry stays open at the same low severity, now for a smaller
reason than when it was filed.
