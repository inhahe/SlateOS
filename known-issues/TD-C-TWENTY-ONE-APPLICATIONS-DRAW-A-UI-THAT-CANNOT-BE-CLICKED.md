## `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED` (lane C, 2026-09-17) -- **ALL EXAMINED 2026-09-25 (lane E)**

**In short:** twenty-one applications draw a graphical interface and handle no
mouse events at all. Not "handle clicks badly" -- they never receive one:
`Event::Mouse` appears nowhere in them, they import no `MouseEvent`, and they
have no hit-test function. Whatever they draw, the pointer does nothing over
any of it.

**Measured across all 139 apps with a `main.rs`:**

| | count |
|---|---|
| handle no mouse event of any kind | 24 |
| ...of those, not GUI applications at all (`installer`, `indexer`, `backup` -- no `App` impl, zero `RenderCommand`) | 3 |
| **draw a GUI and cannot be clicked** | **21** |

    weather 96   rssreader 95   reminders 82   markdowneditor 79
    habits 70    pinball 68     notes 65       slides 61
    flashcards 61 finance 58    logviewer 51   qrcode 48
    mediaconvert 44 tmux 43     regextester 43 torrent 41
    soundrecorder 39 renamer 37 email 36       metronome 24
    filesearch 23

(the number is `RenderCommand::` sites, as a rough measure of how much
interface each one draws)

**Verified in depth for exactly one.** `apps/notes` is written up in the entry
above: three panels drawn, none clickable, and nine operations -- delete a
note, tag one, rename a notebook, restore a version -- that have neither a
keyboard shortcut nor a click, because every route to them was a click that
never arrives. The other twenty are *candidates measured the same way*, not
confirmed defects, and the check for each is the one this file keeps having to
repeat: does the thing it draws look like something you would click?

**Two are plausibly legitimate** and should be read before being counted.
`tmux` is a terminal multiplexer and `pinball` is a game; both have a case for
being keyboard-driven by design. The case has to be *in the file*, though --
`notes` had no such comment, advertised a "Multi-panel UI" in its module doc,
and had three keyboard shortcuts in the whole of its production code, which is
not a keyboard-driven application either.

**UPDATE 2026-09-18: a second one examined, and it is a different case.**
`apps/reminders` is on this list and is *not* `notes`. It binds fifteen keys,
its number keys are view filters, `Space`/`Enter` completes the selected
reminder and `Escape` dismisses notifications -- so it is a genuinely
keyboard-driven program that happens also to take no pointer. Its defects were
specific rather than wholesale: snoozing could not be reached at all (the one
thing a reminder app is for besides listing), and `last_file_action` recorded
what every save did and was drawn nowhere. Both are fixed; neither needed a
pointer layer.

**So the number in this entry is "applications that draw and take no
pointer", and it is not a defect count.** Two examined so far:

| | finding |
|---|---|
| `notes` | wholesale -- a mouse-shaped UI with three shortcuts, nine operations unreachable. A pointer layer was the repair. |
| `reminders` | specific -- keyboard-driven by construction, two unreachable things, no pointer layer needed. |

Nineteen unexamined. The question to ask of each is not "does it handle a
click" but **"can everything it offers be reached by something"** -- which is
a different question, and the reason the first is only a way of finding
candidates for the second.

**Why this is worth a single entry rather than twenty-one.** The repair is the
same shape every time and it is not "add a click handler": it is hit-tests
derived from the same functions the renderer already reads, so the law a click
obeys and the law the drawing obeys cannot drift. `apps/photomanager` and
`apps/explorer` both do it that way and are worth copying. The failure this
prevents is the one `photomanager`'s search box had for its whole existence --
a control drawn at coordinates the click handler had never heard of.

