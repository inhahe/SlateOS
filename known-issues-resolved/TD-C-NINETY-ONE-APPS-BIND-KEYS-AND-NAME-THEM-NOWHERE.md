## `TD-C-NINETY-ONE-APPS-BIND-KEYS-AND-NAME-THEM-NOWHERE` (lane C, 2026-09-21) -- **CLOSED 2026-09-22**

> **Closed.** `scripts/key-survey.py` reports 131 apps binding a letter, digit
> or function key, 87 of them carrying a key list, and **no app answering a
> key it names nowhere**. `scripts/key-survey-baseline.txt` is empty and the
> survey exits 1 if it stops being so.
>
> **The title's number was never right and neither were the next three.** It
> read 91 as filed, then 26 of 68, then 55 of 259, then 36 of 133. Every
> revision was a flaw in the instrument rather than a change in the tree, and
> there were five in all -- the last being that the survey could not see a key
> dispatched from a *typed character*, which is how `apps/tmux` answered
> seventeen of them. The note below, warning the reader to trust the tool over
> the text, is the only thing in the original header that stayed true.
>
> **What the queue actually led to.** Six defects, none of which is about
> naming:
>
> | app | what was wrong |
> |---|---|
> | `paint` | could not save or open a picture, and advertised both |
> | `markdowneditor` | `Ctrl+O` was advertised in a field nothing reads and bound to nothing |
> | `tmux` | `prefix z` set a status line saying "Pane zoom toggled" and did nothing |
> | `camera` | a complete 25-row keyboard reference whose only caller was its own test |
> | `fileassoc`, `diskcleanup` | a key one press from deleting files, named nowhere |
>
> **And seven rows I invented.** Writing a card from the key rather than from
> the arm produced seven wrong descriptions -- `Esc` as "Back" four times,
> `Space` as "select" in a program where it starts a slideshow, `Left/Right`
> as "previous disk" where they resize a partition. Each was caught by the
> guard, never by re-reading. The lesson is in the shape of the mistake: when
> I write a row from the key, I write what that key means in *other* programs.
>
> **What is not done:** the gate is not registered in `scripts/boot-test.sh`,
> which is lane A's file. Until it is, "the queue is empty" is a fact about
> today rather than a property this tree keeps. The request is
> `requests/c-a-a-gate-for-keys-an-app-answers-and-names-nowhere.md`.

> **Read this first (added 2026-09-21; this note has itself been wrong once).**
> The title says ninety-one. The figure has moved three times in one day and
> the current one is **55 apps and 259 keys**. Do not trust any number written
> here; run `python scripts/key-survey.py` and read its tail.
>
> | reading | count | what changed |
> |---|---|---|
> | as filed | 91 apps | apps with no `(&str, &str)` const -- a count of a *shape*, not of the gap |
> | after teaching the survey ranges and group words | 26 apps, 68 keys | `1-7` names `Num2`; `Arrows/WASD` names eight keys |
> | after fixing single-letter matching | **55 apps, 259 keys** | `"A" in "Add City"` had been counting as naming the `A` key |
>
> **The middle reading is the instructive one, because it was mine and I
> announced it as a two-thirds reduction.** I had found the survey
> over-reporting, fixed that, re-measured, and wrote the result down as the
> truth. What I never checked was whether it *under*-reported as well -- and it
> did, far more: the substring match on single letters was hiding about 190
> keys while I was congratulating the tool on losing 23. Varying one axis
> licenses conclusions about that axis and no other, which is shape 17 in the
> catalogue below, applied to my own correction of the thing shape 17 is about.
>
> The entry body is kept as filed. The slug is left alone because it is what a
> triage grep keys on.

**In short:** most of the apps in this suite answer keyboard shortcuts and
never tell you what they are. Of 127 apps that bind a letter, digit or
function key, **36 carry a key list and 91 do not.** There is nothing to
press and nothing on screen: the only way to find out that `R` restarts a
level or `Ctrl+G` finds the next match is to read the source. The decision
about *how* an app should show its keys was already taken (design-decisions
863: `F1` always, `?` as well where nothing needs to type one) and the
toolkit support already exists -- what is missing is doing it 91 times.

