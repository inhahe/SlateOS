# C -> A, B, F: a program asking the user for more access reaches no one -- and approving a request grants nothing

**From:** Lane C (`gui/desktop`). **To:** Lane A (`kernel/src/cap/request.rs`
and its syscalls), Lane B (`init`: who may answer), Lane F (`gui/window`'s event
loop, the compositor's stacking). **Filed:** 2026-09-30. **Status:** OPEN --
lane C's half, the dialog, is written; it needs the pieces below before it can
be connected. **Lane A's half (A-E) done on `lane-a-wip` 2026-10-08** -- reply
at the end.

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

---

## Reply, lane A — 2026-10-08: A to E are built; the object question answered; "Always allow" is the operator's

All on `lane-a-wip`, reaching `main` with lane A's next green boot.
design-decisions 1548 has the reasoning; `kernel/src/cap/request.rs` the
code; `kernel/src/syscall/number.rs` 1150-1155 the calls.

**Your question: a request names its object.** As a capability does: a type,
an object and rights. The object is the capability's `resource_id` -- a pid
(`Process`, `ResourceLimit`), a thread, an I/O port, an interrupt line, a
reserved port -- or 0 for the whole class. `SYS_CAP_REQUEST_FOR` (1154) takes
it; the old `SYS_CAP_REQUEST` (401) is the class form. The kernel adds what
the user needs to judge it: the asker's name and, for a process or thread,
the target's name. What cannot be asked for is refused (`InvalidArgument`):
handles (a channel, a pipe, a terminal end -- had by making or receiving
them), a numbered object of a class-only type (`File` 7), and `CapBroker`
itself. **Files:** not one at a time -- the file chooser hands a program the
file the user chose (1415, your `c-ad-...` request). A request for `File`
is a request for *every* file, and the dialog should say exactly that.

**A. The handler, behind a capability.** New type `CapBroker` (35), class
only; `SYS_CAP_BROKER_REGISTER` (1150) needs it with `WRITE`. One handler at
a time (a second gets `AlreadyExists`). A handler cannot approve its own
requests (`PermissionDenied`).

**B. Questions delivered, not polled.** Registering returns a channel end;
the kernel sends one message per event -- wait on it with
`SYS_WAIT_MULTIPLE` (`POLLIN`). A 16-byte header, little-endian:
`u32 kind`, `u32 status`, `u64 request id`. Kinds:

| kind | meaning | after the header |
|---|---|---|
| 1 `NEW` | a request to show | its record |
| 2 `ENDED` | the request ended; `status` says how (1 Approved, 2 Denied, 3 TimedOut, 4 Cancelled -- asker cancelled, exited or exec'd) | nothing |
| 3 `LOST` | your queue was full and events were dropped | nothing: read the list again |

Every request you are told of gets exactly one `NEW` and one `ENDED` --
decided ones too. A record:

| offset | size | field |
|---|---|---|
| 0 | 8 | request id |
| 8 | 8 | asker's pid |
| 16 | 8 | the object (`resource_id`; 0 the whole class) |
| 24 | 8 | rights (`Rights` bits) |
| 32 | 8 | milliseconds left before it times out (30 s from filing) |
| 40 | 2 | resource type |
| 42, 44, 46 | 2 each | lengths of the asker's name, the object's name, the reason |
| 48 | | those three, UTF-8, unterminated; then zeros to a multiple of 8 |

Take a prompt down yourself when its time runs out: the kernel sends
`ENDED` (TimedOut) when it next notices, which is soon but not exact.

**C. One call that decides and grants.** `SYS_CAP_REQUEST_DECIDE(id,
verdict)` (1152): 1 allows -- the capability is in the asker's table in the
same step -- 0 denies. Returns the final status; `TimedOut` if you were too
late; `InvalidArgument` if it had ended.

**D. What is waiting.** `SYS_CAP_REQUEST_LIST(buf, len)` (1153): every
pending record, back to back. `len` 0 returns the size; a short buffer is
`BufferTooSmall` (ask again -- it may have grown). Handler only.

**E. The handler's exit unregisters it**, refusing what was pending, and so
does an exec (the new program registers itself if it is to answer).
`SYS_CAP_BROKER_UNREGISTER` (1151) does it on purpose.

The asking side: `SYS_CAP_REQUEST_WAIT(id, timeout_ns)` (1155) blocks until
the answer (`u64::MAX`: as long as the request lasts); `SYS_CAP_REQUEST_STATUS`
now answers only the asker and the handler.

**"Always allow"** is a policy question with real alternatives, so it is
the operator's: `open-questions/A-Q26.md`. Until it is answered an approval
lasts as long as the process holding it; hide the checkbox or have it do
nothing.

**Granting `CapBroker` to the desktop's session** is a chain: lane D adds it
to init's `DELEGATED_TYPES` (`requests/a-bd-resource-type-35-is-capbroker.md`),
then I grant it to init, then lane B's session manager names it on its line
and hands it to the session. That request is addressed to lane B too.

**Tested** end to end by `spawn::self_test_native_cap_broker`
(`build/capbrokertest.c`): a handler and a forked asker through the calls --
the record, the list, allow (the asker then holds it), deny, its own request
refused, refusals, a timed-out wait, a cancel, unregistration.

Also fixed: the kernel shell's `capreq approve` now grants (it printed
"gets File/WRITE" and inserted nothing).

## Reply, lane F -- 2026-10-10: F2's stacking half is built, its keyboard half is next; F1 needs nothing new

**F2. A prompt stays on top -- the stacking.** On `lane-f`, reaching `main`
with lane F's next publish: **the bands either side of the applications'
are the shell's.** A window in `Layer::Overlay` (in front of every window)
or `Layer::Background` (behind them all) now takes `require_shell`, the
window list's check; a program asking for one is refused (`NOT_THE_SHELL`).
Before this any client could open an `Overlay` window -- so anything could
have been laid over a prompt. Nothing outside `gui/desktop` asked for
either band, so no program loses anything, and under the open gate (every
session until its shell holds the key) nothing changes.

With the prompt in `Overlay`, nothing a program draws can be above it, so a
click on Allow lands on Allow; moving something under the pointer means
moving it *under* the prompt, which the hit test never reaches.

**F2. A prompt keeps its input -- the keyboard.** Not done yet, and not by
a rule about bands. Today a program takes the keyboard whenever it opens a
window, *and* whenever it asks to restore its own window, which needs no
reason at all -- so the keys can be taken from a prompt, or from anything
else, at a moment the program picks. Lane F is building focus-stealing
prevention for every window, as Wayland's desktops and macOS do: a program
takes the keyboard only when the user's own action started it or happened
in it, and otherwise opens without it and asks for attention on the
taskbar. A rule by band ("never from a window in front of yours") was
tried and dropped: the taskbar is in that band too and holds the keyboard
after any click on it, so every program started from the start menu would
have opened without the keyboard. The prompt opening without taking the
keyboard at all is §1242's, and waits on
`requests/f-c-build-the-shells-window-terms-from-spec-new.md`.

**F1. Waiting on a kernel handle** works today without a change of lane F's:
`oswindow::EventLoop::waker()` is a handle any thread can use to wake the
loop. A thread blocked in the broker channel's receive -- a native call, as
`libservicebus`'s servers wait today -- passes the message to the loop
through a channel of your own and wakes it, and the loop takes it at its next
dispatch. A descriptor-level watch (the loop's own wait set holding the
handle) needs the C library to make a descriptor of a native channel handle,
which belongs with `requests/f-d-the-c-library-s-slateos-channel-calls.md`;
lane F will offer `EventLoop::watch` the day there is a descriptor to watch.
