## TD-COMPOSITOR-IGNORES-MONITOR-HOTPLUG (lane C, 2026-08-21) — RESOLVED 2026-08-21

**Resolution.** Both halves and the seam between them are done, in four commits;
`design-decisions.md` §517 records the reasoning and §516 the detach half.

- `773455e29` — *the seam.* `Present::monitors() -> Option<Vec<MonitorInfo>>`
  (default `None`, meaning "no opinion" — a headless recorder or a host window
  has no connectors to enumerate) and `Server::reconcile_monitors`, called at the
  top of `run_with`'s loop. It is a **poll of the whole set**, not a pushed
  difference: asking twice gives the same answer, a tick that fails is retried a
  second later, and the difference is computed against the arrangement actually
  held rather than the one a sender assumed. Removals run before additions, so
  peak surface is the smaller arrangement and a moved cable's CRTC is free before
  the new head asks for it.
- `382ae7a64` — *detect.* `DrmScanout::reprobe` re-reads `GETCONNECTOR` for every
  connector and diffs against the head list, rate-limited to `PROBE_INTERVAL`
  (1 s, `set_probe_interval` for tests) because a `count_modes == 0` probe makes
  the kernel do real DDC traffic — tens of ms, far more than a frame.
  `resize_to_heads` recomputes the framebuffer bounding box *without* moving any
  head, unlike the pre-existing `lay_out_heads`, keeping §515/§516's "survivors
  do not move" true on the scanout side too. This is also the one place that
  *reaps*: a retired head is removed and its buffers released rather than left
  `alive = false`, because a polled probe can retire a head every second for
  hours and a head list that grows per plug event is a leak with a physical
  trigger.
- `842026285` — *the shared key.* `Compositor::rename_display` plus `main.rs`
  naming every `Display` after its **connector id** instead of its enumeration
  index, and passing each head's real refresh rate instead of a hardcoded
  constant. This was the last item the original entry listed as outstanding, and
  it is load-bearing: a first screen still called `0` is both a display no
  connector claims and a connector no display claims, so reconciliation would
  detach it and attach a duplicate once a second, for ever.

Sixteen tests, every one proved a regression test by reintroducing the defect it
names and confirming a deterministic failure that names it back, source restored
byte-for-byte afterwards. Nineteen markers: `seamadoptsanyreply`,
`seamemptyisarrangement`, `seamnoreconcile`, `seamsizeisidentity`,
`seamreattachesknown`, `seamnewmonitorisprimary`; `probenever`,
`probeeverytime`, `probereportsindex`, `probearrivesatorigin`, `probereflows`,
`probekeepsdeparted`, `probeadoptsfirst`, `probekeepsdarkhead`,
`probefailureisempty`; `renamenoop`, `renamedemotes`, `renamecollides`,
`renamewrongdisplay`. One test had to be strengthened before it was honest —
`an_empty_monitor_list_is_not_an_arrangement_to_adopt` used one display and
passed with its guard deleted, because `detach_display` refuses the last monitor
anyway; with two displays the guard is the only thing holding the desktop
together. Compositor 465 green, clippy clean on `x86_64-pc-windows-gnu`
(`--all-targets`) and `x86_64-unknown-linux-gnu` (`--bins`, the only build that
compiles the Linux arm of `main.rs` at all), fmt clean.

**What is deliberately still out of scope,** and filed elsewhere: a **mode
change** under a live connector is ignored, because reconciliation compares ids
and never sizes — detaching and re-attaching to resize would destroy that
screen's window arrangement and move the monitor to the end of the row
(`TD-COMPOSITOR-CANNOT-CHANGE-MODE`). **EDID is not read**, so a monitor is
identified by the socket it is in and not by which monitor it is; swap two
cables and each takes the other's place in the row. And **nothing is
remembered**: unplug a monitor and plug it back in and it returns at the
right-hand end rather than where it was. All three want an identity that
outlives a cable, which is a larger design than this one.

<details><summary>Original entry (describes pre-<code>773455e29</code> code)</summary>

**In short:** plugging a second monitor in while the desktop is running does
nothing — it stays dark until the display server is restarted. Unplugging one
works, in the sense that the remaining screen keeps going, but the desktop does
not shrink back and windows stranded on the departed monitor stay stranded.

