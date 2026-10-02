## `TD-C-SIXTY-FLAGS-A-USER-CANNOT-REACH` (lane C, 2026-09-18) -- **CLOSED 2026-09-21**

**In short:** 60 boolean fields across 25 apps are read by the program and
never written by it. The renderer draws from them, the behaviour depends on
them, and no keystroke, click or setting can change one. A user sees a toggle
that does not work, or more often a choice that was made for them and never
offered -- which is why none of these has ever been reported.

**The verified example.** `apps/regextester` draws three flag toggles along the
top -- `("i", self.flags.case_insensitive, "Case insensitive")`, `g` for
global, `m` for multiline -- and compiles the pattern with
`RegexCompiler::new(&self.pattern, self.flags.case_insensitive)`. The only
assignment to any of them in the whole crate is
`app.flags.case_insensitive = true;` **inside a test**. So the flags are drawn,
are read, decide the result, and cannot be changed: a regular-expression tester
whose case sensitivity is fixed at compile time.

That is the same defect as `apps/passwordgen`'s frozen options and
`apps/mindmap`'s `show_sidebar`, eight of which were fixed by hand on
2026-09-18 by reading one app at a time. `scripts/frozen-flag-survey.py` is
that reading, mechanised.

**Fixed 2026-09-18, and reading it turned up two more defects behind the
first.** Wiring the toggles meant checking each flag actually did something,
and one did not: **`multiline` was declared, constructed, drawn, asserted in a
test, and read by the matcher nowhere.** The anchor arm was `pos == 0` and
`pos == len` whatever the flag said. Offering a toggle for it would have been
the worse defect -- a button that moves and changes nothing is a claim, where a
frozen button is merely a gap -- so the flag now reaches the matcher and `^`
and `$` match at every line boundary, with a test that asserts the *result*
rather than the field.

The second was one line: `let _ = tooltip; // used for hover tooltip`, in a
crate with no hover tooltip in it. A comment describing what a value is *for*,
directly above the line throwing it away. Lane A hit the identical shape the
same day (`let _ = before; // Used to verify timing sanity.`), which suggests
the form is worth naming: **a discard with a justification reads as considered,
and is the easiest place for an unfinished feature to come to rest.** The three
strings live in `SHORTCUTS` now, where the `F1` card draws them and the guard
test presses them.

`F1` and not `?`, because every printable character is typed into whichever
field has focus -- the same reason as `apps/spreadsheet` and `apps/hexeditor`.

**Why no compiler or existing gate catches it.** `dead_code` is silent because
the field is read. `check-fields-written-never-read.py` looks for the mirror
image -- written and never read -- and `check-unreachable-mutators.py` finds a
mutator nothing calls, which requires the mutator to exist; here there usually
is none. The program is *consistent*, which is precisely what makes it
invisible: nothing is ever wrong on screen, there is simply one behaviour where
two were designed.

**What the number is worth, stated with its method.** Booleans only, in the
struct behind `impl App for X`, in live code across every file of the crate,
with construction not counting as a write. Restricted to `bool` because a
field of a struct type can be mutated by a method without ever being assigned
(`self.viewport.scroll_by(..)`), so "never assigned" means nothing there; for a
`bool` it means exactly what it says.

**The fix is not the same for every one of them, and the difference matters
more than the count.** Three were verified by hand before any code was written,
and they want three different things:

| | |
|---|---|
| `apps/regextester` `i`/`g`/`m` | **a key.** The buttons are already drawn and already read; they needed a chord. Fixed. |
| `apps/spreadsheet` `show_gridlines`, `show_formula_bar`, `show_status_bar` | **a key.** View toggles in an app that already has an `F1` list to advertise them on. |
| `apps/lockscreen` `show_clock_seconds`, `show_date` | **Fixed 2026-09-27 (lane E, C-Q26): set on Settings' Screen Lock page, kept in `lockscreen.yaml`.** **a settings file, not a key.** `main` passes `LockScreenConfig::default()`, so a lock screen can never show seconds and always shows the date -- but a lock screen is a security surface where every keystroke belongs to the password field, and adding shortcuts to it would be the wrong repair. This one is blocked on where its configuration should live, which is a question for the operator rather than a line of code. |

**Down to 51 in 23 apps** as of the spreadsheet and logviewer fixes, and
`apps/passwordgen` dropped off the list entirely when its options were wired --
which is the tool tracking reality rather than a number in a file.

