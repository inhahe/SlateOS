### [E] A key held with Alt or the Windows key works a program's bare-key binding -- 2026-09-29

**Status:** pending a boot test on main. Every program on the list below is
done (2026-09-29 to 10-03), each with a test and mutation rows: stopwatch, hangman, asteroids,
battleship, benchmark, calendar, clipmanager, compass, contacts, credmanager,
crossword, dbviewer, musicplayer, defrag, devicemanager, diskimager, dots,
ebook, email, explorer, fileassoc, filediff, finance, flashcards, jsonviewer,
kanban, launcher, match3, mediaconvert, metronome, pacman, pdfviewer, pinball,
podcast, regextester, reminders, remotedesktop, reversi, rssreader,
screenrecorder, screenshot, simon, snippets, soundrecorder, speedtest,
startupmanager, sysmonitor, systemrestore, terminal, tmux, torrent,
typingtutor, undelete, videoplayer, weather, wordle, wordsearch, worldclock,
yahtzee and settings; nonogram was already guarded, and paint and the
whiteboard now ask `textline`'s predicates rather than their own copies.
The shared one-line and multi-line fields (`apps/textline`, `apps/textarea`)
answered Alt's and the Windows key's chords on their editing keys --
Alt+Backspace deleted, Windows+Left moved the caret -- though their docs
said those keys were the application's; fixed (bca1a40d6), all 73 programs
using them tested. **Fixed on lane E, pending a boot test on main; then it
moves to `known-issues-resolved/`.**

Most programs had the AltGr fault too -- a Ctrl shortcut matched on Ctrl
held, so AltGr+S, a Polish `ś`, saved -- and many typed a command's letter
into a field; both are fixed in the same pass. Found on the way and fixed:
dbviewer's and musicplayer's F1 list never showed (its release toggled it
off again); the terminal sent nothing for AltGr's characters (no `@`, `{`
or `|` at a German prompt) or for Alt and a letter (readline's meta keys);
screenrecorder's volume keys moved the history's selection in the history
view; settings' search could not be left with Escape, and Ctrl+F typed an
`f` into the exclusion pattern. Found and logged: the toolkit's dialogs
answer a chorded Enter or Space
(`requests/e-c-the-toolkit-dialogs-answer-a-chorded-enter-space-and-escape.md`),
and the screenshot tool's text annotation cannot take a digit
(`E-the-screenshot-tools-text-annotation-cannot-take-a-digit-and-escape-throws-the-picture-away.md`).

**In short:** Many programs act on a plain letter: N for a new game, S for
stop, a letter guessed in hangman. The compositor hands a chord its letter as
text, and most of these programs never ask what was held with the key, so
Alt+N, meant for the window, or Windows+S, meant for the desktop, does what N
or S does. Paint's Alt+B chose the pencil; the whiteboard's Alt+R the
rectangle.

**Reproduce:** in hangman, press Alt+A: the letter A is guessed (so is it
with Ctrl+A). In the stopwatch, Alt+R resets the running watch, as R does.

**Where.** A survey of 2026-09-29 (a letter or digit matched with no
modifier on its line or the three above, in a program whose code never reads
`super_key` directly or through a helper) finds 60: asteroids, battleship,
benchmark, calendar, clipmanager, compass, contacts, credmanager, crossword,
dbviewer, defrag, devicemanager, diskimager, dots, ebook, email, explorer,
fileassoc, filediff, finance, flashcards, hangman, jsonviewer, kanban,
launcher, match3, mediaconvert, metronome, musicplayer, nonogram, pacman,
pdfviewer, pinball, podcast, regextester, reminders, remotedesktop, reversi,
rssreader, screenrecorder, screenshot, simon, snippets, soundrecorder,
speedtest, startupmanager, stopwatch, sysmonitor, systemrestore, terminal,
tmux, torrent, typingtutor, undelete, videoplayer, weather, wordle,
wordsearch, worldclock, yahtzee. A heuristic both ways: each is to be read,
and some will turn out to match only Ctrl chords (that is the separate
AltGr item, a Ctrl shortcut matched on Ctrl alone). Done before it: the ten
games of the redo-tree pass, paint and the whiteboard (2026-09-29).

**The fix**, per program, after the program's own chords (Ctrl+S, Alt+Z,
Alt+Left) and before its bare bindings -- one of two rules, by what the
binding matches:

- **On the key** (`Key::R => reset`): only a plain key, nothing held but
  Shift -- `textline::is_plain`. AltGr+R types a character on some layouts,
  or none, and is not R.
- **On the letter typed** (hangman's guess, paint's tool letters): not a
  command -- `!textline::is_command`, which is Ctrl or Alt alone or
  anything with the Windows key. AltGr is not a command and counts by what
  it types.

A test per program presses a bound key with Alt and with the Windows key and
finds nothing changed. The terminal is the exception: Alt+letter is the
shell's (sent as Escape and the letter) and only the Windows key's chords
stay out.