**How the number is got.** `python scripts/key-survey.py`. The printed table
only shows apps that also have *unnamed* keys, so the 91 are not all visible
in its output; the full queue comes from `survey()`'s fourth return value.
The count is candidates, not verdicts -- see the module docstring for the
three things it cannot know.

**What "done" looks like for one app**, all of it already established by the
twelve apps that have been through it (`apps/jsonviewer` is the clearest
model):

1. `const SHORTCUTS: &[(&str, &str)]` listing the keys the app really binds,
   last row `("F1 / ?", "This list")`.
2. A `show_help` flag toggled by `Key::F1` or `Shift`+`Slash`, closed by
   `Escape`, and **guarded against stealing a keystroke from a text field** --
   `jsonviewer` gates the whole block behind `if !typing`.
3. `guitk::shortcut::render_card(...)` in `render`, drawn last so it is on top.
4. Two tests: `every_advertised_key_does_something`, which parses each label
   with `guitk::shortcut::keystrokes` and asserts the app consumes it, and
   `the_shortcut_list_reaches_the_window`, which asserts each row's text is
   actually drawn. The first catches a list that over-promises, the second a
   list that is written and never rendered.

**What the first sixteen apps taught about writing those two tests**, because
every one of these cost a round and the next person should not pay for them
again:

*The guard almost always needs several states, and almost every failure it
reports first is a **correct refusal** rather than a missing binding.* Observed
so far: `set_view`/`set_tab` answering `Ignored` for the view you are already
on, so no single state can answer all of `1-6`; `step_location` refusing to
walk off either end; undo and redo unable to both have work in one state, since
undoing is what creates the redo; `Esc` needing something open to leave;
`Ctrl+S` returning `None` with no storage path; a stopwatch key needing a
running stopwatch. Build the set, use `.any()`, and say in the test *why* each
state is there -- that comment is the whole value, because the next reader's
instinct will be to delete the state.

*The "nothing acts behind the card" test needs a control, and the control is
the half that finds the bugs.* Five times the negative assertion watched a
quantity the action does not move -- `messages.len()` after a `Delete` that
files to Trash, `alarms.len()` after an `N` that opens an editor, a `playing`
flag an app with no audio backend never sets -- and every one passed while
testing nothing. Press the same key with the card **down** and assert it *does*
act. See shape 21 in the catalogue.

*Read what the action does before choosing what to watch.* `delete_message`
sounds like it deletes; it files. `toggle_play` sounds like it plays; it writes
`NO_AUDIO` to a status line.

*Write the list from the handler, not from memory.* `apps/passwordgen`'s first
draft advertised `Up / Down` for "move through the options" and the app binds
neither key -- written from a glance at a `toggle_option` call. The guard
caught it on its first run, which is the pleasant case; nothing else would
have.

*Put the card check above the view dispatch, not beside the modifier handling.*
`apps/contacts` claims `Escape`, `Enter`, `Tab` and `Backspace` in a `match`
that runs before any modifier is looked at, so a card check placed where the
`Ctrl` pair lives could raise the card and never close it -- a modal with no
exit, which is the exact trap being fixed in `apps/editor` the same afternoon.
`apps/ebook` dispatches to five views; the check goes above all five so the
card works from each and closes from each.

*`?` is not always free, and the reason is not always a text field.* dd-863
says bind it where nothing needs to type one. `apps/ebook` needs no `?`
typed -- and `/` opens its search through an arm that never looks at Shift, so
`?` already opens a search. Binding the card to it would have shadowed a key
the program answers, and no guard would have caught that, because the card
really would have opened. Grep for a bare `Key::Slash` arm before deciding.

*Check whether the app already has a better arrangement before adding a card.*
`apps/passwordgen`'s option rows print their own keystrokes and parse the
printed label to match them, guarded by two existing tests; `apps/videoplayer`
drives the whole thing off one table. Copying those keys into a `SHORTCUTS`
const would add the third copy the arrangement exists to avoid.

