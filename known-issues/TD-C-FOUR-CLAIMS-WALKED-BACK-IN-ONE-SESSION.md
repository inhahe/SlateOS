## TD-C-FOUR-CLAIMS-WALKED-BACK-IN-ONE-SESSION -- METHOD 2026-09-16

**In short:** four times in one evening I wrote a confident, specific statement
about this codebase into a file, and four times it was wrong and had to be
corrected within the hour. Three I caught by reading further. One was caught by
a test. The difference between those two is the whole entry.

**Date:** 2026-09-16. **Lane:** C.

| the claim | what was actually true |
|---|---|
| "the thumbnail cache has no eviction" | `purge_stale` exists, is careful and is tested — it simply has no caller |
| "crash recovery needs only a start-up scan" | the journal holds a plan *id* and finished indices, not the plan; it is a format gap |
| "the editor's status bar shows neither encoding nor line ending" | the line ending is drawn, a few lines below where I stopped reading |
| "two distinct volumes could share a device id" | what is hashed is the volume *prefix*, which is ASCII in every real case |

**The common cause is one habit**: describing a thing after reading part of it.
Each claim came from a grep hit or the first screen of a function, generalised
into a sentence that sounded like the result of an investigation. A vague
sentence would have been harmless. **A specific wrong sentence is expensive
precisely because it is actionable** — "add a start-up scan" sends the next
person to write a scan, find journals, and discover there is nothing to do with
them.

**The remedy that worked, found by accident.** The fourth claim went into a
*test* as well as a comment, and the test failed immediately. Writing the
assertion forced the claim to be checkable, and checkable claims get checked by
the machine rather than by the next reader's goodwill. The three that stood
longest were the three that lived only in prose.

So: **when a finding is worth writing down in a file, it is worth writing as an
assertion first if it can be one.** Not everything can — "nothing calls this"
is awkward to assert — but "these two inputs must not collide" and "this field
holds X" usually can, and those are exactly the claims that read as authority
later.

**A second, smaller rule from the same four:** a doc comment that lists what a
module does is a claim about *every* item on the list. Three of today's
corrections were to such lists, and in each case the list was right about most
of its entries and wrong about one, which is the hardest shape to notice.
