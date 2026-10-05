## §674 — the timestamp floor stays where it is and the fixture moves to meet it, because a floor lowered to admit a fixture stops detecting the thing it exists to detect

**Date:** 2026-09-02
**Lane:** A
**Decided by:** Claude (autonomous)

**In short:** The conformance harness refuses to believe any file claims to
have been modified before 1970-04-26, on the grounds that a timestamp that
early almost certainly means the driver returned the number in the wrong
*unit* — seconds where nanoseconds were wanted, say. The btrfs test fixture
declared its files as being from 1970-01-01, fifteen minutes after the epoch,
and so every btrfs object failed all four of the harness's timestamp clauses —
24 broken clauses on a driver that was doing nothing wrong. The choice was to
lower the floor so the fixture fits, or to fix the fixture. The fixture was
fixed, and the harness's own comment — which had flatly asserted that no
fixture predates the floor — was corrected to state the obligation instead.

### The defect, on both sides

`TS_NS_FLOOR = 10_000_000_000_000_000` (1e16 ns = 1970-04-26) is not chosen as
a date. It is chosen because it sits above every wrong *unit* a backend could
plausibly return for a present-day time, and below the right one:

| what the driver returns | value for "now" | verdict |
|---|---|---|
| seconds | ~1.8e9 | caught |
| milliseconds | ~1.8e12 | caught |
| microseconds | ~1.8e15 | caught |
| **nanoseconds** | ~1.8e18 | **passes** |

The btrfs fixture used `1_000`/`2_000`/`3_000`/`4_000` seconds, picked only so
the four timestamps would be *distinct from one another*. Distinctness is what
the fixture's authors needed; being in the present was not something they knew
they owed anyone. So the harness was right, the driver was right, and the
fixture was a 1970 file — precisely the thing the floor exists to catch, caught.

The second half is the more interesting one. The comment above the constant
asserted, flatly, that no file in any fixture or on any real disk predates it.
A fixture in this very tree falsified it. That is a note about a rule, believed
instead of the rule — the same shape as `A-GATES-SILENTLY-STOPPED-CHECKING`,
as the three stale serializer comments in the 647/664 exchange, and as lane B's
*"a gate that reports a fact it has not checked costs more than the finding
does."*

### The decision

1. **The fixture moves.** Constants become 2024-01-01 plus one day each
   (`1_704_067_201` … `1_704_326_404`), preserving distinctness — every
   consumer derives from these, so no assertion needed editing.
2. **The comment is demoted from a claim to an obligation.** It now says a
   fixture *should* not predate the floor and records that one did, with the
   rule stated as a requirement on fixtures: **declare present-day dates, not
   merely distinct ones.** Writing a fixture below the line does not exercise
   the check, it fails it.

### Why not the alternatives

- **Lower `TS_NS_FLOOR` to admit the fixture.** Rejected, and this is the whole
  entry. The floor's value *is* its function: drop it below ~1.8e9 and the
  seconds-for-nanoseconds case — much the likeliest unit error, and the one a
  Unix-shaped API invites — stops being detected at all. Lowering it would have
  turned 24 loud failures into 24 silent passes and left the harness looking
  healthier while checking strictly less. The failing clauses were the harness
  working.
- **Exempt fixture-backed objects from the timestamp clauses.** Rejected. It is
  the same skip-that-reads-as-coverage that §673 rejected one entry ago, and it
  would exempt exactly the backends whose timestamps are easiest to get wrong,
  since a fixture is where a hand-written number enters the system.
- **Fix the fixture and leave the comment alone.** Rejected because the comment
  is load-bearing: it is what the next person writing a fixture will read
  before choosing numbers, and as written it told them the constraint was
  already satisfied by construction rather than being theirs to meet. A stale
  reassurance is worse than no comment, because it is consulted.

### If it is never revisited

Nothing degrades, but the failure mode is quiet. A future fixture written with
small round numbers reproduces this exactly: two dozen failures against a
correct driver, on a check whose message names the file rather than the
fixture. The corrected comment is the only thing standing between that and a
second afternoon spent on it.
