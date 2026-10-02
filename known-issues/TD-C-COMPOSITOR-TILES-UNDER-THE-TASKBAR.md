## TD-C-COMPOSITOR-TILES-UNDER-THE-TASKBAR (lane C, 2026-08-21) — MOSTLY RESOLVED 2026-08-21, step 4 blocked

**In short:** When you tile a window — drag it to the left edge, or press
Super+Left, or pick a slot from the Super+Z chooser — it *used to* be made to
fill exactly half (or a third, or a quarter) of the *entire screen*, including
the strip at the bottom where the taskbar lives, so the bottom of a tiled window
slid underneath the taskbar and whatever was down there — a status line, a
scrollbar's bottom arrow, the last row of a list — was covered up. The
compositor now has an edge-reservation ("strut") request and subtracts what is
reserved from every tiling route, so as soon as something *makes* a
reservation the problem is gone. Nothing makes one yet, because the shell is
still not a compositor client — that is the one piece left, and it is blocked
on `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR`.

**Status of the four steps below:** 1, 2 and 3 are **done** (2026-08-21). Step 4
is blocked, and until it lands a running system still tiles under the bar
because the reservation is never requested.

**Where it was.** `gui/compositor/src/lib.rs` `Compositor::snap_area()`, since
deleted, which was one line:

```rust
fn snap_area(&self) -> WorkArea {
    work_area_of(self.display_manager.virtual_bounds())
}
```

`virtual_bounds()` is the full display arrangement, and `work_area_of` only
converts the type — it subtracted nothing. Every tiling route funnelled through
here (`snap_window`, `snap_window_to_zone`, and the `drop_at` edge-drag path),
so the defect was uniform rather than per-gesture, and the method was already
commented as the one place a fix lands. (That single funnel was replaced in
`TD-C-TILING-MEASURED-EVERY-MONITOR-AT-ONCE` by the per-monitor family
`work_bounds_for` / `work_area_for` / `work_area_at` / `work_area_of_window`,
which kept the funnel property — and it is `work_area_for`, still the one
place, that now does the subtracting.)

**Where it is now.** `gui/remote/src/reserve.rs` holds the shared rules
(`PanelEdge`, `ReservedEdges`, `MAX_RESERVED_FRACTION`, the clamp);
`RequestBody::ReserveEdge` / `ResponseBody::WorkArea` (tags `0x11` / `0x05`)
carry it over the wire; `Window::reserved_edge` stores one claim per window and
`Compositor::{reserved_on, reserve_edge, retile_for_work_area_change}` apply
it. `gui/compositor/src/wire.rs` resolves ownership and then `require_shell`.

**Why it is like this.** The compositor has no concept of a *strut* — a client
declaring "reserve N pixels along this edge for me, and keep everyone else out
of it." X11 has `_NET_WM_STRUT_PARTIAL`, Wayland has `zwlr_layer_shell`'s
exclusive zone; `gui/remote`'s protocol has neither. The shell does know the
number — `Shell::taskbar_height` (`gui/desktop/src/lib.rs:648`, default 40
logical px, scaled by `taskbar_height_px()`) — and `icons.rs` already subtracts
it for desktop-icon layout. But it has no way to *tell* the compositor, and in
fact does not connect to it at all yet (`TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR`).
So the one component that knows the answer and the one component that needs it
have no channel between them.

**The proper fix** — in the order the pieces have to exist:

1. **[x] done.** Add a reserved-area request to `gui/remote`: a client asks for
   `n` pixels along one edge of one output, and the compositor answers with the
   work area that leaves. Model it on `zwlr_layer_shell`'s exclusive zone, not
   on `_NET_WM_STRUT_PARTIAL` — the X11 form encodes start/end offsets along the
   edge, which exists to support multiple panels sharing one edge and is the
   part of that protocol everyone gets wrong. Reject a reservation larger than
   some fraction of the output (a client asking for 100% of the screen must not
   be able to shrink the work area to nothing).
   → `RequestBody::ReserveEdge { window, edge, size }` answered with
   `ResponseBody::WorkArea`. It names an *output* by naming the panel's **own
   window** — the protocol has no output ids, and a window answers "which
   monitor" and "for how long" at once. `size == 0` is the release, so there is
   no second request to keep in step. A greedy claim is **clamped** to
   `MAX_RESERVED_FRACTION` (a third) per edge rather than rejected: the
   realistic failure is a panel that mismeasured its own height, and a
   third-of-screen taskbar is recoverable while a zero work area is not.
   See `design-decisions.md` §510.
