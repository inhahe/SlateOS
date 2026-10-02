## TD-C-THE-OTHER-HALF-PROGRAMS-THAT-SAY-TOO-LITTLE -- FIXED 2026-09-15

**In short:** after the fabrication sweep, ten more programs turned out to be
misleading in the opposite direction. None of them invents anything. Each has
an empty screen, or a silent incapacity, and nothing explaining it -- and an
empty screen is read as an answer.

**Date:** 2026-09-15. **Lane:** C. Follows
`TD-C-WHAT-THE-FABRICATION-SWEEP-ACTUALLY-TAUGHT`.

### Why a second scanner was needed

`scripts/find-reachable-fixtures.py` asks who *calls* an invented-data builder.
It is a good question and it is structurally blind to this: a program with no
fixture at all has nothing for it to find.

`apps/clipmanager` is the case that showed the gap. It invents nothing, so the
fixture scanner never looked at it. It also has no capture path -- nothing
watches the clipboard and nothing can -- so the history is empty and stays
empty however long the window is open. **An empty list under the word
"History" is read as a statement about the user**: you have not copied
anything.

`scripts/find-silent-incapacity.py` asks the complementary question: does the
crate reach anything outside its own process, and if not, does it admit that
**in a string the user could read?** Matched on string literals only, because a
comment is not an admission. That distinction is the whole sweep.

**It accused two honest programs on its first run** -- `apps/diskanalyzer`
("Could not scan: {err}") and `apps/pdfviewer` ("none can be opened") -- because
its vocabulary was narrower than the language. A tool that accuses honest code
is one the next reader learns to skip, so it was widened before a single line
of its output was acted on.

It reports 37 of 142 crates and says in its own docstring that this is **a list
to read, not a list to empty**. A calculator owes nobody an explanation, and
the tail of the list is games and converters.

### What the ten were

| app | the silence |
|---|---|
| `clipmanager` | nothing watches the clipboard; the history is empty forever |
| `alarmclock` | the alarm fires, but there is no sound and no notification |
| `diagram`, `whiteboard`, `mindmap` | drawings cannot be saved |
| `logviewer` | no log can be read -- **an empty log view is not a quiet system** |
| `defrag` | the drive list is empty and said only "No drive selected" |
| `flashcards` | spaced repetition with no memory between runs |
| `calendar` | events invented *and* not kept |

### The three findings worth carrying

**A deferred failure is the expensive kind.** `alarmclock` is the purest
instance in the whole sweep: the alarm genuinely fires, sets `ringing`, changes
the row. What the crate has no path for is sound or a system notification. **An
alarm that changes a pixel in an unfocused window will not wake anyone**, and
the user finds out by missing the thing they set it for. Same structure as
`apps/weather`'s alert channel, `apps/podcast`'s Downloaded mark failing on a
plane, and `apps/flashcards`, where **spaced repetition is defined by history**
and a scheduler that forgets will show a card mastered last week while holding
back one about to be forgotten -- invisibly, because not remembering is what
the user came about.

**An empty view is not always the same sentence.** `diagram`, `whiteboard` and
`mindmap` lose work *prospectively*, when the window closes. `logviewer` fails
*immediately* and its emptiness reads as evidence: "nothing was written" and
"nothing was looked at" are opposite conclusions for somebody who opened a log
viewer because something went wrong. They were fixed in one commit and worded
separately for that reason.

**Care taken nearby is not evidence about the thing in front of you.**
`apps/defrag` was the near-miss. Its `main` explains that SlateOS cannot
enumerate volumes, the Analyze button is greyed to match, the gap is tracked in
this file, and its sample block map is test-only. I nearly dismissed the
scanner's hit on the strength of that. The comment is in the source and the
greyed button is a hint; the one sentence the user read was **"No drive
selected"**, which is an *instruction* -- it asks them to pick a drive when
there are none to pick, and an empty drive list under a defragmenter is read as
"this machine has no drives".

That is the same error as trusting a careful fixture (`reminders`' relative
dates, `habits`' plausible 70%, `contacts`' reserved phone numbers), one level
out: **thoughtfulness in the neighbourhood reads as thoughtfulness about the
line you are looking at, and is not.**

### And a note on having two scanners

They are not redundant and they are not a majority vote. `apps/flashcards` was
reached by both and they disagreed usefully: the fixture scanner flagged its
three decks, which **stay** -- "the capital of France is Paris" is a true
statement about the world, bundled as content -- while the silence scanner
found the real defect. Where they overlap, the answer still has to be reasoned
out per app.
