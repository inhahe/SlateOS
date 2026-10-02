### [A] The heading convention `known-issues.md` documents is invisible to every triage count taken of it, and 48 of the uncounted entries were mine -- 2026-09-18
**Status:** OPEN (lane A's 25 stamped; the gate rule and lanes B/C's 8 remain)

**In short:** this file asks each entry to carry a one-line status so anyone
can count what is still broken. The counting is done by a text search that
only recognises the *older* of the two heading styles in use -- so entries
written in the style the instructions actually ask for are missed. Most of
the missed ones are mine, and none of them had the status line either.

**Three nested populations, none of them the file's contents.** Measured, not
estimated:

| what | count | what it misses |
|---|---|---|
| `grep -c '^## TD-'` -- the triage grep named in `check-known-issues-index.py`'s own docstring | 429 | 36 backticked `` ## `TD- `` headings, and all 62 `### [lane]` ones |
| the checker's own pattern, `^## (TD-[A-Z]-.*)$` | 356 | additionally 73 single-agent-era subsystem names (`TD-FONT-`, `TD-COMPOSITOR-`, `TD-KASAN-`) |
| entries in the documented current style, `### [A]` / `### [B]` / `### [C]` | 62 | seen by **neither** |

So the checker's uniqueness and uppercase-marker rules -- both written after a
firing, both correct -- run over 356 of 518 entries, and the 62 written in the
style `roadmap.md`:379 prescribes (`### [C] ...`) are outside every count.

#### Corrected the same day: 740 entries, not 518, and the entry you are reading undercounted by 222

The census above lists three populations and calls them "none of them the
file's contents". **The list of three was itself not the file's contents.**
I enumerated the shapes I thought of and reported the total as though it
were the file. Full count:

| shape | count | documented triage grep `^## TD-` | checker regex |
|---|---|---|---|
| `## TD-<letter>-` | 356 | sees | sees |
| `## TD-<SUBSYSTEM>-` (`TD-FONT-`, `TD-KASAN-`) | 73 | sees | **misses** |
| `` ## `TD- `` (backticked) | 43 | **misses** | **misses** |
| **`### TD-`** | **201** | **misses** | **misses** |
| `### [A]` / `### [B]` / `### [C]` | 67 | **misses** | **misses** |
| **total** | **740** | **429 (58%)** | **356 (48%)** |

So the triage grep every number in this project comes from sees **58%** of
the file, and the checker that enforces unique slugs and uppercase markers
covers **48%**. The 201-entry `### TD-` population is the largest single
blind spot and I had not looked for it at all.

**How it surfaced, which is the part worth keeping.** Not by re-counting. A
scanner I wrote to find real errors inside passing boots flagged
`selftest: not valid UTF-8, and this stage cannot handle arbitrary bytes yet`
as unexplained. I grepped `known-issues.md` for that message text, got
nothing, and concluded the limitation was untracked -- and was about to file
it. The helper's own doc comment says *"Tracked in `known-issues.md` ->
`TD-KSHELL-LINE-EDITOR-IS-UTF8`"*, and that entry has been at line 18907
since 2026-08-13. It is a `### TD-<SUBSYSTEM>-` heading, so it fails both
counters on both counts.

That is dd-953's *silence vs absence* row -- contributed by lane C and
written by me the same hour -- catching me inside the hour: I searched for
the message rather than the subject, got silence, and read it as absence.
And it is the duplicate direction of the immutability near-miss: not "a
working thing reported broken" but "a tracked thing reported untracked",
which would have put a second entry in a file whose whole problem is that
nobody can count it.

**What this changes about the fix.** The earlier plan -- teach the checker
the `### [lane]` convention once lanes B and C have stamped their 8 -- is
now the smaller half. `### TD-` is 3x larger than `### [lane]` and is not a
lane convention at all; it is the single-agent era's heading style, which
means it belongs to no one and nobody will volunteer for it. Any real fix
has to either normalise the five shapes or make the counter accept all of
them; the second is a one-line regex and the first is 740 edits, so the
counter should move.

Deliberately not doing that in this pass: a counter that suddenly reports
740 where every previous number said ~430 needs the other two lanes to know
why before it lands, which is the same sequencing argument as the gate above.

**And the status line those 62 were supposed to carry was mostly absent.**
`known-issues.md`'s own preamble: *"Put a `**Status:** ...` line immediately
under the heading -- `OPEN` / `FIXED <date>` / `RESOLVED <date>`"*. On finding
this, 6 of 62 had one. **48 of the 56 missing were lane A's -- mine.** I wrote
the rule's violation 48 times while writing entries about verdicts taken over
the wrong population.

**Why a blanket stamp was the wrong fix, and what was done instead.** The 55
lane-A entries are not one kind of thing:

| shape | n | status meaningful? |
|---|---|---|
| plain issue, no status anywhere | 25 | yes -- stamped `OPEN` |
| status word IS the heading (`### [A] RESOLVED -- ...`) | 12 | already greppable; left alone |
| experiment log (`PREDICTION P22`, `RESULT P23`) | 9 | no -- a verdict, not a status |
| already compliant | 7 | -- |
| correction record | 2 | no -- nothing to close |

The 25 were stamped `OPEN`, deliberately, including 6 whose bodies claim a
fix: on reading, every one of those is a **doc-level** fix (`devpower`'s
`/proc` header, `syshealth.rs:18`, `faceunlock`'s "docs only") while the
feature gap the entry is about remains. `OPEN` is also the safe direction: a
wrong `OPEN` costs someone an investigation and then self-corrects, whereas a
wrong `FIXED` is never revisited. The 11 non-issues were left unstamped rather
than forced into a vocabulary with no slot for them -- which is the part that
still needs deciding.

**What is deliberately NOT done: extending the checker.** The obvious fix is
to make `check-known-issues-index.py` enforce the `### [lane]` convention
too. That gate would immediately fail on lane B's 7 and lane C's 1 unstamped
entries -- reddening two trees over a rule they have not been told about,
which is exactly what cost lane A two pre-flight runs (658s and 2022s) on
2026-09-17 when lane C's new gates fired on lane A's tree. The order has to
be: notify, let them stamp, then gate.
