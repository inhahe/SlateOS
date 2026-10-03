## [C] Where should the weather app get weather, once it can fetch at all? — deferred 2026-09-18 — PROMOTED 2026-09-26 to `open-questions.md` E-Q2

*Its trigger fired: applications make outbound connections now
(`apps/netscan`, `apps/dictionary`, and `userspace/pkg` before them). Lane E,
which owns `apps/**` since the six-lane split, raised it as E-Q2 with the
option table below and a recommendation. Kept here for its history; answer
it there.*

**In short:** Our weather app has no weather and no way to get any, and it
says so plainly in its own window rather than showing invented numbers. So
there is nothing wrong with it today and nothing for you to decide. When we
can actually make a network request, someone has to choose where the forecast
comes from — and that is a real choice, because a weather provider learns
where our users are.

**Why this is not in `open-questions.md`.** I raised it there first and
withdrew it the same day. The app already behaves correctly while empty
(`CANNOT_FETCH_LINES`, and it stops before drawing the dashboard), so there is
no harm accruing, and the decision cannot be acted on: `net/httpclient` builds
and parses HTTP and has no transport — its own module doc says "What this
crate does *not* do: send anything". Asking now would be asking you to choose
a supplier for a delivery we cannot receive.

**The choice, when it arrives:**

| Option | *What changes* |
|---|---|
| A public no-key API (Open-Meteo and similar) | *Forecasts work with no signup; the provider sees our users' coordinates.* |
| A keyed commercial API | *Same, plus a key to ship, store and rotate.* |
| User-supplied endpoint, nothing by default | *It stays empty until someone configures it; no third party by default.* |
| Never fetch; delete the app | *One fewer app, no privacy surface.* |

**Trigger to promote this into `open-questions.md`:** the first working
outbound connection in the net stack — a real `connect`/`send` path that
`net/httpclient` can sit on. Until then this is blocked on the project, not on
the operator.

**Related:** `known-issues.md` →
`TD-C-WEATHER-CAN-ONLY-EVER-BE-EMPTY` (withdrawn; explains why the app is
already correct).
