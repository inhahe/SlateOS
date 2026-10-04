### [E] Twenty-nine applications' text boxes type only at the end -- 2026-10-04

**Status:** OPEN

**In short:** in twenty-nine applications a text box takes typing only at
its end. There is no caret to move: the arrow keys, Home and End do nothing
in it, there is no selection, no Ctrl+A, C, X or V, no Delete -- Backspace
from the end is the only edit. A typo at the start of a server address, a
search, or a contact's name is fixed by deleting everything typed after it.
Most of the boxes were given the toolkit's look in the field sweep
(`requests/c-e-a-theme-can-shape-the-controls.md`), which is why they now
look like boxes that can do all of this.

**Where.** Each application's key handling for its boxes: typed text is
`push_str`ed, `extend`ed or `push`ed a character at a time onto a `String`,
and Backspace `pop`s it.

**Found** 2026-10-04, adding vpnmanager's list of keys
(`known-issues/E-thirty-applications-answer-f1-with-nothing.md`).

**The twenty-nine:** alarmclock (an alarm's label), charmap (search),
clipmanager (its form), colorpicker (the hex value), contacts (search and
the form), credmanager (the entry form, the master password, search),
dbviewer (cell editing), defrag (the exclusion box), diagram (a shape's
label), dictionary (the query), diskanalyzer (the path), editor (the find
bar), filesearch (the query), hexeditor (search and Go To), kanban (its
input), lockscreen (the password), mindmap (a node's text and its search),
passwordgen (its input), startupmanager (the dialog and search), tmux (its
command prompt), undelete (search).

Not among them: `terminal`, whose typing goes to the shell's own line
editor; `typingtutor`, where a line typed straight through is the exercise;
and `launcher` and `spreadsheet`, which keep a caret of their own (no
selection or clipboard, which is a smaller gap of the same kind).

**The fix**, per application, as `apps/sysmonitor`'s filter box and
`apps/netscan`'s boxes (2026-10-04): one `guitk::textinput::TextInput` for the
box with the keyboard, reloaded from the box's `String` when it changed
underneath; `textline::apply_key` for every key the box answers; the text
drawn with `guitk::textedit::draw`, so the caret and selection are drawn
where they are; a press that puts the caret under the pointer
(`textedit::cursor_at_click`); and tests of the caret, the selection and the
clipboard, with mutation rows. A numeric box kept as a number (vpnmanager's
port and MTU, taskscheduler's day and interval) edits its digits as text and
parses them back, refusing what does not fit as it does now. Delete each
name as it is done.

**The scan**, from the tree's root -- an application that appends typed text
to a `String`, at once or a character at a time, and has no editor of any
kind in its production code. `dictionary` types through an action
(`Action::Type`) that the pattern cannot follow, and is listed by hand:

```python
import pathlib, re, sys
sys.path.insert(0, "scripts")
from rustscan import production_only
EDITOR = re.compile(r"\bTextInput\b|\bTextArea\b|textline::apply_key|\btextarea::|"
                    r"codeedit|TextBuffer\b|InputDialog\b")
APPEND = re.compile(r"\.push_str\(\s*&?typed|\.extend\(\s*\w*\.?typed\(\)|"
                    r"\.push_str\(\s*&?(key|event)\.text|"
                    r"for \w+ in \w+\.typed\(\)[\s\S]{0,1500}?\.push\(|fn type_char")
SKIP = {"terminal", "typingtutor"}
for d in sorted(pathlib.Path("apps").iterdir()):
    if not (d / "src").is_dir() or d.name in SKIP:
        continue
    prod = "".join(production_only(f.read_text(encoding="utf-8", errors="replace"))
                   for f in sorted((d / "src").rglob("*.rs")))
    if re.search(r"impl\s+(?:[\w:]+::)?App\s+for\b", prod) and APPEND.search(prod) \
            and not EDITOR.search(prod):
        print(d.name)
```

Four more applications append typed text but also have an editor
somewhere -- calendar, email, systemrestore and torrent -- and are worth a
look for a box that was missed.

**Done:** emojipicker (2026-10-04, with its list of keys and the grid's
keyboard); vpnmanager (2026-10-04: the profile form, its port and MTU as
digits, the search and the split-tunnel range); taskscheduler (2026-10-04:
the task dialog's boxes, the day of the month and the interval as digits);
musicplayer (2026-10-04: the search box, which Enter now leaves on screen
with the search it filters by); devicemanager (2026-10-04: the search box);
filediff (2026-10-04: the find bar); ircclient (2026-10-04: the message
line); logviewer (2026-10-04: the search box).
