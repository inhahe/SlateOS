## 885. A window is known as its program's by the name it declares; a pinned button stays a launcher

**Date:** 2026-09-26 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The taskbar can now tell which program a window belongs to: a
window says what it is (its `app_id`), and the desktop matches that against
the installed programs' desktop entries. It uses the answer to draw the
program's picture on the window's button. It deliberately does *not* fold the
window into its program's pinned button, as most desktops do, because this
one's design says the opposite: pinned programs on the left, "all launched
applications go to the right of those" (`design.txt`), and the Aero
reference's taskbar says it again -- "Clicking a pinned app launches a new
running instance on the right".

### The calls

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| Who says which program a window is | the window, by the `app_id` it declares, matched against the entries: the entry's file name (`org.example.Sketch` for `org.example.Sketch.desktop`, the Wayland convention), its `StartupWMClass`, or the program's file name | the compositor attributing each surface to the process it spawned -- `known-issues.md`'s recommendation | the compositor route is a protocol change (lane F's) and the only one that needs no cooperation; the declared name needs none of anyone else's work, SlateOS's own programs already declare their crate's name (`terminal`), and every freedesktop program declares its entry's. The compositor route remains open for programs that declare nothing. |
| Case | not significant | exact | window classes are conventionally capitalised (`Firefox`) and file names are not. |
| A pinned program's open window | its own button, right of the divider; a click on the pin starts another copy | on the pin, which then brings the window forward -- Windows 7 onwards, every dock, `taskbar.rs`'s never-run grouping model, and the known-issues entry that asked for this identity | `design.txt` (the window-manager list: "can pin apps to taskbar on the left, all launched applications go to the right of those") and the reference's taskbar component (`aero-taskbar.jsx`: a pinned "Files" beside an open "workspace" window in the running group, and a pinned "Nushell" beside a running one) both specify it, and `design.txt` is the authority. |

### A merged version was written and withdrawn

The merge was built first, on the known-issues entry's word that it is "the
ordinary behaviour of every desktop", tested, and withdrawn before it was
committed, on reading the reference's component. Whoever reaches for it next
-- it is still what most desktops do -- should read the row above first: the
design would have to change, and that is the operator's call, not a lane's.