2. **[x] done.** Track the reservations per output in the compositor and make
   the tiling area come back as bounds minus them. Because it is derived on
   every call rather than cached, nothing else needs to change for tiled windows
   to follow a taskbar that moves or changes height — but *already-tiled*
   windows will need re-placing when the work area changes, the same way they
   are re-placed on a display hotplug.
   → `Compositor::reserved_on` sums the claims of the visible, non-minimized
   panels whose own monitor is the one being asked about, and `work_area_for`
   applies them. Re-placement is `retile_for_work_area_change`, and it works
   because `maximized` and `SnapTarget` store the *request* rather than the
   rectangle: re-running the same request against the new area is the whole fix.
3. **[x] done.** Gate the request on a capability. Any client being able to
   carve a strip out of every other client's tiling is
   `TD-C-ANY-CLIENT-CAN-READ-EVERY-WINDOW-TITLE` in a more damaging form, and
   this one is worth getting right the first time rather than adding the check
   afterwards.
   → `wire.rs` runs `link.resolve(window)?` **then** `link.require_shell()?`, in
   that order so a non-shell learns nothing about which window ids exist. Note
   that `require_shell` is itself still only as strong as
   `TD-C-ANY-CLIENT-CAN-READ-EVERY-WINDOW-TITLE` leaves it — the check is in the
   right place, but what it checks is a self-declared role, not a
   kernel-attested capability. Closing that closes this too; there is no second
   gate to add here.
4. **[ ] blocked.** Have the shell make the reservation for its taskbar — which
   requires the shell to be a compositor client, i.e.
   `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR` first. Until then steps 1–3 are
   tested but unreachable in a running system: the shell knows the number
   (`Shell::taskbar_height`, `gui/desktop/src/lib.rs`, default 40 logical px)
   and the compositor now knows the request, and nothing yet joins them up.

**Related but not the same:** maximizing had the identical problem through the
identical method, and so did the "restore rectangle" bookkeeping that remembers
where a window was before it was tiled. Both are fixed by the same subtraction;
there was no second place to patch. `re_tiling_for_a_reservation_keeps_the_
restore_rectangle` is the test that holds the restore-rectangle half of that
down.

**Also still under the bar: `set_fullscreen`.** See
`TD-C-FULLSCREEN-IGNORES-WHICH-MONITOR-AND-WHAT-IS-RESERVED` — it does not go
through `work_area_for` at all, and *should* ignore reservations (fullscreen
covers the taskbar on purpose) but should not ignore which monitor it is on.

**If never fixed** (i.e. if step 4 never lands): a cosmetic-but-constant
annoyance — the bottom ~40 px of every tiled window is obscured — and no data
loss or crash. It does not get worse with time. It *will* look like a regression
the day the shell first draws on a real screen next to the compositor, because
today nothing composites the two together and so nobody has seen it happen.

**Proved, not assumed.** 21 defects were reintroduced one at a time into the
finished code (the guarded reversible patcher, not `git checkout`) and each
produced a deterministic failure naming it back; every one of the 31 new tests
is named by at least one of them. The full map is in the sweep, but the ones
worth knowing about:

| Defect put back | Named by |
|---|---|
| nothing subtracts the strip (the original bug) | 9 tests |
| a hidden panel keeps its strip | `a_hidden_panel_reserves_nothing` |
| a panel reserves out of every monitor *but* its own | 9 tests |
| zero is a 1 px minimum rather than a release | `releasing_a_reservation_gives_the_strip_back` |
| the re-tile guard inverted | 2 tests |
| the re-tile forgets which monitor changed | `a_panel_reserves_only_out_of_its_own_monitor` |
| the re-tile overwrites the restore rectangle | 2 tests |
| claims wrap instead of saturating | `a_sum_that_overflows_clamps_rather_than_wrapping_back_to_nothing` |
| nothing clamps a greedy claim | 3 tests |
| a missing window panics instead of erroring | `reserving_against_a_window_that_is_gone_is_an_error_and_not_a_panic` |
| the thickness written narrower than it is read | 3 tests |
| the reply's origin written unsigned | `a_reserve_reply_carries_a_signed_origin_so_a_left_hand_monitor_survives` |

One test has **no** single-string defect that names it —
`destroying_a_panel_releases_its_reservation`. That is not a coverage gap but a
consequence of the design: the claim lives in `Window::reserved_edge`, so
destroying the window destroys the claim structurally, and no one-line edit can
undo that. What it guards against is a *future refactor* to a side table
(`HashMap<WindowId, _>`), which would leak a permanent strip whenever a panel
crashed. Keep the test; it is the reason not to make that refactor.
