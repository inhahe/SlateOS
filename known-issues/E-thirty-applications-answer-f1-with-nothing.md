### [E] Thirty applications answer F1 with nothing -- 2026-10-04

**Status:** OPEN

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

**The thirty** (candidates; each to be read before it is changed):

asteroids, battleship, breakout, calculator, checkers, chess, compass,
credmanager, dots, editor, emojipicker, launcher, lockscreen, match3,
minesweeper, mixer, netscan, nonogram, pacman, pong, reversi, snake,
taskscheduler, tetris, typingtutor, unitconverter, videoplayer, vpnmanager,
wordsearch, yahtzee.

§863 already names minesweeper, mixer and wordsearch as printing their keys
in a permanent footer -- so those three are exempt unless the footer has
since gone. Delete each name as it is settled, one way or the other.

**The scan**, from the tree's root -- an application that implements `App`,
reads `Key::`, and has neither a card, a help flag nor an F1 arm in its
production code:

```python
import pathlib, re, sys
sys.path.insert(0, "scripts")
from rustscan import production_only
HELP = re.compile(r"\b(show_help|help_open|help_visible|showing_help|show_keys|"
                  r"keys_open|help_shown|show_shortcuts)\b|render_card\(|Key::F1")
for d in sorted(pathlib.Path("apps").iterdir()):
    if not (d / "src").is_dir():
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