Two more verified by hand and worth doing next, both in apps that already carry
an `F1` list to advertise the new key on:

| | |
|---|---|
| `apps/calendar` `use_24h` | `false` at construction, read twice, no writer. **The calendar can only ever show 12-hour time.** It already has `W` for the week-start question, which is the same kind of preference, so a key is consistent. |
| `apps/hexeditor` `show_inspector` | `true` at construction, read twice, no writer. The inspector panel is permanent. |

**The survey covered only `bool` at first, and that hid the more expensive
half of one defect.** `apps/torrent`'s `sort_ascending` was on the list;
`sort_column` beside it was not, because it is an enum. It has no writer
anywhere -- declared, constructed as `Added`, read once in the comparator -- so
the torrent list sorts by date-added descending for ever and **ten of its
eleven comparator arms are unreachable**. Reporting the boolean and not the
enum is reporting the smaller half.

Fieldless enums are now included, on the same reasoning that justified
restricting to `bool`: nothing can change one in place, so "never assigned"
means "never changed". The count went to 95 in 33 apps, and the first pass of
the new rows found something larger than anything the bool-only version did:

**`apps/regextester` draws three tabs and only one can ever be shown.**
`active_tab` is `ActiveTab::Tester` at construction, matched to choose the
view, drawn to highlight the tab strip -- and written only by tests. The
Library and Reference tabs are rendered code that no user can reach. That is in
the same app whose `i`/`g`/`m` flag buttons were fixed two hours ago, which
says something about how much a single reading of one app finds: the flags were
visible because they were *drawn as controls*, and the tabs looked like they
worked because a tab strip with one tab highlighted looks exactly like a tab
strip.

`apps/editor`'s `line_ending` is on the list and is **correct**: it is detected
from the file's own content (`if content.contains("backslash-r" + "backslash-n")`)
and set at construction, which is an editor preserving what it opened. Same
false-positive mode as `apps/installer`, and the same tell -- look at where the
struct comes from.

**One app has now yielded three separate findings in three passes, and that
is the most useful thing the survey has taught me about how to use it.**
`apps/regextester`:

1. the `i`/`g`/`m` flag buttons, drawn as controls and unoperable;
2. `active_tab`, so two of its three tabs were unreachable;
3. `show_replace` and `show_groups` -- and `show_replace` is the worst of the
   three, because `Tab` cycles focus *into* the replacement field while the
   pane holding it is never drawn. The app's own shortcut list says Tab moves
   between "the pattern, the text and the replacement".

Each pass fixed what it found and moved on, and each time the app looked
finished. **The lesson is to run the survey against one app until it reports
nothing, rather than fixing its top row and going to the next app** -- a single
frozen field is rarely the only one, because whatever habit produced it
produced the others in the same sitting.

**One row is a choice rather than a defect, and the difference is worth
keeping.** `apps/explorer` threads a `ConflictPolicy` through `plan_copy` --
`Skip`, `Overwrite`, `OverwriteIfNewer`, `Rename` -- and every production call
site passes `Rename`. So three of the four are unreachable from the window and
a copy onto an existing name always renames; the user is never asked and never
offered a choice.

That is **not** the same as the frozen flags above, and saying why matters:
renaming never loses data, so the app is safe and merely inflexible, where a
frozen `show_replace` or a sort nobody can change is a control drawn and not
wired. The fix is also different in kind -- a conflict prompt is a dialog with
three buttons and a decision about what the default should be, which is a
feature to design rather than a key to bind.

Filed here so the next reader does not have to re-derive it, and not fixed,
because guessing at a destructive default is exactly the kind of choice that
should not be made by whoever happens to be passing.

