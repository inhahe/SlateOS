## TD-C-THE-THREE-TOOLS-THAT-REPORT-ACTS-THEY-DID-NOT-PERFORM -- FIXED 2026-09-15

**In short:** three programs told the user that something had happened to their
data when nothing had. A recovery tool reported files recovered, a partition
manager reported formats and deletions applied, and a network scanner reported
open ports on machines it never contacted. None of the three has any access to
the thing it describes.

**Date:** 2026-09-15. **Lane:** C. Ordered by what believing each one costs,
which is the axis lane B proposed and which put all three above the twenty-odd
settings pages fixed earlier the same day.

| program | what it reported | what it had |
|---|---|---|
| `apps/undelete` | files recovered, with byte counts and destination paths | no `std::fs`, no `safeio` |
| `apps/partmanager` | "Applied N operation(s) successfully" for queued formats and deletions | no `std::fs`, no `safeio` |
| `apps/netscan` | hosts up, ports open, service banners | no `std::net`, no socket syscall |

**What separates these from the rest.** Every other fabrication in this sweep
cost time or trust and left the situation recoverable. These three are acted
upon, and the action is often irreversible:

* A person told recovery **failed** keeps looking. One told it **succeeded**
  stops — and may reformat the disk, because the data is safe elsewhere.
* A person told a format was **applied** believes a drive was wiped. That is
  the belief someone acts on before selling or discarding it.
* A person told a port is **closed** concludes their network is secure. The
  tuned probabilities reported far more closed ports than open ones.

In each case the false *success* is worse than the false failure, and by a
margin that the usual "it is only a stub" reasoning does not cover.

**`netscan` was the hardest to have caught and is worth studying.** It was not
a constant list: each address had a 60% chance of being up, each port a
probability tuned by service — 50% for SSH and HTTP, 25% for RDP and SMB — and
a fabricated banner 40% of the time. **Two runs disagreed, which is exactly
what a real scan does.** Repeating it could never expose it; a constant list
would have been suspicious the second time. The randomness was what made the
fiction survive, and a plausible distribution is a stronger disguise than a
plausible value.

**The fix in all three is the same, and half of it is easy to miss.** Remove
the invented source, and report an honest failure — that part is
straightforward. The other half is lane B's: **"no results" and "cannot look"
are different sentences, and an empty list is read as the first.** An empty
partition list claims the machine has no disks; an empty recovery list claims
nothing survives; an empty scan claims the network is quiet. All three are
verdicts these programs have not earned, so all three screens now say outright
that nothing was examined.

**Two smaller inventions surfaced inside the fixes.** `undelete`'s failure
message read "Data blocks partially overwritten" — a physical cause never
established, which would send someone hunting a hardware fault. And I first
wrote `RecycleBinReader::scan` as a *clear*, which asserts the bin is empty;
it is a no-op now, because with nothing to read it learns nothing and therefore
changes nothing. The second was my own, made while fixing the first.

**`partmanager` keeps its queue after a failed apply**, deliberately. Clearing
it would leave a window indistinguishable from one where the work was done —
nothing pending, nothing to see — and the queue is what the user needs if this
ever gains the ability to apply it.

**Twenty-four, forty and nine tests were resting on the invented data.** That
is the fifth, sixth and seventh application in a row where removing the
fabrication turned tests red. Several of those tests asserted the fabricated
behaviour directly — `test_engine_recovery` required that "at least some should
succeed" — and were correct about the behaviour. The behaviour was the defect.
