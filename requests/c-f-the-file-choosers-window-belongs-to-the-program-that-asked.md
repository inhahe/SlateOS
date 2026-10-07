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