> **2026-09-27 (lane E, C-Q26):** the choice is offered without guessing
> at a default -- the folder menu's *When the name is taken* (keep both,
> skip, replace if newer, replace), kept in `explorer.yaml`, with keep
> both still the default, so nothing changes for anybody who does not
> choose. The prompt this entry asks for is the one piece left, and it is
> bigger than a dialog: `ConflictPolicy::Ask` in the executor emits a
> `Conflict` event and then **skips the file** ("In a real async
> implementation the caller would respond. For now, skip."), and a move
> skips without even the event -- so it cannot be offered until the
> executor waits for an answer. `todo.txt` → *explorer: Ask when a pasted
> name is taken*.

**A second kind of noise, found by checking the two rows with the highest
stakes.** `apps/installer`'s `wipe` and `auto_reboot` look frozen and are not:
they are built from an answer file through `disk.get("wipe")` and
`root.get("auto_reboot")`, so they are set by *constructing* the struct, which
the survey deliberately does not count as a write. `apps/backup`'s
`follow_symlinks` is threaded through `scan_dir_recursive` as a parameter.

That matters more than the count, because it is **the same evidence as
`apps/lockscreen` with the opposite answer**: there, `main` passes
`LockScreenConfig::default()` and nothing parses anything, so the flags really
are fixed at compile time. Construction from a parser and construction from a
literal are indistinguishable to the tool and mean opposite things -- so the
question to ask of any row in an app that reads a config file is *where does
this struct come from*, before anything else.

I checked those two first because an installer that cannot be told whether to
wipe a disk would have been the worst finding of the night. It would also have
been wrong.

And the list's own noise is now legible enough to describe: entries like
`ctrl`, `shift`, `bold`, `expandable` and `is_directory` are **data** -- a
recorded keystroke's modifiers, a tree node's shape, a listing entry's kind --
immutable because that is what they are. They sit in the app struct because the
app struct holds a copy of the thing, not because anyone meant them to be
settings. A reader working the list should expect roughly a third of it to be
that.

**And one of them is not a flag at all -- it is two whole features.**
`apps/rssreader`'s `show_add_feed_dialog` and `show_feed_health` are `false` at
construction, written only by tests, and each gates a *render function of its
own*: `render_add_feed_dialog` and `render_feed_health_overlay`. So two
complete overlays are written, drawn conditionally, and reachable by nobody.

That one is filed rather than fixed, because the remedy depends on something
the code cannot say. `show_add_feed_dialog` looks **superseded**: adding a feed
already works through the inline `A` prompt wired on 2026-09-18, so the dialog
is a second way to do a thing that has a first way, and design-decision 1006's
rule -- a command that does not work is deleted, not kept -- argues for
removing it. `show_feed_health` looks **unfinished**: nothing else in the app
shows feed health, so wiring a key would add the feature rather than restore
it. Deleting a finished feature and shipping an unfinished one are opposite
mistakes, and the flags look identical from here.

**Both were settled on 2026-09-21, and one of the two readings above was
wrong.** `show_feed_health` is not unfinished. "Nothing else in the app shows
feed health" was a claim about the *name*: `feed.health.is_healthy()` colours
every row of the sidebar and `record_success` runs on every refresh, both in
live code -- checked with `rustlex.live_code`, which on the same day turned
out to have been blanking production code and is why the claim went unchecked
the first time. The health data is real and already on screen as a colour;
the overlay is the detail behind it. So wiring `H` **restores a finished
feature**, and that is done.

`show_add_feed_dialog` was the superseded one, and it is deleted. `A` opens
an inline add-feed prompt, so the dialog was a second way to do a thing that
has a first way, reachable by nobody. The operator's own rule for this class,
from their answers to `open-questions.md`: "Why not delete all of them that
don't work... The ones that don't work but could work later can simply be
added when we actually implement them?"

The general point survives the correction and is sharper for it: the two
flags looked identical *from the flag*. What separated them was what the rest
of the program does with the data behind each -- which is a question the
survey cannot ask and a reader can.

So the survey's output is a list of *questions about intent*, not a list of
patches. A flag frozen because nobody wired the toggle and a flag frozen
because its home is a configuration file that does not exist yet look identical
from the code and need opposite work.

It is still a candidate list. Some of the 60 are data rather than settings --
`is_directory` on a listing entry is immutable because that is what it is --
and the survey says so rather than pretending otherwise. The first run reported
**197 in 67 apps** before the app-struct restriction, and most of that was
furniture.

**A tool bug worth recording, because it is the third escape today.** That
first 197 did not change when the restriction was added, and the reason was a
literal `0x08` byte sitting in the regex where `backslash-b` should have been: the
heredoc carrying the patch collapsed one backslash level, and `backslash-b` is a
*valid* Python escape, so it became the byte it names and the pattern silently
never matched. `backslash-a` did the same thing to `known-issues.md` an hour earlier
and `backslash-w` -- being *invalid* -- merely warned. **The escapes that warn are the
harmless ones**; the dangerous ones are by definition the ones the language
handles quietly. Regexes do not go through heredocs any more.

**And the repair was worse than the bug for about ninety seconds.** Fixing the
mangled paragraph, I reached for a blanket replace across the whole file --
every occurrence of the two-character sequence, no count, no scope -- on a
document of 160,000 lines shared by three lanes. It corrupted **seven unrelated
places**: a Windows path in a kernel entry, a grep example, a cfg-matching
pattern, a regex in a layout rule. All of them pre-existing, none of them mine,
and the file had no other reader to notice.

Caught only because I listed the remaining matches instead of trusting the
count, and repaired one at a time with `git diff` as the check -- the diff is
zero deletions now, which is the property that actually proves nothing else moved.

The tool for this already existed and I bypassed it: every other edit in this
session goes through a helper that asserts the match count *before* replacing
and aborts otherwise, precisely so a broad pattern cannot quietly hit more than
it was aimed at. **A safety rail abandoned under time pressure is a safety rail
that was never there.** The rule earns its keep most exactly when the edit
feels too small to need it.

**Update 2026-09-18 — the headline number was wrong, in both directions.**
The survey's scope regex wanted `impl App for X` and every app here writes
`impl oswindow::app::App for X`, so it matched none of them: 19 of the 20
apps it had reported on were silently scanned *whole*, where a directory
entry's `is_directory` counts as a frozen setting. Two-thirds of the number
above was furniture, and the fallback that produced it was described in the
docstring as "visible in the count" while being printed nowhere.

Correcting it made the survey both shorter and longer. Scoped to the app
struct alone it read 16 in 12 apps -- and `apps/lockscreen`'s
`show_clock_seconds`, a real one already filed here, had vanished, because it
lives one field away in a `LockScreenConfig`. Following singleton-held
structs but not collection-held ones -- a type the crate ever puts in a
`Vec` is data, so `apps/chess`'s `Move::is_castling` stays out -- gives
**107 in 37 apps**, which is the first figure from this tool that means what
the entry title claims.

Four apps out of it so far:

| App | Was | Now |
|---|---|---|
| `videoplayer` | a "Player Settings" screen drawing six settings, none changeable; `repeat` read by the playlist and set by nothing | one `SettingRow` list the renderer walks and a cursor indexes; `R` and `Shift+E` for the other two |
| `passwordgen` | every passphrase capitalised and ending in a digit, for everyone, always | `Shift+C`/`Shift+D`/`Shift+S`, plus `M`; panel and handler are one list |
| `diskimager` | "verify after write" and "compress" drawn as checkboxes with no writer | `V` and `C` |
| `torrent`, `email`, `regextester` | see their own entries above | fixed earlier the same day |

**Closed 2026-09-21 at nought open.** The survey reports
`0 field(s) in 0 app(s)`, out of 888 scanned, with 52 rows recorded in
`scripts/frozen-flag-answered.txt` as looked-at-and-not-defects and the rest
fixed. The apps that got a key or a control out of this, in order:
`regextester`, `torrent`, `email`, `diskimager`, `videoplayer`,
`passwordgen`, `filesearch`, `credmanager`, `screenrecorder`,
`archivemanager`, `systemrestore`, `hexeditor`, `filediff`, `qrcode`,
`ircclient`, `connect4`, `remotedesktop`, `photomanager`, `spreadsheet`,
`explorer`, `imageviewer`, `diagram`, `netscan`, `rssreader`.

**A zero is the easiest number to get wrong**, because a survey that has
stopped working reports it too. This one was checked by putting a defect
back: commenting out the writer `H` gained in `apps/diskimager` returns
`1 field(s) in 1 app(s)  diskimager  hash_algorithm`, and restoring it
returns nought. The count is of a live instrument.

**What the answers file is for, and what it is not.** 52 rows say "looked
at, not a defect" with a reason that has to state *what makes it so* --
a measurement computed once, a setting whose feature says on screen that it
does not exist, a decision with the operator. A line naming a field the
survey no longer reports fails the run, so an answer cannot quietly outlive
the code it was about. That check fired once already, on six `mediaconvert`
rows that were answering a question the tool had got wrong.

**The lesson is the one this file keeps recording, for the third time in a
day: a checker whose population is defined by a name it expects will one day
be handed a different name and say nothing.** `key-survey.py` matched
`SHORTCUTS` and missed `HELP_ROWS`; `check-variant-lists.py` checks lists
*named* `ALL`; this one wanted a bare trait name. Each was silent, and each
reported a confident number while doing so. The cure that worked here was
not a better regex -- it was **printing the count of things the tool could
not scope**, so a population that collapses says so out loud.
