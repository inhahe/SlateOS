## 1428. The feature list is re-checked one section at a time, as work is picked from it, and every check is dated

**Date:** 2026-09-27 &middot; **Decided by:** Operator (Claude's recommendation, B) &middot; **Lane:** C, for every lane

**In short:** About half the items checked in `roadmap-detailed.md` were wrong,
mostly saying "not built" about built things. It is not re-checked wholesale.
Whoever picks work from a section first checks that section against the code,
and each checked item records that it was checked and on what date -- so an
empty box can be told from an unexamined one, and the list converges instead of
being re-checked forever.

**The question:** `open-questions.md` C-Q23 (now resolved). **Verbatim:** "B".

**The convention:** a checked item carries `(checked YYYY-MM-DD)` beside its
status flag; an item whose code contradicts the design is flagged as such, not
merely marked done. See `roadmap-detailed.md`'s header.
