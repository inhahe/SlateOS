### TD-C-WEATHER-HAS-A-REFRESH-INTERVAL-AND-NOTHING-TO-REFRESH — 2026-09-04 — OPEN (fixed on lane E 2026-10-09; close once on `main` and boot-tested)

**In short.** The weather app's settings offer an update interval — "every 30
minutes" by default, adjustable from 5 to 120 — and nothing is behind it. There
is no weather source at all: every temperature, forecast and alert in the app is
sample data compiled into the binary. The setting is a control wired to nothing,
displayed as though it worked.

**Why the app does not ask for a clock.** `tick_interval` returns `None`, which
is deliberate and is documented at the method. Returning
`update_interval_min * 60` would wake a sleeping machine on a schedule to redraw
numbers that cannot have changed — `known-issues.md` lesson 47's cost paid for
none of its benefit. The one line to change when a source exists is written out
in the doc comment.

**What the user sees today.** A complete, correct-looking weather dashboard for
a city, which never changes and is not about the weather where they are. The
locations list is real (a user can add and remove cities); what each city
*shows* is the same fixed sample.

**Proper fix.** A source, in this order: an HTTP client against a public
forecast API (lane C would need one — `net/` is lane C but no app currently makes
an outbound request), a cache with the fetch time so a stale reading is labelled
rather than shown as current, and only then `tick_interval` returning the
setting. The interval control should be greyed out or absent until it drives
something.

**Not urgent, and it does not get worse with time.** Nothing is lost or
corrupted; the app is a picture of a weather app. The reason to write it down is
that the settings screen actively asserts otherwise, and a future reader could
spend a while looking for the fetch that the interval configures.

**Update 2026-09-25 (lane E): the settings screen no longer asserts it.** The
interval row is gone from the settings view, as the proper fix above says it
should be until it drives something; `Settings::update_interval_min` and
`set_update_interval` stay for the source to use. What remains open is the
source itself. (The sample weather the entry describes had already been
removed on 2026-09-15; the app opens on a notice that it cannot fetch.)
*Later the same day:* the field and its setter are gone too -- the pre-push
write-only-fields gate refused them, written, clamped and tested and read by
nothing but the tests. The interval comes back with the source.

**Update 2026-10-09 (lane E): there is a source, and the clock drives it.**
The operator's answer to E-Q2 (design-decisions §1236) put Open-Meteo behind
a switch that is off until the user turns it on; `apps/weather/src/source.rs`
makes the requests. `tick_interval` now returns the time to the next request
the window makes on its own -- `source::REFRESH`, half an hour after the
forecast shown arrived, or `source::RETRY`, five minutes after one failed --
500 ms while an answer is out (the waker brings it; the clock is the fallback),
and still `None` with forecasts off or nothing asked. The interval is a fixed
half hour -- the old setting's default -- rather than a setting: nobody has
asked for another, and every request is one more time the user's places go
out in plain text. If someone does ask, the setting goes back on the Settings
tab and into `next_ask`. R asks at once.
