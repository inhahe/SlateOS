## `TD-C-A-HUNDRED-APPS-BIND-KEYS-NOBODY-CAN-FIND` (lane C, 2026-09-18) -- **CLOSED 2026-09-22**

> **Closed as the programme it asked for, finished.** This entry filed the
> work; `TD-C-NINETY-ONE-APPS-BIND-KEYS-AND-NAME-THEM-NOWHERE` measured it
> and is where the outcome is recorded. Today `scripts/key-survey.py`
> reports 131 apps binding a letter, digit or function key, 87 carrying a
> key list, and no app answering a key it names nowhere; the baseline file
> is empty and the survey exits 1 if that stops being true.
>
> **Its estimate was the one thing it got right.** "About fifteen minutes
> per app and entirely mechanical" held for roughly thirty apps. What it
> did not predict is that the mechanical work would surface six defects
> that had nothing to do with keys -- `paint` could not save or open,
> `markdowneditor`'s `Ctrl+O` was bound to nothing, `tmux` announced a pane
> zoom it never performed, `camera` had a 25-row reference no code drew,
> and `fileassoc` and `diskcleanup` each had a key one press from deleting
> files. Naming a key forces someone to say what it does, and that
> question had not been asked of these apps before.

**In short:** Seven apps got a shortcut list today. A survey of the other 134
says the same problem is everywhere: **126 apps bind at least one letter, digit
or function key, and ten of them print a list of their keys.** The fix per app
is now about fifteen minutes and entirely mechanical, so this is a programme of
work rather than a defect -- filed with the tool that finds the candidates and
one worked example verified by hand.

**The number of apps that print a key list is 27.** This entry has said ten,
then 29, and now 27. Three figures for one quantity, and the sequence is the
most useful thing in the file, so it goes first.

I wrote "the one number here that needs no inference is ten -- a crate either
contains a key list or it does not". The premise is true and the number was
wrong, because **the inference was hidden inside the tool**: it searched for
the identifiers `SHORTCUTS` and `ALL_KEY_ACTIONS`, and `apps/magnifier` calls
its list `HELP_ROWS`. A dozen more spell it a dozen other ways. I had written a
search for the spelling somebody happened to use, which is the exact defect the
survey exists to find, and then quoted its output as the one figure not subject
to it.

So I replaced the name test with a *shape* test -- `const NAME: ... (&str,
&str)` -- on the reasoning that a name is chosen and a type is fixed. That gave
29, and 29 is also wrong. `(&str, &str)` is what a key list is made of and
equally what every other table of string pairs is made of: `apps/explorer` has
`SIDEBAR_ITEMS` mapping a label to a path, `apps/gomoku` has `PANEL_LINES`
mapping a label to a sample value for measuring text. Both matched.

**There is no purely structural signal for "a list of keys".** The type says
pairs of strings; only the contents say what kind. The detector now reads the
first column and asks whether it parses as key labels, which is openly a
heuristic, and 27 is reported as one. It agrees with a hand count of the same
thing, which is the only reason to believe it at all.

**The sequence is the lesson, not the number.** I moved from a wrong answer to
a differently wrong answer while each time believing I had removed the
judgement -- first into the tool's choice of identifier, then into its choice
of type. A number about a codebase almost always contains an inference about
what counts, and the useful habit is not to eliminate it but to *say where it
is*, so a reader knows what they are being told. Every figure in this entry now
names its own method. **Lane A hit the identical shape the same day**: their
`scripts/check-variant-lists.py` checks every list *named* `ALL`, so a list
that should be total and is called `PRIMARY_COMMANDS` is invisible to it and
nothing says so. Two tools, two authors, same afternoon, both defining their
population by a name.

Everything else the survey prints is a *candidate list*, and it is wrong in
both directions:

| | |
|---|---|
| **Overstates** | a `Key::` match is not a shortcut. `apps/crossword` pairs `(Key::Q, 'Q')` through the whole alphabet -- that is how letters get into squares, not twenty-six hidden commands. |
| **Understates, badly** | a one-character key name matches almost any prose. `apps/renamer` binds `L`, `U`, `T`, `S`, `K`, `W`, `E` and `X`; the survey flagged four, because "Lower" contains an `L` and "Snake" an `S`. |

So the survey ranks apps to read by hand. It does not decide anything, and the
report says so at the top of the file.

**The worked example, verified by reading it and then fixed.** `apps/renamer`
answered eight letter keys, each adding a rename operation to the pipeline -- lower, upper,
title, snake, kebab, trim, extension-lower, extension-remove. The only string
in the crate that comes near naming one is `"kebab-case"`, which is the label
of the *operation*, not of the key that adds it. So the program's entire
purpose is reachable only by someone who has read the handler, which is the
same defect as `apps/slides` being unable to put a shape on a slide, one step
further out: the operation is reachable, and the way in is not. It now prints
the list on `F1` or `?`. `apps/hexeditor` and `apps/filediff` followed -- thirteen chords and a
page of navigation, `Ctrl+B`/`Ctrl+N`/`Ctrl+P` worst of all, because bookmarks
are invisible until one is set and so the feature could not be found by looking
at the window in any state. That leaves the survey's count at 126 apps binding
an unguessable key and 27 printing one -- a gap of roughly a hundred, not the
hundred and sixteen the first count implied.

**A gate for this was considered and declined, which is worth saying so nobody
builds it twice.** The obvious move is a ratchet: baseline today's list, fail
when an app joins it, the way `lossy-decode` reports "26 file(s) baselined,
none worse". Two things argue against it. The battery those gates live in runs
on **push**, not commit, and a push in this tree already takes thirty to forty
minutes -- it is the throughput bottleneck, and adding to it costs all three
lanes on every push to catch a defect in one lane's tree. And the thing it
would prevent is a *new* app being written without a key list, in a tree that
already has 141 of them and is not gaining many. The tool is `scripts/key-survey.py`,
it runs in about a second, and running it is one line. If apps start being
added in numbers again, the ratchet becomes worth its cost and this paragraph
is the argument for building it then.

**Four things the program had never written down** turned up while building the
states its guard test needs, each found by a red test rather than by reading
the source, and they are the reason the per-app cost is fifteen minutes rather
than five: `Up` declines at the top of the list; adding a rule selects it, and
the newest is last, so no single state has a rule with room both above and
below it; `execute_rename` records nothing unless a name actually moves on
disk; and files arrive *already selected*, so a helpful `Ctrl+A` in the setup
saw everything selected, did its opposite, and renamed nothing. The last was
caught only by an assertion written into the state helper for exactly that
case. Without it the suite would have been green over a `Ctrl+Z` pressed at an
app with nothing to undo -- **a test that checks nothing passes exactly as
loudly as one that checks everything.**

**Why this is filed rather than done.** There are of the order of a hundred
apps in it, at roughly fifteen minutes each. That is not a task, and doing a
handful more would leave the entry saying the same thing with a smaller number
in it. What makes it worth filing rather than merely noting is that everything
expensive is already built: `guitk::shortcut::keystrokes` reads the labels,
the overlay is forty lines that cannot disturb a layout it does not understand,
and the two tests each app needs are written once and copied --
`every_advertised_key_does_something` against the handler and
`the_shortcut_list_reaches_the_window` against the screen.

**Order to work in**, when somebody picks this up: the survey's letter and
function-key columns first and its digit column last. Digits are nearly always
a size, a level or a view, and an app that draws "Levels 1-8" has named all
eight to a reader while naming two to a substring search.
