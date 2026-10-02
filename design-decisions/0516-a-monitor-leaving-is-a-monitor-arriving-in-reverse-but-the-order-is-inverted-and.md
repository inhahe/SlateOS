## 516. A monitor leaving is a monitor arriving in reverse, but the order is inverted and the survivors do not move

**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** unplugging one of two monitors used to do nothing to the desktop's
model of itself. The surviving screen kept working, but the compositor still
believed the departed monitor was there, still composited a rectangle of frame
that nothing copied out, and still left windows sitting on it — including
maximised ones, which have no title bar edge on any surviving screen to drag
back by, so they were gone until the session was. `detach_display` is the
missing half. It removes the display, shrinks the composited surface, and lets
the existing re-layout machinery put everything back on a screen that exists.

**The order is the reverse of `attach_display`'s, and that is the decision.**
Attaching allocates the larger surface *first* and adopts the display only if
that succeeded, because a desktop wider than its framebuffer has a monitor with
no pixels behind it (§514). Detaching adopts first and shrinks after. The
asymmetry is not an oversight: the monitor is already physically gone, so
refusing to acknowledge it — which is what propagating a failed shrink would
do — leaves the model describing a screen that does not exist, and *that* is the
bug being fixed. A surface that stays too large still covers the smaller
desktop completely. A failed shrink wastes memory; it cannot draw anything
wrong. The failure path does still have work to do, though, and it is not
nothing: the over-large surface holds the departed monitor's last frame, so the
full clear the successful path performs has to be repeated by hand or that
picture stays on screen.

**The survivors keep the offsets they already had.** Take the leftmost of two
monitors away and the other one stays at x = 800: the virtual desktop starts
part-way along, and the space the departed screen occupied remains in the
bounding box as a hole that is composited and scanned out nowhere. Re-flowing
them leftwards is plainly the tidier arrangement, and it is deliberately not
done.

The reason is that there are two layouts, not one. `DisplayManager` holds the
compositor's, `DrmScanout` holds the scanout's, and they agree only because both
lay monitors out left-to-right in enumeration order — agreement by construction
rather than by protocol, which is what let §515's multi-head scanout land
without a single trait signature changing. §515 also decided that a head which
*fails* mid-session does not re-flow the others, because a monitor going dark
must not drag its neighbours' pictures sideways. So the scanout will not
re-flow, which means the compositor must not either: a re-flow on one side alone
puts every window on the wrong screen, silently, with both sides internally
consistent. One of the two has to be the authority and it is the one holding the
framebuffers.

*The alternative considered and rejected:* re-flow both sides together, since a
*detach* — unlike a mid-session flip failure — is a deliberate act we drive from
one place and could sequence. It would cost every window on every monitor to the
right of the departed one being displaced by its width, which is a far more
visible event than a hole, and it would make the two layouts agree only while
the detach path is the *only* way a head disappears. It is not: a flip failure
still marks a head dead without telling anyone. Two rules that must be kept in
step are worse than one rule applied twice.

**Nothing new re-places the stranded windows.** With the display removed,
`display_for` answers *primary* for any window that no longer overlaps a real
monitor, so `relayout_for_desktop_change` — written for §512's resize case and
untouched here — does the whole job: the re-tile picks up maximised and snapped
windows, `refit_fullscreen_windows` re-fits fullscreen ones and tells their
clients, `bring_stranded_windows_back` rescues hand-placed ones, and
`pull_pointer_onto_the_desktop` retrieves the cursor. That the removal case
needed no new pass is a consequence of §513 having defined "reachable" against
the whole virtual desktop rather than against one screen.

**What it is aimed at is the primary, not the desktop.** For two monitors those
are the same rectangle and the choice looks arbitrary. For three they are not:
take the *middle* monitor away and the desktop's bounding box spans the hole, so
it is not any monitor's bounds — and the re-tile's "windows on the screen that
changed" filter, which compares against a monitor's bounds, then matches nothing
at all and leaves a maximised window in the gap. That is the no-title-bar
failure again, reached by a different route.
`a_window_maximised_on_a_monitor_taken_from_the_middle_comes_back` is the only
test that separates the two, and it exists because the distinction is invisible
in every two-monitor case.

**Detaching the last monitor is refused.** A desktop with no monitors has
zero-sized virtual bounds, no primary to fall back to, and every window stranded
with nowhere to be rescued to; `pull_pointer_onto_the_desktop` already carries
an explicit guard against the malformed clamp such an arrangement produces.
There is no arrangement to adopt, so the display is kept and the caller told. A
display server whose only screen has been unplugged has nothing useful left to do
either way, and keeping a screen it cannot paint on is strictly better than
adopting a desktop it cannot describe.

**What this does not do yet: notice.** Nothing calls `detach_display` — that is
the *detect* half of `TD-COMPOSITOR-IGNORES-MONITOR-HOTPLUG`, which needs
`DrmScanout` to re-probe its connectors and a seam through `Present` to report
what changed. It is deliberately a separate change: the model half is testable on
a machine with no graphics card, and pairing it with the DRM half would have made
one commit whose interesting half could only be exercised by the other.

**Verification.** 10 new tests, each proved a regression test by reintroducing
the defect it names through the guarded patcher and confirming a deterministic
failure that names it back: `detachnopromote`, `detachreflows`,
`detachwrongdisplay`, `detachlastmonitor`, `detachnoshrink`, `detachnorelayout`
(which proves four at once — the four re-layout passes have one call site
between them) and `detachhomeisdesktop`. Compositor 449 + 18 green, clippy and
fmt clean.
