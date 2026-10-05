## C-A-SPEED-TEST-HISTORY-DRAWN-UPSIDE-DOWN-RELATIVE-TO-ITS-HIT-TEST (lane C, 2026-08-20) — FIXED

The audit the entry above asked for, carried out. Two of the nine candidates
had real defects; five turned out not to be candidates at all.

**`apps/speedtest` — the list was mirrored.** `render_history_panel` drew the
results newest-first (`.iter().rev()`) and highlighted the row whose *history*
index matched `history_hover`; both hit tests (`handle_click` and
`handle_mouse_move`) computed the **draw position** and stored that. So
pointing at the top row highlighted the bottom one, pointing at the second
highlighted the second-from-last, and only the middle row of an odd-length
list lit itself — which is why a spot check could miss it. Fault family 3 from
the entry above, in its purest form: the renderer applied a transformation the
hit test did not.

Two more in the same panel:

- **The hit test ran under the stats strip.** The avg/best/worst strip is
  opaque and painted over the bottom 24 px of the list, but the hit test's
  bound was the panel's full height. Pointing at "avg 84.2 Mbps" selected a
  result painted underneath it. Fault family 3's clip variant, the same as the
  notification centre's.
- **The history could not be scrolled at all.** `history_scroll` was read in
  three places and written by nothing — there was no wheel handler. With
  `MAX_HISTORY` of 20 rows of 26 px in a 206 px viewport, **twelve of the
  twenty results were drawn nowhere and reachable by nothing.** Fault family 2,
  in its terminal form: not "clamped at one end" but "never moved at all".

Fixed the way the four panes were: one `history_rows()` walk yielding
`(screen y, history index)` with the scroll already applied, which the
renderer, both hit tests and the scroll bound all read; a
`history_viewport_height()` that subtracts the stats strip and feeds the clip,
the bound and the hit test alike; `max_history_scroll()` derived from measured
content; and a wheel arm using `wheel::pixels` (the offset is an `f32`, so no
`Accumulator`). 66 → 80 tests.

**`apps/compass` — dead scroll state with a divergence already built in.** The
waypoint list carried a `wp_scroll: usize` that `handle_mouse` **added to its
row index** and `render_waypoint_view` **ignored outright**. It was never
written, so it sat at zero and never bit — but it is exactly the shape that
made the notification shade hit-test 76 px from where it painted, pre-loaded
and waiting for whoever wired scrolling up. The two also disagreed about the
row's width: 36..864 painted, 40..860 clicked, so a 4 px strip down each side
of every row looked selectable and was not.

Removed rather than wired up: `MAX_WAYPOINTS` is 10 and sixteen rows fit above
the status bar, so this list provably cannot overflow and scrolling it would
be dead code with dead tests. The assumption that makes that safe is now a
tripwire — `every_waypoint_the_app_will_hold_fits_on_screen` fails if the cap
is ever raised past what fits, so whoever raises it is made to add real
scrolling instead of finding out from a user. 111 → 116 tests.

**Correction to the candidate list above: five of those nine were not
candidates.** `apps/jsonviewer`, `apps/logviewer`, `apps/regextester`,
`apps/renamer` and `apps/snippets` contain **no mouse handling whatsoever** —
zero occurrences of `MouseEventKind` between them. The line numbers cited were
renderer arithmetic, not hit tests, so no divergence is possible: there is
nothing on the other side to diverge from. They have a different problem, and
it is the one already tracked as `C-NO-APP-IS-WIRED-TO-AN-EVENT-LOOP`.

Worth recording separately, because it is a distinct smell from the ones
above: **several of them carry a `scroll_offset` field that nothing ever
writes.** `apps/logviewer/src/main.rs:714` is the clearest — the field exists,
`render_log_list` faithfully honours it, and its value is `0.0` forever. The
renderer looks like it supports scrolling and the state looks like it tracks
it; only the absence of a writer gives it away. When those apps are wired to
an event loop, each such field is a place where a wheel handler will be added
without anyone re-deriving the bound or the hit test — so check for the three
fault families at that point rather than assuming the plumbing is sound
because the renderer reads the offset.

Both fixes are pinned by `scripts/reintro-list-hit-tests.py`, which puts all
eight defects back one at a time and asserts the suite goes red for each.