**Two things that will bite whoever does this.**

*The label is parsed, so it has to be parseable.* `guitk::shortcut::keystrokes`
understands `/` and `,` alternatives, the word `Arrows`, and exact ranges like
`A-Z` or `1-9` (**no spaces** -- `1 - 9` is not a range). It does **not**
understand `WASD`, which four games bind as movement (`apps/asteroids`,
`apps/game2048`, `apps/snake`, `apps/sokoban`). Either write them out as
`W, A, S, D` or add a `wasd` case beside the existing `arrows` one, which is
the same shape of abbreviation and probably the right fix.

*A key can be advertised and still be refused in the state the test starts in.*
`apps/sokoban` answers `Enter` in its level menu but deliberately ignores it on
an unsolved board, so `every_advertised_key_does_something` passes or fails
depending on which screen the sample app is on. The list is per-app work, not a
sweep: the row has to name the mode (`"Enter / Space", "Next level, once this
one is solved"`) and the test's sample app has to be in a state where the key
means something.

**Known false positives in the queue.** `apps/terminal` tops the survey with 41
keys, and they are *encodings* rather than shortcuts -- `Key::A => Some(0x01)`
is how a control character is produced, not a command the user should be told
about. `apps/markdowneditor`'s two are `Key::Char` and `Key::Function`, which
are variant names and not keys. Neither needs a list.

**The queue, most keys first** (from the run of 2026-09-21):

```
videoplayer 31, wordle 29, hangman 29, crossword 27, rssreader 26, editor 19,
paint 17, sokoban 16, kanban 16, filesearch 16, compass 15, metronome 14,
rush 13, musicplayer 13, automator 13, torrent 12, remotedesktop 12,
worldclock 11, klotski 11, dictionary 11, weather 10, stickynotes 10, snake 10,
passwordgen 10, sysmonitor 9, pomodoro 9, benchmark 9, screenrecorder 8,
launcher 8, alarmclock 8, yahtzee 7, screenshot 7, notes 7, ebook 7,
contacts 7, snippets 6, mediaconvert 6, email 6, dbviewer 6, archivemanager 6,
whiteboard 5, tetris 5, systemrestore 5, stopwatch 5, settings 5, match3 5,
dots 5, credmanager 5, asteroids 5, startupmanager 4, pinball 4, mahjong 4,
freecell 4, fileassoc 4, devicemanager 4, charmap 4, typingtutor 3, sysinfo 3,
spades 3, solitaire 3, procexplorer 3, colorpicker 3, unitconverter 2,
undelete 2, speedtest 2, soundrecorder 2, pong 2, podcast 2, partmanager 2,
pacman 2, nonogram 2, markdowneditor 2, hearts 2, habits 2, gomoku 2,
fontmanager 2, flashcards 2, diskcleanup 2, diskanalyzer 2, clipmanager 2,
breakout 2, battleship 2, tmux 1, taskscheduler 1, reversi 1, netscan 1,
netmanager 1, defrag 1, chess 1, checkers 1, terminal 41 (false positive)
```

**A related gap found while measuring, worth fixing with the first batch:**
`apps/game2048` *has* a help overlay and raises it with `H`, and `Escape`
closes it. That predates 863 and is exactly the failure 863 names -- a user who
learns `F1` from one app finds it dead here. It needs `F1` and `?` added to the
existing overlay, not a new one.

**If this is never done,** nothing breaks and nothing gets worse; the keys keep
working for whoever reads the source. It is a discoverability gap, not a bug,
which is why it is an entry here rather than an operator question.

### Correction, 2026-09-21, same day: 91 is the count of a *shape*, not of the gap

The first app opened off this queue disproved the headline. `apps/sokoban`
appears in the 91, and it already names every key it binds, in a permanent
two-line footer that changes with the screen:

```rust
const SELECT_FOOTER: [&str; 2] = ["Up/Down: choose   Enter: play", "1-9: jump to a level"];
const PLAY_FOOTER: [&str; 2] = [
    "Arrows/WASD: move   Z: undo   R: restart",
    "Esc: menu   N: next level",
];
```

