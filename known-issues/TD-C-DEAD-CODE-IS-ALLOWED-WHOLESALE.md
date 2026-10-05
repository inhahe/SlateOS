### TD-C-DEAD-CODE-IS-ALLOWED-WHOLESALE. 120 lane-C files switch off the warning that finds orphaned code, and one of them was hiding a dead calendar (lane C, 2026-08-21)

**In short:** Rust warns you when a function exists that nothing calls. 120
files in lane C's tree begin with a line that turns that warning off for the
whole file. So when the round-4 migration made several private helpers
unreachable, the compiler said nothing about most of them. The one crate that
*had not* switched the warning off reported its orphan immediately, which is
how the pattern was noticed at all.

**Where:** `#![allow(dead_code)]` at the top of 120 files under `apps/`,
`gui/`, `net*/` and `pkg/` (148 files carry an `allow(dead_code)` in some
form). Enumerate with:

```
grep -rn '^#!\[allow(dead_code' apps/ gui/ net/ pkg/ --include=*.rs
```

**How it bit.** Round 4 moved nine programs' timestamp rendering onto
`guitk::datetime`. That orphaned three private calendars:

| Crate | Orphaned | Did the compiler say so? |
|---|---|---|
| `apps/archivemanager` | `days_to_ymd` | **Yes** — no file-level allow. Deleted the same minute. |
| `apps/rssreader` | `days_to_ymd` | No — `#![allow(dead_code)]` at line 24. Found by hand. |
| `apps/taskscheduler` | `days_to_ymd` | No — its last caller was inside the same file, so it was still live until `decompose_timestamp` was rewritten. |

A dead private calendar is not merely clutter. It is a second answer sitting
in the file, correct-looking and untested against anything, waiting to be
picked up by the next person who needs a date here — which is the exact
failure round 4 exists to undo.

**Why the allows are there.** Almost all of them are original-authorship
convenience: an app was written with a full set of helpers before the UI that
calls them existed, and the allow silenced the noise. That reason expires once
the app is written; the line does not.

**The proper fix:** delete the file-level allow from each crate and either use
or delete what falls out. This is per-crate work with no shared blast radius —
each file can be done independently and gated on that crate's own tests — so
it is a good background task rather than one large change. Where a helper is
genuinely a deliberate public-shaped API that nothing yet calls, narrow the
allow to that item with a comment naming the caller it is waiting for, rather
than leaving it file-wide.

**If never fixed:** the tree keeps accumulating unreferenced code that no
tooling reports, and every future consolidation like round 4 has to find its
own orphans by hand.

**Progress 2026-08-21 — `gui/desktop` cleared, and the prediction above was an
understatement.** Making the shell a library removed all **54** module-level
`#[allow(dead_code)]` in `gui/desktop`. The crate had been reporting zero
warnings for its entire life; it immediately reported **119**.

The entry above argued that a file-wide allow hides orphaned code. What the
119 actually contained is worse than orphans:

- **115 were dead palette constants** — and pulling on them exposed
  `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`: 549
  surviving constants, 33 distinct values, an entire settings page that the
  desktop ignores. The unused-constant warning was the *only* signal that
  duplication existed, and it had been suppressed 54 times.
- **Three were live defects, not dead code:**
  - `RememberedDecision::recorded_at_ms` was written and never read — a
    security prompt answered "allow, remember this" was remembered for the
    whole session with no expiry and no revocation UI. Fixed here (grants
    expire after 8h, denials never; see `design-decisions.md` §496).
  - `BannerStyle::description` had no caller — the notification settings page
    offered a choice of banner styles and never showed what any of them did.
    Fixed by rendering it as a hint line.
  - `calendar::days_in_month` was a private identity wrapper over
    `date::days_in_month`, justified by a doc comment claiming the toolkit
    took `(month, year)`. `gui/toolkit/src/date.rs:402` takes `(year, month)`.
    The wrapper was a second answer defended by a false statement about the
    first. Deleted; 20 call sites requalified.

**The lesson for the remaining ~66 files:** treat the warning burst as a
*defect report*, not a cleanup list. The instinct on seeing 119 unused items
is to delete all 119; three of these would have been deleted along with the
bug they were evidence of. Read each one for *why* nothing calls it before
deciding that nothing should.
