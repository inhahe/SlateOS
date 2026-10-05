## TD-C-NOTHING-CONNECTS-A-LAUNCHER-ENTRY-TO-THE-WINDOWS-IT-OPENS -- FIXED 2026-09-26

**Status:** FIXED 2026-09-26 (lane C). Filed among the resolved on 2026-10-05; what follows is the entry as it stood.

**Date:** 2026-09-14. **Lane:** C. **FIXED 2026-09-26**, by the first of the
three options below rather than the recommended third: the installed programs'
desktop entries now exist (`gui/desktopentry`), and a window's `app_id` is
matched against an entry's file name, its `StartupWMClass` or its program's
file name (`DesktopShell::program_for_app_id`); a window's button draws its
program's picture from it. **The merge this entry calls "the ordinary behaviour
of every desktop" is not this desktop's**: `design.txt` puts every launched
program to the right of the pins, and the Aero reference's pinned button starts
a new copy there, so a pinned program and its open window keep two buttons by
design (design-decisions §885). The compositor route stays open for programs
that declare nothing.

**In short:** the desktop cannot tell that the window in front of you belongs to
the program you started. It knows the program by the file it ran, and it knows
the window by a name the program chose for itself, and nothing anywhere maps
one to the other. So a pinned application and its own open window get two
separate taskbar buttons, and neither knows about the other.

**Where:** `launcher::AppEntry` has `name`, `description`, `executable_path`,
`keywords`, `category` and `launch_count` -- and no identifier.
`ManagedWindow` has `app_id`, *"which program the window belongs to, as that
program declares it"*, read fresh from the compositor on every list. The two
are different kinds of fact: one is what the desktop ran, the other is what the
running thing calls itself.

**What it blocks.** Taskbar pinning shipped on 2026-09-14 showing a pinned
button *and* a window button for the same program, and the code says why in as
many words. Merging them is the ordinary behaviour of every desktop and cannot
be written until this exists. It is also `design-decisions.md` 849's second
step: the shell adopting `taskbar.rs`'s window-*grouping* model, which is keyed
on exactly this correspondence.

**Why it is not a five-minute fix.** Adding an `app_id` field to `AppEntry` is
easy and answers nothing on its own, because the value has to be the one the
*program* will declare -- and the built-in app database would then be asserting
identities that the programs themselves have never agreed to. The question is
which end is the authority:

* **the launcher**, with programs required to declare the id their desktop
  entry gives them -- the freedesktop model, and it means a program that says
  nothing gets no grouping;
* **the program**, with the launcher learning the id the first time it sees a
  window from something it started -- no contract needed, but the first launch
  of each program is unmatched, and a program that changes its mind is
  indistinguishable from a second program;
* **the compositor**, which knows the process it spawned and the surface that
  appeared, and could attribute one to the other without either end declaring
  anything. This is the only one that needs no cooperation, and it is also a
  change to the protocol rather than to an application.

**Recommendation, for whoever picks this up:** the third. The compositor is
already the authority on which window is which, `ShellAction::Launch` goes
through it, and a launch-to-surface attribution is the same kind of fact it
already keeps. The first two both require every program to be well-behaved
before anything works, which is the condition this tree keeps discovering it
cannot rely on.

Not filed in `open-questions.md` because nothing is blocked *today* -- pinning
works, it just does not merge -- and the operator's queue is for decisions that
have to be made now.
