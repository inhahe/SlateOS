### [E] A press goes through the shortcut card to the control drawn under it -- 2026-10-03

**Status:** OPEN until the last of the fixes has had a boot test on `main`:
every application the scan below found has its fix on lane E's branch (done
2026-10-04), and this moves to `known-issues-resolved/` once they are all on
`main`.

**In short:** in most applications the F1 shortcut card was modal for the
keys but not for the pointer. With the card up, a click landed on whatever
control the card was drawn over -- a toolbar button, a list row, a Delete
button -- which acted, though the reader could not see it for the card. The
card now takes the press: it puts itself away and nothing else happens, as
the dictionary, file search, automator and crossword already did.

**Where.** Each application's mouse handling. The card's flag (`show_help`)
was read by the drawing and by the key handler, and by nothing that handled
a press.

**Found** 2026-10-03, converting colorpicker's value box to the toolkit's
field. Fixed the same day in colorpicker, contacts and dbviewer; then on
2026-10-04 in alarmclock, archivemanager, benchmark, calendar, camera,
charmap, clipmanager, defrag, devicemanager, diagram, diskanalyzer,
diskcleanup, diskimager, ebook, explorer, fileassoc, filediff, fontmanager,
hexeditor, imageviewer, ircclient, jsonviewer, reminders, mindmap,
musicplayer, notes, paint, partmanager, pdfviewer, photomanager, podcast,
pomodoro, procexplorer, radio, remotedesktop, screenrecorder, screenshot,
settings, spreadsheet, startupmanager, stopwatch, sudoku, sysinfo,
systemrestore, undelete, whiteboard, wordle and worldclock.

**Modal for the keys no more than for the pointer.** Each card was checked
for both, and many failed the keys too: calendar's, diagram's,
diskimager's, explorer's, filediff's, hexeditor's, imageviewer's,
ircclient's, jsonviewer's, reminders', mindmap's, pdfviewer's,
remotedesktop's, spreadsheet's, sudoku's and wordle's -- and, among the
applications whose pointer was already guarded (below), logviewer's,
regextester's and rssreader's. Hexeditor's let a hex digit typed with the
card up be written into the file under it, imageviewer's let Delete send
the picture under it to the bin, ircclient's let Enter send the line under
it to the channel, pdfviewer's let Ctrl+W close the tab under it, and
remotedesktop's sent every key to the remote machine under it. (Reminders
takes no press at all; its card was modal for no key.)

**Struck off unchanged.** A scan is a lead, not a verdict: an application
may guard its card some way the scan does not see, and each was confirmed
before it was changed. Eighteen were struck off:

- *A hit box over the whole window* (15): email, finance, flashcards,
  habits, logviewer, markdowneditor, metronome, pinball, qrcode,
  regextester, rssreader, slides, soundrecorder, tmux and torrent each
  record one, last, while the card is up, so every press, move and turn of
  the wheel resolves to the card -- a guard worth copying wherever an
  application hit-tests its frame. Their keys were checked too: logviewer's,
  regextester's and rssreader's were fixed (above), the rest were modal.
- *No press on the window* (2): kanban and passwordgen take a press only in
  their file dialogs, and both cards were already modal for the keys.
- *No card at all* (1): sysmonitor. The `render_card(` the scan found is
  its own method, which draws a dashboard panel.

**The scan** prints those eighteen and nothing else as of 2026-10-04. Run
it from the tree's root; anything else it prints is a new application with
the bug:

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
at the top of the mouse handling -- ahead of any dialog the card is drawn
over -- while the card is up a press or double click with any button puts it
away and nothing else happens, and the wheel is swallowed; a move or a
release is not a press, and passes, so a drag begun before the card came up
still ends where it is let go. The keys: while the card is up, F1, `?` (where
the application binds it) and Escape put it away, plain, and every other key
is swallowed. Each application's test presses a control under the card with
each button and checks the card went and the control did not act, then --
the control -- the same press with the card down acts; mutation rows in each
`mutate.py` hold every half of it.

**Found along the way, and fixed.** The wheel truncated or rounded a
touchpad's fractions of a notch in pomodoro, stopwatch, undelete and
startupmanager, and moved radio's selection one station per event whatever
the event's size; photomanager's grid had no wheel at all. Each now keeps a
`guitk::wheel::Accumulator`.
