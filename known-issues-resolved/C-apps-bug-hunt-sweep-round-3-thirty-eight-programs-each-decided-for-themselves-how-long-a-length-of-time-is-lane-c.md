## apps/** bug-hunt sweep, round 3: thirty-eight programs each decided for themselves how long a length of time is (lane C)

**Filed:** 2026-08-21 by Lane C.
**Status:** FIXED — `textfmt::duration` (7 shapes) plus a 31-call-site migration
across 29 files, in six commits `e1a723a91`..`ed8faae31`.
See design-decisions.md §490 for the shape policy this settles.

### The defect, and how it differs from round 2

Round 2's defect had **one** right answer: dividing by 1024 and printing `MB` is
false under any policy. Durations do not work that way. `01:05`, `1m 5s` and
`just now` are all honest renderings of a length of time, and picking between
them is a real editorial choice. **That legitimacy is exactly what let the
disagreement go unnoticed for 38 functions** — every reviewer looking at any one
of them saw a defensible formatter.

So the audit could not ask "which of these is wrong?". It asked two narrower
questions:

1. Do two formatters that have chosen the *same* shape disagree about it?
2. Do two surfaces that display the **same object** render it differently?

Question 2 found the serious ones. Four times, two windows a user can have open
simultaneously rendered one underlying number two ways:

| The one number | Surface A | Surface B |
|---|---|---|
| a recording's length (`duration_ms` **is assigned from** `elapsed_ms`) | overlay: `01:01:01` | list, a moment later: `61:01` |
| the machine's uptime | `procexplorer`: `3d 4h 12m 30s` | `sysmonitor`: `3d 4h 12m` |
| the battery's remaining time | `power` (tray) | `power_settings` (page) |
| one clipboard's entries | `clipmanager`: `10s ago` | `clipboard_viewer`: `just now` |
| one notification queue | `notif_pane` | `gui/notifications` |

### Why the tests did not catch it

`screen_capture.rs` is the clearest case and worth recording in full, because
the same structure recurs. Both methods had a passing test.
`test_stats_elapsed_display` used 3 661 000 ms; `test_entry_duration_display`
used 125 000 ms. Under an hour the two functions agree, so **neither test ever
evaluated the other's input**, and each proved its own copy correct. The bug
lived in the gap between two green tests.

That is this round's lesson, stated generally: *the defect is not duplication.
It is duplication whose copies are each locally proven.* A test that pins what
one copy does is not evidence that the copy is right — `musicplayer` asserted
`format_time(3661.0) == "61:01"`, recording a missing hour rollover as intended
behaviour.

### The individual bugs found

| Where | Bug |
|---|---|
| `gui/desktop/screen_capture.rs` | no hour branch in `duration_display` — 61 minutes read `61:01` |
| `apps/musicplayer:2480` | same, and it also formats the *playlist total*, which routinely exceeds an hour |
| `apps/benchmark:720` | same, in `elapsed_display` |
| `apps/camera:1039` | minutes digit unpadded (`1:05`) against the screen recorder's `01:05`, for the same kind of object |
| `apps/taskscheduler:2578` | divided into an `f64` and printed one decimal, so 59 960 ms reported **`60.0s`** — a string its own under-a-minute branch excludes |
| `apps/automator`, `apps/taskscheduler`, `startup_settings` | no hours field: a 90-minute automation read `90m 0s`; a five-minute startup delay, configurable *on that very screen*, read `300.0s` |
| `apps/vpnmanager:2958` | no days field, so a four-day tunnel read `100h 0m 0s` |
| `apps/pomodoro:442` | `0m 00s` for a zero span |
| `procexplorer` / `sysmonitor` | the uptime disagreement above |

### The fix

One module, `textfmt/src/duration.rs` (`no_std`, no dependencies), re-exported
by `guitk` as `guitk::duration` for the apps that reach the toolkit and depended
on directly by the headless ones. Seven public shapes, tabulated in the module
doc by *what the reader is doing*, so a caller picks a shape rather than writing
one. 83 unit tests + 22 doctests.

The one genuinely new idea is the split between `coarse` (bottom unit: seconds)
and `coarse_minutes` (bottom unit: minutes). Both render "the two most
significant components", and the choice between them records **whether the
number was measured or estimated**: printing `1m 30s` for a battery estimate
invents precision nobody has, while dropping `30s` from a measured 90-second
backup discards a measurement that was taken. A test asserts the two agree
across 2 000 000 seconds above the hour, so the module cannot reintroduce its
own founding defect internally.

### What did not change

Seven formatters deliberately stay hand-written:

- `apps/backup:1153`, `apps/taskscheduler:2561` — these render *timestamps*, not
  spans. Different problem.
- `apps/worldclock:466` — a timezone offset is a signed displacement between two
  clocks, not a length.
- `apps/calendar:601` — event lengths are natively in **minutes**. Routing them
  through a seconds API would turn a one-hour meeting from `1h` into `1h 0m`.
  Not every duration formatter belongs to a shared seconds module.
- `apps/netscan:2196`, `apps/videoplayer:2031`, `:2038` — seek and scan
  arithmetic the scanner mistook for formatting.

Two **sentinels** also stay at their call sites, because they are not durations:
`notif_pane`'s `"now"` (the pane has no clock reading yet — a fact about the
pane) and `startup_settings`' `"Immediate"` (a word for *no delay configured*,
not a rendering of zero). `gui/notifications` had a third, a `"now"` for
*future* timestamps, and that one was **removed**: a future timestamp is an age
of zero, which `relative` already renders as `just now`.

### Related

`textfmt::duration` is the fourth primitive this sweep has extracted for one
recurring reason, after `guitk::grid::columns_across` (round 1, finding 7),
`textfmt::bytes` (round 2) and the `tzrules::civil_from_days` request still open
with lane B. The round-2 note asked whether an audit for *any* small computation
restated more than three times was worth generalising; three rounds now say yes,
and the discriminator that makes such an audit tractable is question 2 above —
**start from objects displayed on more than one surface**, not from functions
that look alike.
