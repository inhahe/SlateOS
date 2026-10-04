### [E] Thirty applications answer F1 with nothing -- 2026-10-04

**Status:** OPEN until the fixes have had a boot test on `main`: every one
of the thirty is settled on lane E's branch (2026-10-04) -- nine given a
list, twenty-one found to print their keys -- and this moves to
`known-issues-resolved/` once they are all on `main`.

**In short:** design-decisions §863 makes "press F1 to see the keys" true of
the whole suite: every application shows a list of its keys on F1 (and on
`?` where it is free), unless it prints them permanently on screen. A scan
finds thirty applications that take keys, raise no list on F1, and keep no
`show_help`-style flag at all. Some will turn out to print their keys in a
footer, which §863 accepts; the rest have keys a reader can find only by
pressing them -- sysmonitor's Delete, which ends the selected process, was
one, and has a list now.

**Where.** Each application's key handling and drawing.

**Found** 2026-10-04, giving taskscheduler's dialog the toolkit's fields
(taskscheduler is one of the thirty) and sysmonitor its list.

**Given a list** (2026-10-04), each with keys nothing on screen named while
the reader needed them: calculator (Escape is C, Delete CE, `^` x^y, Enter
`=`), credmanager (Ctrl+L, Ctrl+G, Delete, the generator's keys -- only
Ctrl+E was printed, on its button), editor (the keys its menus do not carry:
Ctrl+T and Alt+Z were in no menu, nor the find bar's keys or the moves by
word and by file; the list is built from the commands' own rows, so a
menu's key is written once), emojipicker (Escape -- and no key picked an
emoji at all: the grid answered only the pointer, and has the arrows, the
page keys and Enter now), launcher (Ctrl+1-8, Tab), netscan (everything but
F5: Ctrl+Enter, Ctrl+Tab, the list's keys -- and its five text boxes took
no key at all), taskscheduler (Delete, Space, Ctrl+N), videoplayer (*had*
a full list, in its Shortcuts tab: F1 opens that tab, and goes back) and
vpnmanager (its list's keys, Escape closing the window; the search box was
reachable only by a press).

Where `?` is never typed it raises the list too; where it is (a search box,
a document), F1 alone does, and the list says so.

**Settled as exempt** (§863: "a list already on screen needs no key to raise
it"), each read on 2026-10-04 and found to print its keys permanently:
asteroids (the controls line under the readings), battleship (a footer per
phase), breakout and pong (footer buttons that carry their keys -- `N  New
game`, `P  Pause` -- beside a line naming the arrows), checkers and chess
(the side panel), compass (the readouts panel's key help, and a footer in
each of its other two views), dots, match3, minesweeper, mixer, nonogram,
reversi, snake, unitconverter, wordsearch and yahtzee (a footer), lockscreen
and pacman (each prompt or sheet names the keys that answer it; pacman's
menu names the arrows and P before play starts), tetris (the CONTROLS
panel, whose rows are also buttons) and typingtutor (a footer per view, and
an `Esc` chip while typing).

A note for the next reader of a candidate: a first pass that looked only
for words like "Press" and "Enter" in strings called breakout, pong and
pacman unlisted. They are not -- their keys are on button faces and in
arrow glyphs (`\u{2190}/\u{2192}`). Read the drawing before deciding.

None remains: the scan below finds nothing (2026-10-04). An application it
finds later is one that took keys and no list since -- the same fix.

**The scan**, from the tree's root -- an application that implements `App`,
reads `Key::`, and has neither a card, a help flag nor an F1 arm in its
production code, and is not one of the exempt applications above:

```python
import pathlib, re, sys
sys.path.insert(0, "scripts")
from rustscan import production_only
HELP = re.compile(r"\b(show_help|help_open|help_visible|showing_help|show_keys|"
                  r"keys_open|help_shown|show_shortcuts)\b|render_card\(|Key::F1")
EXEMPT = set("asteroids battleship breakout checkers chess compass dots "
             "lockscreen match3 minesweeper mixer nonogram pacman pong reversi "
             "snake tetris typingtutor unitconverter wordsearch yahtzee".split())
for d in sorted(pathlib.Path("apps").iterdir()):
    if not (d / "src").is_dir() or d.name in EXEMPT:
        continue
    prod = "".join(production_only(f.read_text(encoding="utf-8", errors="replace"))
                   for f in sorted((d / "src").rglob("*.rs")))
    is_app = re.search(r"impl\s+(?:[\w:]+::)?App\s+for\b", prod) is not None
    if is_app and re.search(r"\bKey::", prod) and not HELP.search(prod):
        print(d.name)
```

**The fix**, per application, as sysmonitor's (2026-10-04): a `SHORTCUTS`
table naming every key the window answers; F1 raises it from anywhere (it
is never typed), `?` where nothing types one; `guitk::shortcut::render_card`
draws it over everything; while it is up it is modal for the keys and the
pointer (known-issues
E-a-press-goes-through-the-shortcut-card-to-the-control-drawn-under-it);
and the tests every other list has -- every advertised key answered, the
list reaching the window, the list modal -- with mutation rows.
