## TD-C-THE-FEATURE-INVENTORY-WAS-WRONG-IN-BOTH-DIRECTIONS -- METHOD 2026-09-16

**In short:** fifteen items in `roadmap-detailed.md` were checked against the
code in one evening. Six were built and marked unstarted, two contradicted the
design while looking finished, one was built and unreachable, and four were
partly done with no sign of which part. Five were genuinely unstarted. **An
unticked box in that file carried almost no information**, and the errors ran
in both directions, which is what made them expensive.

**Date:** 2026-09-16. **Lane:** C.

| what the box said | what the code said | count |
|---|---|---|
| unstarted | built and working | 6 |
| unstarted | built, unreachable by any user | 1 |
| unstarted | partly built, no record of which part | 4 |
| unstarted | genuinely unstarted | 5 |
| (not flagged at all) | **built and contradicting the design** | 2 |

**The two that contradicted the design are the ones to remember.** The file
list auto-selected columns from a folder's contents, which §4.1 forbids in bold
with four reasons; and a folder's Size cell was blank, justified in a comment
as "what every file manager does" -- which is the convention §4.1 considered
and rejected. Neither showed up as a missing feature, because nothing is
missing. They were *finished work pointing the wrong way*, and no checkbox
state can express that.

**What made the check cheap.** Look for the type and the entry point, not the
word. **Four cases this evening, and the count pointed the wrong way in every
one:**

| word | hits | what they actually were |
|---|---|---|
| `tab` in `apps/editor` | 260 | tab characters and indentation; the feature was real but the count proved nothing |
| `priority` in `apps/procexplorer` | 9 | all display — no setter exists |
| `history` in `apps/terminal` | 5 | the scrollback buffer; input history does not exist |
| `dmi` in `apps/` | several | matched inside "admin" |

The pattern is not that counting is imprecise. It is that **a word appears in a
file because the domain is adjacent**, which is exactly the situation where the
answer is least obvious and the count most tempting. Grepping "tab" in `apps/editor` returns 260 hits, nearly all tab
characters; the answer came from finding `Tabs<Document>` and a `render_tabs`
that the frame calls. A count measures vocabulary, not behaviour -- the same
error that matched `dmi` inside "admin" earlier the same day.

**And what to check after finding the code:** whether anything reaches it. Four
of today's fifteen had working code behind no caller, no menu row and no key.
`[x]` on those would be the fabrication design-decisions 856 is about, moved
into the planning file.

**A third failure of the checkbox, found later the same evening: the compound
bullet.** `roadmap-detailed.md` §4.3 has "Pause, resume, kill, change priority,
restart" on one line. That is five requirements in three states — kill is real
and reachable, pause and resume are present, changing priority is not
implemented anywhere reachable, and restart does not exist. A checkbox can only
report the weakest of the five, so the bullet sat unticked while most of it was
built, and a reader learned nothing about the four that work.

The same shape appears in §4.1's metadata labels (three toggles, a date-field
choice, a per-folder scope and a line-count setting — one box) and §4.4's whole
applications (a program of five thousand lines behind a single line of spec).
**Where a bullet lists capabilities, the honest state is a sentence, not a
mark** — which is why several items now carry one.

**Recommendation for the next sweep:** record "checked and absent" explicitly,
because after this the empty box no longer implies it. Five of the fifteen now
say so, and that is the only way the next reader can tell a searched shelf from
an unsearched one.