**What.** `DrmScanout::new` enumerates connectors once and builds the head list
from that. Nothing re-reads `GETCONNECTOR` afterwards, so a connector that
becomes `CONNECTED` later is never noticed. On the way out, a head whose flips
start failing is marked dead (`head.alive = false`) and stops being drawn on,
which keeps the *other* monitors alive — but the composited frame keeps its
size, `Compositor` is never told the display went away, and so
`DisplayManager` still lists it, `work_bounds_for` still resolves to it, and a
window maximised there is on a screen that no longer exists.

**Where.** `gui/compositor/src/present/drm.rs` — `DrmScanout::new`,
`lay_out_heads`, and the `Err(_)` arm of `Present::show`.

**What the proper fix looks like.** Two halves, and the second is the load-
bearing one:

1. *Detect.* DRM signals hotplug with a uevent on a netlink socket, which
   SlateOS's kernel does not expose. The portable fallback is to re-probe every
   connector periodically (a `GETCONNECTOR` per connector per second is cheap)
   and diff against the head list. Polling is the right first implementation
   here because it needs nothing from lane A.
2. *Propagate.* A head appearing or disappearing has to reach `Compositor` as
   an `attach_display` / a new `detach_display`, which must re-run
   `relayout_for_desktop_change` so the surface is resized, fullscreen windows
   are re-fitted and stranded windows are rescued — the machinery §512 and §513
   built, which already does exactly this for a *resize* and has never been
   asked to do it for a removal. `detach_display` does not exist yet and is the
   real work: it is `attach_display` in reverse, and it must decide what happens
   to a window whose only monitor left, which `bring_stranded_windows_back`
   already answers for the resize case.

Note that `Present` currently has no way to tell its caller anything except
"still open", so the seam for (2) has to be added: either a method that returns
the head list and is polled by `Server::run_with`, or the same
`Present::input`-shaped channel that `TD-COMPOSITOR-HAS-NO-LOCAL-INPUT` needs.
Doing both through one mechanism is probably right and is a reason to do them
together.

**Severity.** Low. It costs a restart, on hardware SlateOS does not yet run on
(QEMU presents one card), and nothing is corrupted — the surviving monitors
keep a correct picture. It rises to medium alongside any real bare-metal
bring-up, and it is the obvious next thing after
`TD-COMPOSITOR-DRIVES-ONE-HEAD`.

**Update 2026-08-21 — half (2), *propagate*, is done.**
`Compositor::detach_display` exists (lib.rs, beside `attach_display`) with
`DisplayManager::remove_display` under it, and design-decisions.md §516 records
why it is not simply `attach_display` run backwards: it adopts the removal
*first* and shrinks the surface after, because the monitor is already gone and a
surface that stays too large still covers the desktop; and the surviving
monitors keep the offsets they had, because §515 already decided the scanout
will not re-flow its surviving heads and the two layouts have to agree pixel for
pixel. Everything that re-places the stranded windows turned out to be §512's
existing `relayout_for_desktop_change` — no new pass was needed, which is what
§513's "reachable is a question about the whole desktop" bought. 10 tests, all
proved by reintro markers (`detachnopromote`, `detachreflows`,
`detachwrongdisplay`, `detachlastmonitor`, `detachnoshrink`, `detachnorelayout`,
`detachhomeisdesktop`).

**What is left is half (1), *detect*, and the seam.** Nothing calls
`detach_display` yet, and nothing calls `attach_display` after startup either,
so the observable behaviour is unchanged: hotplug is still ignored. What remains
is (a) `DrmScanout` re-probing `GETCONNECTOR` periodically and diffing against
its head list, and (b) a way for it to tell `Server::run_with` what changed —
which is still the seam this entry describes, and still probably wants to be the
same mechanism `TD-COMPOSITOR-HAS-NO-LOCAL-INPUT` needs. One further thing the
wiring must fix: `main.rs` builds each `Display` with `id = <enumeration
index>`, but `detach_display` names a display by id and the stable key on the
scanout side is the **connector id** (§515), so the two have to be made the same
number before a detach can name the right screen.

</details>
