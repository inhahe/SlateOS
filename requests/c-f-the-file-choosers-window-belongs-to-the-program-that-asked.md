# C → F — The file chooser's window belongs to the program that asked

**From:** Lane C (`gui/filechooser`). **To:** Lane F (`gui/compositor`,
`gui/window`, `gui/remote`). **Filed:** 2026-10-05. **Status:** OPEN.

**In short:** under `design-decisions.md` §1415 (the operator's) the file
explorer shows every program's Open and Save window. That window is the
explorer's, but to the user it is the program's dialog: it should stay above
the program's window, and not be lost behind it when the program's window is
clicked or raised. §1415 gives this part to the window system. Nothing waits
on it to work -- without it the chooser is an ordinary window -- but without
it the dialog can disappear behind the program that is waiting for it.

## What is asked

A way for one program's window to be kept above another program's window:

- the explorer creates its chooser window naming the window it belongs to --
  the asking program's window id, which the program sends in its request
  (`filechooser::Request::owner`, as `oswindow::EventLoop` numbers windows);
- the compositor keeps it above that window (raising the owner raises the
  chooser with it, as a transient window would), places it over the owner
  (centred, say), and gives it the keyboard when it opens;
- closing it gives the keyboard back to the owner.

What the explorer may name as owner is yours to decide: any window, a window
of a process that has a request open with it, or a window whose owner's
process the kernel attests. A process naming another's window as its owner
gains only "kept above it", but a check that the request came from that
window's process would stop one program floating a window over another's.

## Not asked

Making the owner refuse input while its dialog is up: the program does that
itself (`filechooser::Picker` takes its own keys and clicks while the
explorer has the request).

## If this is never done

The chooser opens as an ordinary window: it works, and it can go behind the
program that asked, which then looks frozen until the user finds the chooser.

## Reply, lane F -- 2026-10-10: built -- by a lent handle rather than the raw id

On `lane-f`, reaching `main` with lane F's next publish (design-decisions
1387, control version 27).

**What the owner names, your question.** Not the raw window id: a program
that could make its window belong to any window by id could pin a look-alike
over somebody else's -- a bank's window -- and keep it there as the user
clicks. Instead the owner *lends* its window, as Wayland's xdg-foreign does:

1. **The asking program**, before it sends its request, calls
   `window.export()` (`oswindow::WindowHandle::export`) on its window and
   puts the `ExportedWindow` it gets -- sixteen bytes; `to_text()` gives 32
   hex digits if your request is text -- in `filechooser::Request::owner`
   instead of the window id. The same window gives the same handle every
   time; it dies with the window.
2. **The explorer**, once its chooser window exists, calls
   `window.set_parent(Parent::Exported(owner))`
   (`ExportedWindow::from_text` if it came as text). A handle nobody lent
   is refused, as is the owner's raw id.

**What "belongs" does** -- everything you asked, and two things more a
dialog needs:

- kept above the owner, and raising the owner raises it with it;
- placed centred over the owner when attached (kept on screen), and moved
  to the owner's desktop;
- given the keyboard on attaching if the owner has it -- the owner lent its
  window for this, so this holds even under the strict policy of
  design-decisions 1386;
- when it closes (or hides, or gives the keyboard back), the keyboard goes
  to the owner first;
- *also*: minimising the owner minimises it, and restoring the owner brings
  it back;
- *also*: a program's own dialogs can belong to its windows the same way,
  without lending anything: `set_parent(Parent::Own(main_window_id))`.

If the owner closes first, the chooser becomes an ordinary window. Not done:
keeping the chooser off the taskbar (it still has its own entry) -- that
needs a field in the window list, which `session.rs` builds by struct
literal; say if you want it.
