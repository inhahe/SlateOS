## TD-C-FULLSCREEN-IGNORES-WHICH-MONITOR-AND-WHAT-IS-RESERVED (lane C, 2026-08-21) — RESOLVED 2026-08-21

**Resolution.** Already fixed by `c85729f62` (design-decisions.md §514), which
landed between this entry being written and being picked up — the entry below
describes code that no longer exists. `set_fullscreen` reads
`self.work_bounds_for(w.frame_rect())` and sizes from *that* (lib.rs ~5216), and
`work_bounds_for` returns `Display::bounds` — the monitor, not its work area —
which is exactly what step 1 asks for. Step 2's blocker was resolved in the same
commit and recorded in §514: the direct-scanout bypass is deliberately a
single-head optimisation, and the guard that makes it so is the pre-existing
"the window must cover the whole framebuffer" test, which a one-monitor
fullscreen window no longer satisfies. `the_direct_scanout_bypass_declines_a_second_monitor`
(lib.rs ~15366) pins that the guard is load-bearing rather than incidentally
true.

Step 3's checklist had two of its three tests —
`fullscreen_fills_the_windows_own_monitor_and_not_every_monitor` and
`leaving_fullscreen_on_the_second_monitor_stays_on_it`. The third, the contrast
against a reservation, was genuinely missing and is now
`fullscreen_covers_the_taskbar_that_maximize_stops_at`: it maximizes a window
over a 40-row bottom reservation and asserts it stops at `screen.bottom() - 40`,
then fullscreens the *same* window and asserts it covers the whole 800x600. The
gap mattered — with nothing reserved the two helpers return the same rectangle,
so swapping `work_bounds_for` for `work_area_for` in `set_fullscreen` passed all
427 other tests while leaving a strip of taskbar across every full-screen video.
Proved by the reintro marker `fullscreenspareasthetaskbar`, which fails that one
test and no other. Compositor 428 + 18 green.

<details><summary>Original entry (describes pre-<code>c85729f62</code> code)</summary>


**In short:** Putting a window fullscreen always makes it fill the *first*
monitor, starting at the top-left corner of the whole desktop — no matter which
monitor the window was actually on. On a single-monitor machine you cannot tell.
On two monitors, a video you fullscreen on the right-hand screen jumps to the
left-hand one and covers whatever was there. Nothing crashes and Escape still
puts it back where it came from; it just goes to the wrong screen.

**Where.** `gui/compositor/src/lib.rs` `Compositor::set_fullscreen`:

```rust
let (fb_w, fb_h) = self.backend.size();
…
window.x = 0;
window.y = 0;
window.width = fb_w;
window.height = fb_h;
```

`backend.size()` is the framebuffer — one screen's worth — and `(0, 0)` is the
origin of the *virtual desktop*, so the two do not even describe the same
rectangle once a second monitor exists. This is the same family as
`TD-C-TILING-MEASURED-EVERY-MONITOR-AT-ONCE` (now resolved), but it was
deliberately left out of that fix: fullscreen is the one path that must **not**
go through `work_area_for`, because a fullscreen window is *supposed* to cover
the taskbar. It needs the monitor's bounds, not the monitor's work area, so it
wants a different call and got skipped rather than half-converted.

**Why it was deferred rather than fixed with the rest.** Changing what
fullscreen resolves to interacts with **direct scanout** — the optimisation
where a fullscreen window is handed to the display controller as the scanout
buffer instead of being composited. Eligibility for that is currently phrased in
terms of the window matching the framebuffer exactly, which is the very
expression this change would invalidate. Making fullscreen per-monitor without
first deciding what direct scanout means on a multi-head arrangement (scan out
on the window's own monitor only? disable it entirely when more than one head is
active?) would silently either break the optimisation or, worse, keep it enabled
while it is no longer correct. That is a decision, not a typo.

**The proper fix.**

1. Resolve the monitor the way everything else now does:
   `self.work_bounds_for(window.frame_rect())` — the monitor with the largest
   intersection, primary as the fallback — and use *those* bounds, not
   `backend.size()` and not `work_area_for` (see above).
2. Re-state direct-scanout eligibility against that monitor's bounds rather than
   against the framebuffer, and decide explicitly what it does when more than
   one head is active. Record the decision in `design-decisions.md`.
3. Add the tests the tiling fix's fixtures make easy: fullscreen on the second
   monitor stays on the second monitor; fullscreen ignores a reservation (the
   taskbar *is* covered) where maximize honours it; unfullscreen restores to the
   original rectangle on the original monitor.

**Reproduce.** With the existing `two_monitors(1)` fixture, `set_fullscreen` a
window on `screens[1]` and read its `frame_rect()`: it comes back at
`screens[0]`'s origin with the framebuffer's size.

**If never fixed:** harmless on one monitor, which is every configuration
tested today. On two it is a visible misfeature the first time anyone
fullscreens anything on the secondary screen. It does not corrupt state — the
restore rectangle is captured before the move, so leaving fullscreen still
returns the window to where it was — and it does not get worse with time.

</details>