Drawn by `draw_footer` on every frame, and covering all four of its screens'
bindings. That is precisely the case design-decisions 863 already carved out --
"a list already on screen needs no key to raise it, and adding one would mean
drawing the same list twice" -- so the correct amount of work on `apps/sokoban`
is none. Adding an `F1` card would have been a second copy of a list that is
already right, which is the very defect
`TD-C-A-PRINTED-KEY-LIST-IS-A-SECOND-COPY` exists about.

**What the survey's `list` column actually means.** It looks for a `const` or
`static` of `(&str, &str)` whose first column reads as key labels. `sokoban`'s
footer is `[&str; 2]` of pre-joined sentences, so the detector cannot see it,
and correctly does not claim to -- the module docstring says it reports
candidates rather than verdicts, and names "an app may name its keys in prose
the user reads elsewhere" as the third thing it cannot know. The 91 was read
as the size of the gap when it is the size of a shape.

**Re-measured, two axes rather than one.** Of the 91:

| | apps | what they print |
|---|---|---|
| separator form | **10** | `"Z: undo"`, `"Arrows/WASD: move"` -- a real hint line |
| prose form only | **25** | `"Press F5 to refresh"` -- at least one key named in a sentence |
| neither | **56** | nothing in any string literal that reads as a key hint |

The second axis is the one that matters methodologically: the first pass found
only the separator form and reported 81 as the gap. Adding the prose pattern
moved 25 apps out of it. **A single pattern could not tell "this app says
nothing" from "this app says it in a shape my regex does not match"** -- the
same reading that made the first pass of this very entry wrong.

**So the real queue is at most 56, and is probably smaller still.** Both
numbers are string-literal counts, and a literal is not proof it is drawn --
`apps/netscan`'s `wol_note` was written by the model and drawn by nothing.
`sokoban` was confirmed by reading `draw_footer`; the other nine separator-form
apps (`compass`, `rush`, `nonogram`, `battleship`, `klotski`, `reversi`,
`checkers`, `snake`, `dots`) are *candidates for needing nothing* and each
wants the same two-minute read before any work is done on it. The 25
prose-form apps are the opposite case and almost certainly still need a list:
`apps/videoplayer` binds 31 keys and has one prose mention, which is not a list
by any reading.

### The second front -- retracted the same day; it was a measurement error

**What this section said for about twenty minutes:** that 39 apps carry a key
list, **18 of them have no guard**, and that chasing those eighteen was the
higher-yield front because `apps/paint`'s unguarded list had been 8 of 33 dead.

**That number was wrong, and how it was got wrong is the useful part.** It came
from grepping each crate for the *names* `every_advertised_key_does_something`
and `the_shortcut_list_reaches_the_window`. `apps/minesweeper` came back
"unguarded" and has guarded its list since the day it was written -- its tests
are called `every_key_the_footer_names_does_something` and
`the_footer_names_the_keys_that_do_something`. The scan was keyed on one
spelling of a name, which is the failure this file has a whole catalogue about.

**Three measurements of one question, in order:**

| keyed on | answer |
|---|---|
| the two test *names* | 18 unguarded |
| any mention of the list's identifier from test code | 1 unguarded |
| a test that *iterates* the list (`for .. in NAME`, `NAME.iter()`) | 3 unguarded |

None of the three is the property. The second counts `apps/paint`'s old
`assert!(!shortcuts.is_empty())` as a guard, which guarded nothing -- it is the
exact test that sat beside 8 dead rows for the program's whole life. The third
misses `apps/life`, which checks its rows by index rather than by iterating,
and `apps/mixer`, which pairs each row with an action.

**Settled by reading, which is what it needed all along: one app.**
`apps/towers` mentions `HELP_ROWS` twice and both are in *drawing* code -- no
test refers to it at all. `apps/life` and `apps/mixer` are genuinely checked,
in shapes no regex of mine recognised.

