### [E] A press goes through the shortcut card to the control drawn under it -- 2026-10-03

**Status:** OPEN

**In short:** in most applications the F1 shortcut card is modal for the
keys but not for the pointer. With the card up, a click lands on whatever
control the card is drawn over -- a toolbar button, a list row, a Delete
button -- which acts, though the reader cannot see it for the card. The card
should take the press: put itself away and do nothing else, as the
dictionary, file search, automator and crossword already do.

**Where.** Each application's mouse handling. The card's flag (`show_help`)
is read by the drawing and by the key handler, and by nothing that handles a
press.

**Found** 2026-10-03, converting colorpicker's value box to the toolkit's
field. Fixed the same day in colorpicker, contacts and dbviewer; then in
alarmclock, archivemanager, benchmark, calendar, camera, charmap,
clipmanager, defrag, devicemanager, diagram, diskanalyzer, diskcleanup
diskimager, ebook, explorer, fileassoc and filediff (2026-10-04) --
calendar's, diagram's, diskimager's, explorer's and filediff's cards were modal for the keys no more than for the pointer,
so each application's card is checked for both.

**Still to do.** A scan the same day found 66 more applications whose
production code draws `guitk::shortcut::render_card` and has no function
handling a mouse event that reads the card's flag:

finance, flashcards,
fontmanager, habits, hexeditor, imageviewer, ircclient, jsonviewer, kanban,
logviewer, markdowneditor, metronome, mindmap, musicplayer, notes, paint,
partmanager, passwordgen, pdfviewer, photomanager, pinball, podcast,
pomodoro, procexplorer, qrcode, radio, regextester, reminders, remotedesktop,
rssreader, screenrecorder, screenshot, settings, slides, soundrecorder,
spreadsheet, startupmanager, stopwatch, sudoku, sysinfo, sysmonitor,
systemrestore, tmux, torrent, undelete, whiteboard, wordle, worldclock.

A scan is a lead, not a verdict: an application may guard its card some way
the scan does not see, and each is confirmed by a test before it is changed.
Email was one: its card records a hit box over the whole window, last, so
every press, move and turn of the wheel finds the card -- a guard worth
copying wherever an application hit-tests its frame.
Delete each name as it is fixed; to see what is left, run from the tree's
root:

```python
import pathlib, re, sys
sys.path.insert(0, "scripts")
from rustscan import production_only
FN = re.compile(r"^[ \t]*(?:pub(?:\([^)]*\))?\s+)?fn\s+(\w+)[^{;]*\{", re.M)
MOUSE = re.compile(r"Event::Mouse|MouseEventKind|MouseButton|MouseEvent\b")
def bodies(text):
    for m in FN.finditer(text):
        i, depth = m.end(), 1
        while i < len(text) and depth:
            depth += (text[i] == "{") - (text[i] == "}")
            i += 1
        yield text[m.end():i]
for d in sorted(pathlib.Path("apps").iterdir()):
    if not (d / "src").is_dir():
        continue
    prod = "".join(production_only(f.read_text(encoding="utf-8", errors="replace"))
                   for f in sorted((d / "src").rglob("*.rs")))
    if "render_card(" in prod and not any(
        MOUSE.search(b) and re.search(r"\bshow_help\b", b) for b in bodies(prod)
    ):
        print(d.name)
```

**The fix**, per application, until the toolkit's card takes the pointer
itself (`requests/e-c-the-shortcut-card-takes-the-pointer-as-well-as-the-keys.md`):
at the top of the mouse handling, while the card is up a press with any
button puts it away and nothing else happens; a move or a release is not a
press. Where the application ends a drag on the release, the release must
still reach the drag. The test presses a control under the card and checks
the card went and the control did not act, then -- the control -- the same
press with the card down acts.
