## 517. Monitor hotplug is a polled whole-set reconciliation keyed on the connector id

**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the compositor could add a monitor (§514) and drop one (§516), and
the scanout could drive several at once (§515) — but nothing ever *noticed* that
a cable had moved. Both halves had to be told by hand. Plug a second monitor in
after boot and it stayed dark for the life of the session; unplug one and the
desktop kept a screen-sized hole full of windows nobody could reach. This
section is the wiring that joins the two: once a second the compositor asks the
graphics card which monitors are actually attached, and changes its arrangement
to match. Three terms recur below. A *connector* is one physical socket on the
card (a particular HDMI or DisplayPort jack); its *connector id* is the small
number the kernel gives that socket, stable for as long as the machine is up. A
*CRTC* is the card's scanout engine — the thing that reads pixels out of memory
and drives a cable — and there are fewer of them than sockets. A *head* is our
name for one live connector-plus-CRTC pair.

**The answer is polled, not pushed.** The card can raise a hotplug event, and
using it would be the obvious design: the kernel already knows the instant a
cable moves, so why ask? Because a pushed message describes a *difference*, and
a difference is only meaningful against the state the sender believed the
receiver had. Drop one, mis-order two, or fail to apply one because a CRTC was
momentarily busy, and every later message is being applied to an arrangement
that is not the one it was computed for — and nothing ever notices, because
there is no point at which the two sides compare totals. A poll returns the
*whole set*. Asking twice gives the same answer, a tick that fails is simply
retried a second later, and `reconcile_monitors` computes the difference itself,
at the moment it acts, against the arrangement it is actually holding. The cost
is the reason polling is not free: reading a connector's state with
`count_modes == 0` makes the kernel re-probe it, which is real DDC traffic down
the cable and tens of milliseconds per head — far more than a frame. So the
probe is rate-limited to once a second (`PROBE_INTERVAL`, overridable via
`set_probe_interval`, which the tests set to zero), and `monitors()` returns the
cached head list on every other call. One second of blackness on a screen that
was just plugged in is not perceptible as a fault; a frame that misses vsync
because it stopped to talk to a monitor is.

**`None` and an empty list are different answers.** `Present::monitors` returns
`Option<Vec<MonitorInfo>>`, and the two negative cases mean opposite things.
`None` is *no opinion*: a headless test recorder or a host window has no
connectors to enumerate and no business overruling whatever displays the
compositor was configured with. `Some(vec![])` is a card that had monitors and
has none left. Both are declined today, and — this is worth stating plainly
rather than glossing — they are declined by two separate guards with *identical*
observable behaviour, so no test can currently tell them apart. The empty case
is declined because detaching every display to match would leave a compositor
with a zero-sized desktop and no surface to composite into; a machine whose last
monitor is unplugged is expected to end its session, not to keep running as a
zero-by-zero desktop. Keeping the distinction in the type costs nothing and
means the day one of them wants to behave differently, the information is still
there.

**Reconciled by connector id, and only by connector id.** The compositor's
`Display::id` is set to the connector id of the socket the monitor is plugged
into, which is what makes the two sets comparable at all — hence
`Compositor::rename_display`, which exists solely so the first display, created
before any connector is known, can be renamed from `0` to its real connector id
at startup. Get that wrong and the failure is not subtle: a first screen still
called `0` is simultaneously a display no connector claims *and* a connector no
display claims, so every tick detaches it and attaches a duplicate, once a
second, for ever.

Sizes are compared nowhere. A monitor whose *mode* changed under a live
connector is deliberately left alone: the id still matches, so reconciliation
sees nothing to do. Doing otherwise would mean detaching and re-attaching, which
would destroy that screen's window arrangement and move the monitor to the right
end of the row — a spectacularly destructive way to change a resolution.
Changing a mode in place is `TD-COMPOSITOR-CANNOT-CHANGE-MODE` and stays out of
scope.

**Removals happen before additions, on both sides.** Peak resource use is then
the *smaller* of the two arrangements rather than their union, which matters
because a CRTC is exclusive: a monitor moved from one socket to another needs
the old head to give its CRTC back before the new one can take it. The scanout's
re-probe does the same in the same order, for the same reason, and a test builds
exactly that case — two sockets that share one CRTC, cable moved between them.

**A retired head is destroyed, not merely marked dead.** Everywhere else in the
scanout a head that fails is left in place with `alive = false`, keeping its
buffers until `Drop`. That is affordable because a head fails at most once. A
*polled* probe is different in kind: a cable working loose can retire and
re-adopt a head every second for hours, and a head list that grows once per plug
event is a leak with a physical trigger. So `reprobe` is the one place that
reaps — it removes dead heads from the list and releases their framebuffers back
to the card immediately, bounding the list by the number of monitors rather than
by the number of times somebody wiggled a cable.

**The survivors still do not move — now on the scanout side too.** §516 decided
that detaching a display does not re-flow the remaining ones, and the same rule
had to be re-established inside `DrmScanout`, where the existing `lay_out_heads`
*does* re-flow and is called only from `new()`. `resize_to_heads` is its
non-moving sibling: it recomputes the framebuffer's bounding box from the live
heads without touching any head's position. Both sides now place an arriving
monitor at the right edge of the ones already present and neither ever moves
one, so the compositor's layout and the scanout's agree pixel-for-pixel by
construction, with no protocol between them. That agreement is also why
`MonitorInfo` carries no position: a position that travelled would be a second
source of truth for something both sides already derive identically.

**What this does not do.** It does not change a mode under a live connector (see
above). It does not read EDID, so a monitor is identified by the socket it is in
and not by which monitor it is — move two monitors' cables to each other's
sockets and each takes on the other's place in the row. And it remembers
nothing: unplug a monitor and plug it back in and it returns at the right-hand
end, not where it was. All three want an identity that outlives a cable, which
is a larger design than this one.

**Verification.** Sixteen new tests, every one proved a regression test by
reintroducing the defect it names and confirming a deterministic failure that
names it back, with the source restored byte-for-byte (SHA-256) afterwards.
Nineteen markers, because several defects are only reachable in combination:
the seam's six (`seamadoptsanyreply`, `seamemptyisarrangement`, `seamnoreconcile`,
`seamsizeisidentity`, `seamreattachesknown`, `seamnewmonitorisprimary`), the
re-probe's nine (`probenever`, `probeeverytime`, `probereportsindex`,
`probearrivesatorigin`, `probereflows`, `probekeepsdeparted`, `probeadoptsfirst`,
`probekeepsdarkhead`, `probefailureisempty`), and the connector-id naming's four
(`renamenoop`, `renamedemotes`, `renamecollides`, `renamewrongdisplay`).
`seamadoptsanyreply` removes the `None` guard and the empty guard together,
because as noted above a defect that removes only one of them is not
distinguishable from one that removes the other.

One of these tests had to be strengthened before it was honest.
`an_empty_monitor_list_is_not_an_arrangement_to_adopt` originally used a single
display, and passed with the empty-list guard deleted — `detach_display` refuses
to remove the last monitor, so the desktop survived a missing guard by accident.
With two displays the guard is the only thing standing between an empty reply
and a half-dismantled desktop, and the test fails as it should. Compositor 465
tests green; clippy clean on `x86_64-pc-windows-gnu` (`--all-targets`) and on
`x86_64-unknown-linux-gnu` (`--bins`, which is the only way the Linux-only arm
of `main.rs` is compiled at all); fmt clean.
