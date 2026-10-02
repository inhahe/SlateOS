## 817. The timezone database is packaged by lane B, and the lane map is corrected to say where the package manager actually lives

**Date:** 2026-09-07
**Lane:** C
**Decided by:** Operator (Claude recommended B)

**In short:** set your clock to New York and SlateOS quietly gives you UTC,
because the world's timezone rules were never packaged. Nobody had written
them because the document saying who owns that job pointed at a directory that
does not exist. Lane B, which already has the package manager, does the work,
and the map is corrected in the same change so the next reader is not sent to
the same empty directory.

**The question.** `open-questions.md` -> C-Q8. Four options: move the package
manager to a top-level `pkg/` and have lane C write it; give the job to lane
B; correct the map to point at `userspace/pkg/` and leave ownership with lane
C; or leave it and write down that the clock lies.

**The answer: B**, which was also the recommendation. The package manager is
already lane B's and already exists; moving 5,004 lines across a lane boundary
to make an ownership document true is the tail wagging the dog.

**The correction travels with it.** The map's error is the actual cause of the
stall -- not a missing decision but a document that named a path nobody could
find. Fixing the ownership without fixing the map would leave the trap set for
the next question.
