### TD-C-FINANCE-IS-A-VIEWER-OVER-SAMPLE-DATA — 2026-09-04 — FIXED 2026-09-25

**Fixed 2026-09-25 (lane E), all three gaps in one change, as this entry
said they had to be.** Forms enter and change accounts, transactions and
budgets; the ledger is kept as it changes, in a text file under the settings
directory (`design-decisions.md` §1202 says why text and not YAML); and
`current_date` is the clock's, with the view following midnight. The sample
data stays test-only -- a first run opens on an empty dashboard that says
why it is empty, where things will be kept, and has the one button that
begins. The rest of this entry is the history.

**In short.** The finance app now opens a window and responds to the keyboard,
but there is no way to *put anything into it*: no "new transaction", no "new
account", no way to change a budget, and nothing is saved when it closes. Every
figure on screen comes from a fixed block of made-up May 2026 data compiled into
the program. It reads like a working budget tracker and is in fact a picture of
one.

**Three separate gaps, which have to be closed together.**

| gap | what it looks like now |
|---|---|
| no creation UI | `add_account` and `add_transaction` exist and work; the only caller is `create_sample_data`. The keyboard has no binding that reaches them. |
| no persistence | Not a single `fs::` call in the crate. Close the window and any edit — a deletion, a budget change — is gone. |
| no clock | `current_date` was `SimpleDate::new(2026, 5, 18)`, a constant. |

They are one job because fixing any one alone makes the app worse. A real clock
without real data opens the app on an empty September while the sample
transactions sit in May, so the dashboard reads all zeroes. Persistence without
a creation UI saves a file nobody can change. A creation UI without persistence
invites the user to type in their finances and then throws them away.

**What was done on 2026-09-04, and what was deliberately not.** The app was
wired to the compositor and its defects fixed (below). `current_date` was left
as a constant, and `Home` now returns the view to the month containing it, so
the field is at least read. Wiring `SystemTime::now()` was *not* done: it is one
line, and it would have shipped an app whose every screen reads zero.

**The defects the wiring exposed**, all fixed in the same commit:

- **The selection was an index into a `Vec` that `remove()` is called on.**
  `CLAUDE.md` names this one directly — store stable identifiers, not positions
  into a container that moves. Every deletion silently re-pointed the selection
  at whatever slid into the gap.
- **Arrow keys walked rows the user could not see.** Navigation stepped through
  all transactions by index while the screen listed only the filtered ones, so
  with a filter or a search active the highlight vanished for several presses —
  and Ctrl+D then deleted an invisible row.
- **Ctrl+D on a fresh window deleted the first transaction**, which the user had
  never pointed at. Nothing is deleted without a selection now.
- **A budget limit of zero divided by zero** on the dashboard, producing `inf`,
  which draws as a bar past the end of its track and a permanently red category.
  The budgets screen returned `0.0` for the same input: two copies of one
  division that disagreed. Now one `usage_ratio`.
- Money arithmetic (`income - expenses`, `initial + tx_sum`, `-amount`) was
  unchecked; it saturates now, because a wrap turns a surplus into a deficit.
- `prev_month` at January of year 0 wrapped to year 65535.

**Proper fix for the entry itself.** In order: a transaction/account/budget
editor, then a YAML store under the user's config directory written through
`safeio` (the archive manager and the installer both already route their writes
that way), then `SystemTime::now()` for `current_date` — and the sample data
becomes what a *first run* seeds, or is dropped entirely.

**Why it is not urgent.** Nothing here loses user data, because there is no user
data. It is a demo that a user may mistake for an application, and the cost of
that mistake is disappointment rather than damage.