**So this front is one app, not eighteen, and the first front is the real
work.** `apps/paint` stays the argument for writing the guard -- it found six
defects nobody suspected -- but it was not evidence of a widespread pattern,
because the population it seemed to belong to did not exist. Its list was
unusual in being a `Vec` returned by a function rather than a const, which is
also why the survey never counted it as a list at all.

**One finding from that retracted pass is still worth keeping,** because it
applies to any list a guard is put on. Several of these panels are *how to
play* rather than *shortcuts*, and their first column mixes keys with prose:
`Click`, `Click a cell`, `Wheel`, `Goal`, `Two tiles alike`. Several others
write ranges with spaces -- `1 - 9`, `1 - 7`, `1 - 4`.

`guitk::shortcut::keystrokes` refuses both. It returns `UnknownKey` rather than
skipping what it cannot read, deliberately, and its doc comment gives the
reason: "a guard test handed a shorter list than it asked for would pass while
checking less". A range is accepted only as an exact `X-Y`, so that the `-` key
itself is never mistaken for one.

So a guard dropped on one of those panels *panics* rather than reporting, and
the tempting repair -- teach it to skip rows it cannot parse -- is precisely
the hole that lets a genuinely dead key hide behind a label nobody taught the
parser. **Do not teach the guard to skip.** Either split the panel so the key
rows are their own list, or keep one list whose first column is strictly
keystrokes and move `Click` and `Goal` into the description column.

### Progress, and a third app off the queue that needed nothing

`apps/towers` is **done** -- its sheet is split into `RULES` and `SHORTCUTS`,
`N` is advertised at last, `F1` and `?` join `H`, and three guards hold it.
That closes the one-app second front.

`apps/videoplayer` sits at the top of the first front with 31 keys and "no
list", and **needs nothing at all** -- it has the best key documentation in the
suite. Its `Shortcut { keys, action, press, command }` table is drawn by the
help panel *and* searched by the key handler
(`Shortcuts::list().iter().find(|sc| sc.press.matches(event))`), so the printed
label and the working binding are one object and cannot drift. Its doc comment
even records removing `Ctrl+O` and `Ctrl+S` because the tree has no file
chooser and no framebuffer read-back -- the exact defect `apps/paint` shipped,
caught here as a matter of course because deleting the row and deleting the
binding are the same edit. Written up as design-decisions 866, which adopts
that shape as the preferred one where an app already has a command type.

**That is three of the first four apps opened off this queue that needed no
work** -- `sokoban` (footer), `minesweeper` (guard under another name),
`videoplayer` (single table) -- against one that needed a great deal
(`paint`). The queue is a list of *candidates* and behaves like one. Read
before editing, and expect to close entries with a note rather than a change.

### `apps/launcher`: named at run time, invisible to a static survey

Top of the real queue with six unnamed keys, and it needs nothing.
`Ctrl+1`..`Ctrl+8` pick the nth result and the launcher draws the hint beside
each of the first eight rows -- as `format!("^{}", i + 1)`, so the digit is
computed at run time and the source literal is `"^{}"`. No survey over string
literals can see that, and this one should not pretend to.

Caret-notation support was added for it and then removed: a measurement showed
**zero** string literals in the whole `apps/` tree match caret notation, so the
feature fired nowhere. Its only possible effect was a false negative -- a
footnote marker in prose read as naming a key, making the survey go quiet
wrongly. Speculative generality in a checker is worse than in ordinary code,
because the only thing it can do is hide something.

This is the first entry for the answered-file when that exists, and the reason
is *the keys are named at run time*, not *this is inconvenient to fix*.

### The two directions, and which tool owns each

Every per-app guard in this tree -- `every_advertised_key_does_something`,
`every_key_the_footer_names_does_something`, and the two written today for
`apps/klotski` and `apps/snake` -- runs in **one direction only**: it takes
each row of the printed list and checks that something answers it. Nothing in
any of them can notice a key that *works and is not printed*.

