# C -> A, B, F: a program asking the user for more access reaches no one -- and approving a request grants nothing

**From:** Lane C (`gui/desktop`). **To:** Lane A (`kernel/src/cap/request.rs`
and its syscalls), Lane B (`init`: who may answer), Lane F (`gui/window`'s event
loop, the compositor's stacking). **Filed:** 2026-09-30. **Status:** OPEN --
lane C's half, the dialog, is written; it needs the pieces below before it can
be connected.

**In short:** `design.txt` wants a program that needs more access to *ask the
user*, saying why, and the user to answer in a security dialog. All three
pieces exist and none is connected. A program can ask (`SYS_CAP_REQUEST`, 401);
the kernel queues the question (`kernel/src/cap/request.rs`); the desktop has
the dialog (`gui/desktop/src/security_dialog.rs`, 2,154 lines, 38 tests). But
nothing outside the kernel can answer, so every question is refused the moment
it is asked -- and a "yes" given in the kernel shell, the only place one can
be given, grants nothing. `roadmap.md` §1.5 marks the whole chain done.

A *capability* here is an unforgeable handle to one kernel object -- holding
it is the permission, and there is no other kind (no "ambient authority":
permission a program has just by being who it is).

## What happens today

Checked on `lane-c-wip` at `6a0bf211b` (2026-09-30):

1. **Every request is refused at once.** `request_capability` auto-denies
   unless `HANDLER_REGISTERED` is set, and the only things that set it are the
   kernel shell's `capreq handler on` and the broker's self-test. A program's
   `SYS_CAP_REQUEST_STATUS` reads 2, Denied, straight away.
2. **Nothing in userspace can be the handler.** `register_handler`,
   `list_pending`, `approve` and `deny` are called from `kshell.rs` and the
   self-test, and there is no syscall for any of them.
3. **Approving grants nothing.** `approve` marks the request Approved "for
   audit purposes"; its own doc says the handler must grant the capability
   through the cap table, and nothing does. The kernel shell's `capreq
   approve` prints `Approved #N: pid=P gets File/READ` and inserts nothing,
   so a program that polls, sees Approved and goes ahead still does not hold
   what it asked for. (Lane A: that message is itself a defect worth a
   `known-issues.md` line of yours.)
4. **The dialog is constructed by nothing** -- `scripts/orphan-modules-baseline.txt`
   lists it, and no file outside it names `SecurityDialog`.

## First, a question for lane A: a request names no object

A request carries a resource *type* and rights -- `File, READ|WRITE` -- and
nothing saying *which* file. Granted as asked, that is either meaningless (a
handle to what?) or every file there is, which is the ambient authority the
architecture rules out. So a request has to name its object: a path for a
file or folder, a device for a device, a port number for a port -- and the
dialog shows it, which is also what makes the question answerable: "Photos
wants to read and change /home/ann/Pictures" can be decided; "Photos wants
File read-write" cannot.

How the object is named is lane A's to shape. Lane C needs only a form it can
show a person -- a path as bytes, a device's name -- and to know what "Always
allow" (a checkbox the dialog has) should be allowed to mean, if anything. If
the shape needs the operator, it belongs in `open-questions.md`.

## What lane C needs

**Lane A** -- the answering side of the broker:

- **A. Becoming the handler, behind a capability.** One process at a time
  registers to answer. Gated, so that a program cannot register and approve
  its own request -- which, with the grant in C below, would make every
  capability one request away.
- **B. The questions delivered, not polled.** A channel on which the kernel
  posts a message for each new request (id, pid, program name, the object,
  the rights, the reason, when) and for each that ends without an answer --
  timed out, cancelled, the asking program gone -- so a prompt for a question
  that no longer stands is taken down. Polling is not an option for the
  desktop: an idle desktop sleeps with no wake-up registered at all
  (design-decisions §812).
- **C. One call that decides and grants.** Approve inserts the capability into
  the asker's table in the same step; deny refuses. One call, so there is no
  window in which a request reads Approved and nothing is held.
- **D. The list of what is still waiting**, for a desktop that starts -- or
  restarts after a crash -- with requests already queued.
- **E. The handler's exit unregisters it**, so a desktop that crashes leaves
  requests refused rather than hanging until their timeout (the broker already
  auto-denies on unregister).

**Lane B** -- init, or the session manager, grants the handler capability to
the desktop's session and to nothing else, in the per-service capability list
that already carries the compositor's `InputDevice`.

**Lane F:**

- **F1. The desktop's event loop waits on a kernel handle** beside its
  compositor connection -- `SYS_WAIT_MULTIPLE` (1066) exists -- and hands the
  shell an event when that handle is ready. Needed twice over: for this, and
  for the network indicator's link events
  (`known-issues.md` `TD-C-THE-NETWORK-INDICATOR-HAS-A-SOURCE-AND-NOTHING-TO-WAKE-IT`).
- **F2. A prompt stays on top.** While the shell shows a security prompt, no
  other client's surface can be raised above it or take its input -- so a
  program cannot cover the prompt with a look-alike, or move something under
  the pointer as the user clicks Allow.

## What lane C does when these land

The desktop registers as the handler when its session starts, and asks for the
list; each request goes into the dialog, on a surface of its own above every
window; Allow and Deny become the decide call; a request that ends unanswered
takes its prompt down. Tested against a fake broker, as the shell's other
services are, with a session test through `TestDesktop`. Until then
`roadmap.md` §1.5 says `[-]` for the dialog's integration instead of `[x]`, and
`known-issues.md` `TD-C-A-PROGRAM-ASKING-FOR-A-CAPABILITY-REACHES-NO-ONE`
tracks lane C's side.
