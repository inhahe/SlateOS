## 842. The system tray is the shell's, because 815 already said so

**Date:** 2026-09-13. **Lane:** C. **Decided by:** Claude (autonomous), but
only in the sense of *applying* an Operator decision -- 815's dividing line
answers this without a new judgement, and the value here is noticing that it
does.

**In short:** the system tray is implemented four times over and nothing
connects any two of them, so the first question is which one is real. It looked
like a fresh architectural fork worth asking about. It is not: the operator
already drew this exact line in 815, and a tray falls on the shell's side of it
plainly.

**815's rule, verbatim:** *"is this something the desktop shows you, or a
screen you open?"* -- the volume overlay and the login screen are the desktop
showing you something, so they stay in the shell; everything you open from a
menu moves to the Settings app.

A system tray is on screen the whole time, in the taskbar, whether or not
anybody opened anything. It is the clearest case of "something the desktop
shows you" in the tree. So:

| | verdict |
|---|---|
| `gui/desktop`'s taskbar tray | **the real one.** Already wired, already drawing the clock, bell, desktop and layout indicators, and already owning the taskbar's right-to-left layout that icons have to fit into |
| `apps/systray` (3 809 lines) | the copy. Its unique parts -- quick settings, the volume popup, the network popup -- are also things the desktop shows you, so they move to the shell rather than to Settings |
| `gui/desktop/src/tray_dnd.rs` | keeps its home; it was always the shell's |
| `kernel/src/fs/systray.rs` | lane A's persistence, orthogonal -- it stores what the shell decides |

**What this does not decide,** and it is the actual work: no protocol exists by
which an application registers an icon with anything. That is a registration
verb plus a per-client registry with reaping plus a subscription frame, and
`gui/remote/src/window_list.rs` is its model. Knowing where icons *go* is a
prerequisite for building that, which is why this was worth settling first.

**The alternative, and why it loses.** `apps/systray` is the bigger and newer
implementation, so "keep the app, delete the shell's tray" is the argument from
sunk work. It fails on geometry: the tray is a strip inside the taskbar, and
the taskbar is the shell's. A separate window would have to be positioned
inside another process's panel and kept there through every resize, theme
change and scale change -- a compositor-enforced parenting that nothing else in
this tree needs and that exists in no desktop worth copying. The shell's tray
is not the smaller implementation, it is the one in the right process.

**A note on how nearly this became an open question.** It was about to be filed
for the operator. Re-reading 815 first turned a fork into an application of an
existing rule, and `open-questions.md` says plainly what padding it costs: a
queue with items that need no thought trains the reader to skim it.