That is exactly how `apps/minesweeper` came to have `F2` and `1`/`2`/`3` bound
and unadvertised while carrying a guard that passes: `F2` is an alias for `N`
and the three digits pick the difficulty, and the footer names neither. Its
guard is correct and complete for what it does. It is simply the other
direction.

**The reverse direction cannot be a unit test** -- a test cannot enumerate the
arms of a `match` -- and it does not need to be, because
`scripts/key-survey.py` already does precisely that: it collects every `Key::`
variant the crate mentions in live code and asks whether the app's own drawn
strings name it. So:

| direction | owner |
|---|---|
| everything advertised works | the per-app guard test |
| everything that works is advertised | `scripts/key-survey.py` |

**The survey should become a gate, and cannot be one yet.** Today it prints a
report nobody is obliged to act on, which is why `F2` sat unadvertised. The
proper end state is a non-zero exit when an app binds a key it never names,
with an answered-file for the genuine exceptions -- `apps/terminal`'s 26
control-character encodings (`Key::A => Some(0x01)` is not a shortcut),
`apps/paint`'s `J` and `Q` (present in its `Key`-to-`char` table, bound to
nothing), `apps/markdowneditor`'s `Char` and `Function` (variant names, not
keys) -- on the same terms as `scripts/frozen-flag-answered.txt`: every line
carries a reason, and the reason has to say what *makes* it not a defect.

**Order matters.** Seeding that file with all 26 apps would be a suppression
list wearing an answered-file's clothes. Fix the ~38 genuine ones first, then
close the loop with an answered-file holding only the ~30 true exceptions.
After that no app can gain a key without naming it.

### Triage of the 10 hint-printing apps -- 6 need nothing, 3 need one row each

Read rather than edited, which is the point. For each, the keys it binds were
compared against the keys its own drawn hints name, **with digit ranges
expanded** -- `1-7: puzzle` covers `Num1`..`Num7`, and a first pass that
matched on the literal `Num2` reported six false gaps in `apps/klotski` alone.
That is the same misreading as `apps/sokoban`, three times over.

| app | verdict |
|---|---|
| `rush`, `nonogram`, `battleship`, `reversi`, `checkers`, `dots` | **need nothing** -- every key they bind is named in a hint they draw |
| `sokoban` | **needs nothing** -- verified earlier by reading `draw_footer` |
| `compass` | **needs nothing** -- see the correction below |
| `klotski` | advertise `P` -- **done** |
| `snake` | advertise `1`, `2`, `3` -- the difficulty keys -- **done** |

**`apps/compass` was wrong in this table for about an hour, and it is the
fourth app off this queue that needed nothing.** Its `draw_help` already
draws `"1-0: select waypoint"`, and re-checking with that understood leaves it
with no unadvertised key at all. Two separate bugs in my own scan hid it: the
hint pattern required the first token to look like a key *name*, so a row
beginning with a digit was never recognised as a hint; and the range expander
read `1-0` as `range(1, 1)`, which is empty -- `1-0` means 1 through 9 and then
0, which no ascending expander gets right. The hint is correct and readable by
a person; it is only machine-hostile, and `guitk::shortcut::keystrokes` would
refuse it too, since a range there must ascend.

**Every automated pass over this question has over-reported, in a different way
each time** -- `sokoban` (footer the detector cannot see), `minesweeper` (guard
under another name), `klotski` (literal `Num2` vs the range `1-7`), `videoplayer`
(table of structs, not tuples), `compass` (digit-led row, descending range).
Five mechanisms, five false alarms. The queue is worth having because it points
at candidates, and **every candidate has to be read before it is edited.**

**The working order.** The three above first -- each is one row in a hint line
that already exists. Then the 56, largest first
(`videoplayer` 31, `hangman` 29, `wordle` 29, `crossword` 27, `rssreader` 26,
`editor` 19, `paint` 17 ...). Then the 25 prose-form. Then read the 10 and
expect to close most of them with no change, recording *why* each needed
nothing so the next reader does not re-open it -- the survey will keep
reporting all 91 until it learns the footer shape, and an answer that is not
written down is one that gets rediscovered.
