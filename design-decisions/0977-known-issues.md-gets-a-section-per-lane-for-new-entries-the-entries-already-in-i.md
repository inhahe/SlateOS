## 977. `known-issues.md` gets a section per lane for new entries; the entries already in it stay where they are

**Date:** 2026-09-27 · **Decided by:** Operator (Claude recommended this option) · **Lane:** A

Answering A-Q18. Relayed by lane F, 2026-09-27; the answer was one word, "A".

**In short:** every agent added new bug entries at the bottom of one shared
file, so any two that wrote between merges collided there. It happened eleven
times in one day. The operator chose per-lane sections: each lane adds new
entries at the end of its own section, so two lanes never write the same
lines.

**What it obliges.**
1. **Six sections at the end of `known-issues.md`**, `## Lane A: new entries`
   through `## Lane F: new entries`. A lane appends at the end of its own
   section. There are six, not the three the question named, because the tree
   has had six lanes since 2026-09-22.
2. **The existing entries do not move.** Moving them would conflict with every
   lane at once and would need a halt. Left where they are, they cause no
   conflicts, because nobody appends there any more. An amendment still goes
   directly under the entry it amends, wherever that is.
3. **roadmap.md's shared-document row for `known-issues.md` says so**, replacing
   "new entries go at the end". `check-known-issues-index` walks the headings,
   and is re-run to prove it still does.
4. **Every lane is told directly**, since the row they follow today says the
   opposite.