**The measurement, which is one command:**

    grep -c "Event::Mouse\|MouseEvent" apps/*/src/main.rs

**UPDATE 2026-09-25 (lane E): the list is lane E's now, and a third is
examined -- `apps/markdowneditor`, a wholesale case like `notes`, and a worse
one.** Recounted over each app's whole `src/` (every one is a single `main.rs`),
twenty apps still take no pointer: weather, reminders, rssreader,
markdowneditor, habits, pinball, slides, flashcards, finance, logviewer,
qrcode, torrent, mediaconvert, regextester, tmux, soundrecorder, renamer,
email, filesearch, metronome. `roadmap.md` → Lane E tracks them.

The markdown editor drew a toolbar, a tab bar, a table of contents, a find
panel and three dialogs, and its module doc called the contents "clickable".
Asking the reachability question of each thing it offered found that the
pointer was the smaller half of it:

| | before | now |
|---|---|---|
| view mode, contents, templates, Save As | no key, no pointer: **unreachable** | a toolbar button each and a key each |
| switching or closing a tab | no key, no pointer: a document once open stayed open, behind whichever was newest | tabs and close buttons answer; Ctrl+Tab, Ctrl+W; a `»` menu when the tabs overflow |
| HTML export | computed the HTML and dropped it (`let _ = html;`) | writes it where the picker says |
| a file changed on disk | `check_external_change` had no caller -- the prompt, merge and review were never raised | checked on focus and on tab switch; every answer a button and a key; a deleted file no longer offered a "Reload" that did nothing |
| selection | the anchor had four writers that cleared it and none that set it | drag, Shift+press, Shift+arrows, Ctrl+A; typing replaces it; cut, copy, paste |
| auto-save | no off switch | the status-bar label is one |
| a failed Save As | reported only to a field nothing drew | sticky and red, like any failed save |

Found on the way and fixed: undoing a delete that spanned lines put the
newlines *inside* one line; the delete recorded an unclamped column; a heading
button stacked `## ## `; a toolbar separator was a `NewFile` button in
disguise; a narrow window drew its last toolbar buttons off its own edge. The
close-button dialog is in and tested, and cannot take effect yet -- see the
[E] entry on closing over unsaved work.

Three examined: `notes` (wholesale), `reminders` (keyboard-driven by
construction, two specific gaps), `markdowneditor` (wholesale, plus five
operations nothing reached by any route). Seventeen to go.

**`apps/filesearch`, 2026-09-25 -- wholesale, and the search could not use
what it found.** Enter was on the F1 card as "Open what is selected" and re-ran
the search; the preview's four action buttons were wired to nothing; the
results stopped at the panel's edge with no way to scroll, while the keyboard
could select rows that were never drawn; the filters panel ran under the status
bar; every keystroke was recorded into a search history nothing drew, and
saving a search had no caller. Now: every control answers the pointer; Enter,
Open and Ctrl+L open a result with the program File Associations names or its
folder in the file manager; the results and the filters scroll; recent searches
(the ones something was opened from) and saved searches (which outlive the
window) are listed and run again on a press. Copy Path and Properties were
removed rather than wired: the first needs an application-reachable clipboard
that can carry a path (the explorer's clipboard entry), and the second was the
preview pane itself. Also fixed: "now" was the constant `1_779_000_000`, so the
date filters and every "3 hours ago" were measured from one day in May 2026,
and the Content mode matched names (its own [E] entry, fixed the same day).
Four examined; sixteen to go.

**`apps/renamer`, 2026-09-25 -- wholesale, and most of its purpose was
unreachable.** Beyond the missing pointer, every rule that needed a string typed
or a choice made could not be added (its own entry,
`TD-C-RENAMER-CAN-ONLY-ADD-THE-RULES-THAT-NEED-NO-TYPING`, now fixed), only the
newest rule could be selected, the file list and the extension filter had no
way to scroll or be typed into, and the layout ignored the window's size. All
reachable now, by pointer and by key. Five examined; fifteen to go.

**`apps/logviewer`, 2026-09-25 -- wholesale, and there was no log.** Beyond the
missing pointer, the viewer read no file: its entries were a string compiled
into it (`TD-C-LOGVIEWER-TAILS-A-STRING-COMPILED-INTO-ITSELF`, now fixed), so
its tailing, its follow switch and its "export filtered view" were words. The
list could not be scrolled at all -- `scroll_offset` was written by nothing, and
the keyboard walked the selection off the bottom of the window -- N and P moved
a counter in the filter bar and not the selection, the search was a substring
under a doc comment promising regular expressions, the time range could not be
set by any route, a source filter could be cleared only by clearing everything,
the detail view cut its message and raw line to one elided line each, and
unbound chords ran the bare key under them (`Ctrl+D` raised the level floor).
Now it opens the system journal at start and any log by Open or Ctrl+O, follows
what is written with a line caught half-written becoming one entry, reads a
rotated log again, exports the entries shown as the file's own bytes, searches
by POSIX ERE (the engine `grep -E` uses) on Ctrl+R, and every control answers
the pointer. Following now stops when the reader selects an earlier entry, since
on a busy log the next line would otherwise drag the selection away before it
could be read. Six examined; fourteen to go.

**`apps/regextester`, 2026-09-25 -- wholesale, and half of it was out of
reach.** Beyond the missing pointer: the test input could not take a newline
(Enter's text is a control character and the typing path dropped them all), so
the multiline flag had nothing to act on; every field could be edited only at
its end; nothing scrolled (`scroll_offset` and `match_scroll_offset` were read
and never written), so a long text, the matches past the first screenful, half
the library and the end of the reference could not be seen; the Groups and
Explain sub-tabs were drawn with nothing behind them and the breakdown was cut
at six lines; a library pattern could not be loaded and none could be saved
(`load_library_entry` and `save_to_library` had no caller -- `todo.txt`'s
entry, now done); and typing on the Library tab went into a pattern nobody
could see. Also fixed: match highlights were placed by treating a character
index as a byte offset (wrong letters after any accent), the replacement's
result was painted with the input's highlights, and each match attempt copied
the whole input into a new character vector (a thousand matches over the
16 384-character limit copied sixteen million characters per keystroke). Now
the test input is a real multi-line field (caret, selection, clipboard, drag),
the library saves patterns with their flags to the user's settings, and every
control answers the pointer. Seven examined; fourteen to go -- the counts in
the paragraphs above are each one short, because the list is twenty apps
*besides* `notes`, which was the first examined.

**`apps/weather`, 2026-09-25 -- a pointer layer, and three things that failed
only with nothing fetched, which is every real run.** The app has no data
source and says so; that part was right. But with nothing fetched F1 raised a
shortcut card that was never drawn -- the draw returned before reaching it --
while the card, being modal, swallowed every key, so the window looked frozen
until F1 or Escape happened to be pressed. The Settings tab showed the same
"cannot fetch" notice as every other tab, so U, W, P and T changed units
nobody could see; and the units were forgotten at every start. Now the card
and the settings draw whatever was fetched, a settings row changes its value
on a press, the units are kept in the user's settings, the six tabs, the
places and the hourly strip (under the wheel, either way) answer the pointer,
and the update interval -- shown as "30 min", unchangeable, with nothing to
refresh -- is absent until something refreshes, as
`TD-C-WEATHER-HAS-A-REFRESH-INTERVAL-AND-NOTHING-TO-REFRESH` said it should be.
Eight examined; thirteen to go.

**`apps/rssreader`, 2026-09-25 -- a pointer layer, three panes that could not
scroll, and a notice nobody could see.** The reader is keyboard-rich and
honest about not fetching -- but its "cannot fetch" lines were drawn at the
top of the window before the title bar, which painted over them, while the
title bar offered a "Refresh All" button for a refresh nothing here can do.
None of the three panes scrolled (`article_scroll_offset` and
`sidebar_scroll_offset` were read and never written, and the article's offset
only ever reset), so the selection walked off the list after nine articles and
a long article was cut. Feed discovery (`discover_feeds`) was written, tested
and called by nothing. Now the notice is where the articles would be and names
the one way to get some (open a downloaded feed file); Open… and Export…
replace the dead button; every row, dot, star, badge, button and prompt
answers the pointer; the panes scroll under the wheel and follow the
selection, and Page Down reads on; a saved web page opened with Open…
subscribes to the feeds it links to. The reader still forgets everything at
exit -- its own entry below. Nine examined; twelve to go.

**`apps/habits`, 2026-09-25 -- the clock's day, a record that is kept, a
pointer layer, and two lists that stopped at the edge.** "Today" was 18 May
2026 in every run, moved only by `+` and `-`, so a check-in made today was
filed in May; it is the clock's now, and rolls over at midnight while the
window is open (design-decisions 1201). Nothing was kept -- the notice said
so, from under the header that painted over it -- and every habit and
check-in is now kept in the user's settings (`habits.yaml`, one entry per
habit; an entry that cannot be read is skipped and left in the file). Ctrl+D
deleted a habit and its whole record without asking; it asks now, on a card
whose buttons answer the pointer, and an archived habit can be deleted from
its row. Every tab, button, row, day cell and form control answers the
pointer, and F1 lists the keys (the form's frequency moved from F1 to F2, its
weekly count to Up and Down). The archive and the statistics table stopped
drawing at the window's bottom edge; both scroll now, the dashboard follows
its selection, Page Down pages, and the number keys start a screen at its top
as a tab does. And a space could not be typed into a habit's name. 167 tests;
`apps/habits/mutate.py` has 55 rows. Ten examined; eleven to go.

**`apps/slides`, 2026-09-25 -- a pointer layer, and an editor that could not
change most of a slide, keep a deck, or show one.** Nothing selected an
element but adding one, so nothing a layout put on a slide -- the first
slide's title included -- could be selected, typed into, moved or deleted, and
only text boxes could be typed into at all (every Title + Content slide said
"First point" for good). Undo and redo had no key, and the toolbar's Undo and
Redo lit up and could not be pressed. The notes were shown and exported and
could not be written. Ctrl+N made one layout and nothing made the other five.
Nothing could move or resize an element. The thumbnail column and the sorter
clipped and never scrolled. `set_theme` said it restyled every slide and
restyled none, so the Light theme put pale text on a pale ground. There was
no save and no open -- a deck lived as long as the window -- and no show: the
transition every slide chose was stored, printed and exported as nothing. And
the export's result message was drawn nowhere, so a failed export failed in
silence. Now: Tab selects, a press selects and a second press types, a drag
moves and corner handles resize, the arrows nudge; every toolbar and panel
button works and the panel's values change; Ctrl+M and + Slide reach every
layout; the thumbnails scroll, follow and reorder by drag; a theme change
restyles what came from the theme; decks save and open as YAML (`.slides`,
versioned), with a question before Open loses unsaved work and a `*` in the
window bar; F5 presents, with each transition played -- in the export too, and
lines there run in their own direction instead of flat. 146 tests;
`apps/slides/mutate.py` has 61 rows. Eleven examined; ten to go.

**`apps/flashcards`, 2026-09-25 -- a card editor nothing could type into,
decks that could not be named, and a spaced-repetition program that forgot
its reviews.** The card editor's keys were Enter and Escape, so no card could
be made or changed (the tests set the strings directly); `n` made a deck
called "New Deck" that nothing could rename; a delete took a deck or a card
and its whole history at once. Nothing answered the pointer, the deck list
had no scrolling, the card list showed eight rows at any height, the deck
view's hint line advertised a `[D]ay+` key removed when the day came from the
clock, a long answer was cut on the study card, and the notice was drawn
under the header. Nothing was kept: Ctrl+S wrote one deck to a file the user
had to open again at every start, and a review not saved that way was gone at
close. Now the fields type (TextInput, Tab between them), decks are named and
renamed, deletes ask, every button, row, field and rating answers the
pointer, both lists scroll and fit the window, F1 lists the keys, answers
wrap, and every deck and review is kept as it changes (one file per deck under
the settings directory, opt-in so no test can write the developer's own).
198 tests; `apps/flashcards/mutate.py` has 38 rows. Twelve examined; nine to
go.

**`apps/finance`, 2026-09-25 -- a budget tracker nothing could be entered
into, that kept nothing, and whose today was a constant.** `add_account`,
`add_transaction` and `set_budget` had no caller outside the tests; the
notice saying so was drawn at the top of the window and then painted over by
the sidebar and the header. Nothing was kept. "Today" was 18 May 2026 in
every run. Nothing answered the pointer. The transaction list showed every
month in the order things were typed, under a header naming one month whose
arrows changed nothing in it, and it did not scroll -- rows past the bottom
were not drawn, and the arrow keys stopped at the last one on screen. The
budgets screen listed only budgets already set, so none could be set. Ctrl+D
deleted at once, from any screen, including a transaction chosen on another
one. Now: forms enter and change accounts, transactions and budgets (typed
fields, Tab between them, choices stepped by the keys or their arrows, and a
refusal that says why and keeps what was typed); deletes ask, on the screen
that shows what goes; the list is the header's month, newest first, and a
search reaches every month; every list scrolls and keeps its choice in view;
the sidebar, month arrows, search box, category chip, buttons, rows, fields
and question all answer the pointer; F1 lists the keys; today and midnight
come from the clock; and the ledger is kept as it changes (a text file under
the settings directory, refused whole and left alone if it cannot be read
whole -- `design-decisions.md` §1202). 105 tests; `apps/finance/mutate.py`
has 49 rows. Thirteen examined; eight to go.

**`apps/qrcode`, 2026-09-25 -- a generator whose codes could not be kept,
whose history was keystrokes, and whose symbols a scanner could not always
read.** Nothing answered the pointer; the toolbar's Generate button did
nothing (typing already generates) and nothing else could keep a code; the
colour swatches could not be changed; the one box took typing at its end and
nothing else; the history gained an entry per keystroke ("H", "He", "Hel"...)
and a press on one did nothing; emptying the box left the old code on screen;
an empty web-address box encoded `https://`; a barcode silently dropped what
Code B cannot hold ("Café" scanned as "Caf" under a label reading
"Café"); WiFi security was Ctrl+S, which everywhere else saves; and the
window opened on a code and a history entry for "Hello, Slate OS!". Reading
the symbols back as a scanner does found worse: **versions 7-10 had no version
information**, so their data sat where the version belongs and every later bit
was misplaced (anything past about 120 bytes -- most contacts); **version 10's
alignment centres were 52 instead of 50**; the second format copy never wrote
its bit 7; and **the Code128 table was corrupt from value 60** (entries
marked "placeholder", one with a zero-width space, several duplicates), so no
barcode with a lowercase letter could be read. Now: every control answers the
pointer; the boxes are text fields (caret, selection, copy, cut, paste); one
history entry per code, which a press brings back; SVG saving at the module
size, in the chosen colours (PNG waits on
`requests/e-f-a-png-encoder-applications-can-save-with.md`); colours chosen in
the toolkit's colour dialog, with a warning when scanners may fail; the
preview shrinks to fit; and a test reads every version back through a layout
written again from the standard, and checks the Code128 table's rules and a
sample of the published one. 114 tests; `apps/qrcode/mutate.py` has 41 rows.
Fourteen examined; seven to go.

**`apps/torrent`, 2026-09-25 -- a client that says it cannot transfer, said
so where nobody could read it, and offered controls nothing could press.**
The three-line notice was drawn at the top of the window and then painted
over by the background and the header. Nothing answered the pointer: six
toolbar buttons, seven filters, five labels, six tabs and every row. The
transfer list did not scroll -- rows past the bottom were not drawn -- and Up
and Down walked the transfers in the order they were added, not the order on
screen, so with a sort or a filter on they jumped about and onto hidden rows.
A label could be neither given nor chosen by; the search had a query and no
way to type one; `add_magnet` had no caller and the dialog fields that would
have fed it were written and never read; a file set to Skip was downloaded all
the same, because nothing carried a file's priority to the pieces the picker
reads; and a transfer set going asked for a tick every 150 ms for good, with no
network to bring it a peer. Now: every button, filter, label, tab, column head
(a press sorts, a second reverses) and row (a second press shows the details)
answers the pointer; the list scrolls and follows the selection; a magnet
dialog (Ctrl+U) that says what is wrong with a bad link; a search box (`/`);
labels given with L or a press in the details; file priorities that set the
pieces' (a piece shared by a skipped file and a wanted one is still fetched);
"No network" in place of "Downloading" at 0% for good; and no clock without a
peer. 103 tests; `apps/torrent/mutate.py` has 25 rows. The transfer itself is
its own entry below. Fifteen examined; six to go.

**`apps/mediaconvert`, 2026-09-25 -- a converter that could not be given a
file or start a job.** Nothing added a source -- `add_source` had no caller
outside the tests -- and `start_next_job` refused every job, so the queue could
only ever hold a plan for files it had never seen; the notice saying so was
drawn at the top of the window, under the toolbar that painted over it.
Nothing answered the pointer. Of the settings drawn, only the profile and the
quality preset could be changed: the sample rate, channels, sample format,
picture size and naming rule were drawn and fixed, and every output was bound
for `/home/converted`, a folder nothing makes, with no way to choose another.
A `{date}` in a naming pattern was always `20260518`. And nothing in the tree
decodes MP3, FLAC, AAC, Vorbis, Opus or any video, or encodes PNG, JPEG, GIF
or WebP. Now: files are added with Ctrl+O and folders with Ctrl+Shift+O, and
each is read for what it is (its size; a WAV's length and format; a picture's
dimensions); jobs run one at a time on a worker thread, with a progress bar
that moves and a cancel that stops before anything is written; every setting
is walked with Up and Down and changed with Left and Right or a press; outputs
go beside their sources or to a folder chosen in the dialog (`B`, `O`); a job
is chosen by the arrows or a press, and Delete acts on that one; and nothing
is written over -- a name on disk, the source's own, or one another job has
planned gets " (2)", and a file that takes the name while the job waits is
left alone (`safeio::write_new_atomically`, new: the finished file is linked
into place, so the check and the claim are one operation). It converts WAV
to WAV at any rate, channel count and sample format (`apps/wavpcm`, new: every
PCM and float WAV read, windowed-sinc resampling, the standard channel mixes,
TPDF dither) and PNG or JPEG to BMP, at their own size or fitted inside one.
Every other profile is listed as "Not available", saying which half --
decoder or encoder -- is missing, and is refused before it is queued. On the
way: with a file chosen, the source list could not be scrolled, because every
frame scrolled it back to the choice. 89 tests, 14 in `wavpcm` and six new in
`safeio`; `apps/mediaconvert/mutate.py` has 29 rows, `apps/wavpcm/mutate.py`
21 and `apps/safeio/mutate.py` 6. (A first cut read a WAV's length from the
megabyte it reads for the header, so anything longer showed the wrong length;
`wavpcm::parse_header_prefix` measures against the file.) The music player and the sound recorder
read and write WAV with their own code; moving them onto `wavpcm` is part of
examining the recorder, next. Sixteen examined; five to go.

**`apps/soundrecorder`, 2026-09-25 -- a recorder that can neither record nor
play here, and could reach nothing after a take.** No application can open a
capture device -- the kernel's ALSA capture node hands back silence, because
the mixer behind it has no input -- and nothing gives an application a way to
play sound; the window said the first and not the second. Nothing answered the
pointer. Everything after a take was out of reach even in principle: Save added
a history entry naming `/recordings/<name>`, a file nothing wrote; trim handles
and a playback bar were drawn over a take that could not exist; and no
recording already on disk could be opened, so on this system the program had
no use at all. Now: the recordings folder (`~/Recordings`, or one chosen for
the session) is listed with each file's length and format, read by `wavpcm`
from its first megabyte; any WAV opens -- from the list, or from anywhere with
Ctrl+O -- as the waveform of the whole file, drawn from its stored samples;
markers are put down (M or a press), named (F2), dragged, removed (Delete) and
saved into the file as the `cue ` and `labl` chunks sound editors share -- but
not over a file another program has changed since it was opened; a stretch is
kept by setting its start and end (`[`, `]`, or dragging the handles) and saved
as a new file, its samples copied as stored and its markers moved to its start,
never over the recording itself; and unsaved markers are not left behind by one
press on another file. Record and Play say why they cannot, by key and by
press. The take is kept for the day a capture source exists, and Stop now
saves it -- samples exactly as captured (the recorder's own WAV writer, and its
reader that took only a 44-byte header, are gone for `wavpcm`), markers as cue
points -- as a new file named for the moment, never over another, and opens
it. 129 tests; `apps/soundrecorder/mutate.py` has 27 rows. The missing sound
path for applications is its own entry below. Seventeen examined; four to go.

**`apps/metronome`, 2026-09-25 -- a silent metronome that never said so.** It
cannot make a sound (no application can, the [E] entry below) and drew nothing
to say it, so a user who started it and heard nothing would look for a muted
speaker. Nothing answered the pointer: not the tempo, not the beats, not the
practice settings. Practice mode always began at 80 BPM -- `practice_start_bpm`
had no writer -- under a panel whose other values could be changed, and its
keys worked only once practice mode was already on. The beats were accented
with the digits, so beats ten to twelve of a 12/8 measure could not be. Now:
the window says the beat is shown, not heard; every control answers the
pointer (tempo steps of one and ten, the wheel over the tempo, tap and forget,
time signature, subdivision, a press on any beat to accent it, start, reset,
practice, settings); the practice settings are rows -- practice itself, the
start tempo, the target, the step, the measures -- that Up and Down walk and
Left and Right change, set before practice starts, with a start above the
target pulling it up; and F1 or `?` lists every key. 82 tests;
`apps/metronome/mutate.py` has 15 rows. Eighteen examined; three to go.

**`apps/email`, 2026-09-25 -- a mail client with no mail it could show, and a
compose form it never drew.** It cannot send or receive (no network, no TLS),
which it said -- in a notice the header then painted over. Worse, Ctrl+N opened
a compose panel that nothing drew: every key typed after it went into a draft
no one could see, and Escape threw it away. Nothing answered the pointer. The
reading pane showed a message's one-line preview as the message; "below", one
of the three pane positions Ctrl+P steps through, drew no pane at all. And no
message could ever be in the window, from anywhere. Reading its parser to give
it some found more: it took only text and read a part only if it was UTF-8;
encoded words stayed `=?UTF-8?B?...?=` in subjects and names; an address list
split at every comma, so `"Doe, Jane"` became two broken addresses; parameters
split at every semicolon; attachments named only in `Content-Type` were not
attachments; an HTML-only message showed nothing; and **the builder declared
its body quoted-printable and wrote it raw**, so `x=41` read back as `xA`,
while non-ASCII subjects and file names went into the headers as raw UTF-8.
Now: it reads mail kept in files -- `~/Mail`'s mbox files (Thunderbird's
extensionless folders too) and folders of `.eml` messages, and any file opened
with Ctrl+O -- through a reader that decodes charsets, RFC 2047 words, RFC 2231
names, HTML as text and mbox quoting (`apps/email/src/decode.rs`); a message
is shown whole, with its attachments saved where the dialog says; marks (read,
flagged) are kept in a file of its own under the configuration directory,
because **it never rewrites mail it did not make**, and deleting such mail is
refused with the reason; the compose form is drawn, with real text fields and a
many-line body (`apps/textarea`, shared with the regular-expression tester),
Tab between From, To, Cc, Subject and the body, attachments added and taken
off, a draft saved to `~/Mail/Drafts` and opened again to go on writing, Save
as file for an `.eml` to send from elsewhere, Send checking the message and
saying plainly it cannot be sent, and closing over unsaved work asking first;
and every control answers the pointer, the list and the message scroll, and
all three pane positions draw. A message written here reads back exactly as
written, which a test checks. 109 tests; `apps/email/mutate.py` has 35 rows
over the window, the codings and the store. Nineteen examined; two to go.

**`apps/tmux`, 2026-09-25 -- a multiplexer of terminals with nothing in them.**
Every pane held a banner saying the system had no PTY layer, long after
`apps/terminal` had a shell on a kernel pseudo-terminal; typing into a pane went
nowhere, because every key that was not a multiplexer command was dropped; and
nothing answered the pointer. Its own ANSI parser -- no scroll regions, no
alternate screen, no cursor-key modes -- could not have drawn a full-screen
program anyway. Reading it for the rework found more that was false:
`:split-window -h` and the `even-horizontal` layout each did the opposite of
tmux's; `prefix +`, "grow the pane", shrank every pane on the right or at the
bottom; `}`, "swap this pane with the next", only moved the focus; `;`, "the
pane you were in before", was the previous pane in order; `prefix 1` went to
the window labelled 2 once a window had closed; the status bar's clock counted
the seconds the window had been open and printed them as the time of day; a
refused window or split left an orphan pane behind; a pane too small to halve
was split into two that overlapped; a chooser drew all sixty-four sessions at
the same pitch, most of them below its box; and the detached screen said to type
`tmux attach`, a command that exists nowhere. Now every pane is an
`apps/terminal` terminal -- the terminal is a library as well as a program for
this -- running the user's shell: keys go to the active pane's shell, output is
read on the tick (from background windows and detached sessions too), a pane's
size reaches its shell whenever the layout changes, a shell that exits cleanly
takes its pane with it and one that fails leaves the pane to say how, and a
closed pane hangs its shell up. Closing a pane or a window asks first, as tmux
does. The pointer reaches everything: tabs, a new-window button, each pane's
title, grid and scrollback bar (a drag selects, the wheel scrolls the pane under
it), the status bar's session and windows, the choosers' rows, the detached
screen's Attach and Sessions, the question's Yes and No, and an F1 Keys button.
Copy mode has the keyboard while it is on, marks whole lines that the terminal
highlights, and copies the pointer's selection too; a paste goes to the program
as a paste, fenced when it asked for bracketed paste. The clock is the wall
clock, in UTC and said so. Putting the terminal's emulator in panes that are
resized on every split found five bugs in *it*, all fixed: shrinking the grid
pushed a shell's prompt into the scrollback; the hidden screen under a
full-screen program lost its prompt on a resize; one saved-cursor slot served
both screens; `reset` (`ESC c`) dropped the link to the shell and so killed it;
and its grid was drawn in the proportional face at a guessed cell size. 62
tests in tmux (91 in the terminal); `apps/tmux/mutate.py` has 42 rows and
`apps/terminal/mutate.py` 90. Sessions end with the window -- see `[E] tmux is
not a server`. Twenty examined; one to go.

**`apps/pinball`, 2026-09-25 -- the last, and the case for "keyboard by design"
did not hold.** It is a game played with the keys, and could have been left
so -- but its table, its plunger lane and its New game were all drawn for a
pointer that did nothing, and reading it found false features besides.
"Tilt" was pressing the flippers fifteen times in a second, which killed the
flippers for three seconds: the penalty fell on playing well, and there was no
way to shove the table at all. The high-score table was five scores -- 10000
down to 1000 -- that nobody had made, and nothing was kept when the window
closed. N threw away a game in progress on one key. The flippers moved only
inside the physics step, so they did not move while a ball waited in the
plunger lane. The game ran on while its window was behind another, and a
flipper held when the window lost the keyboard stayed up for good. And it asked
for a tick sixty times a second whether or not anything was moving. Now: the
pointer plays the whole game -- hold on the table's left or right half for that
flipper, hold on the plunger lane to pull and let go to launch -- and the
sidebar has New game, Pause, Nudge and Keys (F1, which lists every key); Up
nudges the table and shoves the ball, and the third nudge in five seconds tilts
it: dead flippers and no scoring until the ball drains; high scores are real,
dated, and kept in `pinball/high-scores.txt` under the settings directory -- a
file that cannot be read whole is left alone and the sidebar says why; N during
a game asks first and says what is lost; losing the keyboard pauses the game
and lets go of everything held; and the clock runs only while something moves.
132 tests; `apps/pinball/mutate.py` has 31 rows. **All twenty-one examined** --
two (`reminders`, `notes`) before the list was lane E's, nineteen since.
