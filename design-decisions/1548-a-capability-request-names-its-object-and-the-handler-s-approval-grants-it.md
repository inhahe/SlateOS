## 1548. A capability request names its object, and the handler's approval grants it

**Date:** 2026-10-08 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** design.txt wants a program that needs more access to *ask the
user*, saying why, and the user to answer in a security dialog. A program
could ask (`SYS_CAP_REQUEST`), but nothing outside the kernel could answer:
every request was refused the moment it was made, and a "yes" typed in the
kernel shell granted nothing. Now one program -- the desktop's security
dialog, holding a new right to do so -- registers to answer, is told of each
request on a channel it can sleep on, and answers with one call that grants
what was asked in the same step. A request names exactly what it is for
("debug process 1234", "set the clock"), and what cannot sensibly be asked
for is refused. Lane C's `requests/c-abf-a-program-asking-for-a-capability-reaches-no-one.md`.

**What a request names.** A type, an object and rights, as a capability
does: the object is the capability's `resource_id` -- a pid, a thread, an
I/O port, an interrupt line, a reserved port -- or 0 for the whole class
(`SYS_CAP_REQUEST_FOR`; the old `SYS_CAP_REQUEST` is the class form). The
kernel adds what the user needs to judge it: the asker's name, and the
object's own name where it has one (the target process's). Three kinds of
thing cannot be asked for (`cap::request::requestable`, exhaustive over the
types so a new type forces the question):

| | examples | why |
|---|---|---|
| handles | a channel, pipe, terminal end, socket | had by making or receiving them; another process's terminal end would be a keystroke injector, and nobody can judge a handle number |
| numbered objects of class-only types | `File` 7, `Socket` 3 | the id is reserved or names a handle: nothing a user could judge |
| the right to answer requests (`CapBroker`) | -- | a program asking to become the security dialog |

Files are not asked for one at a time at all: the file chooser hands a
program the file the user chose (design-decisions 1415,
`requests/c-ad-hand-a-program-the-file-it-chose-not-its-name.md`). Asking
for `File` asks for *every* file, and the dialog must say so.

**Who answers.** A process holding the new resource type `CapBroker` (35)
with `WRITE` registers (`SYS_CAP_BROKER_REGISTER`); there is one at a time.
It is given the end of a channel the kernel sends on: one message per event
-- a new request with its record, a request ended with its final status
(decided ones too, so each request is one `NEW` and one `ENDED`), or "events
were lost" when its queue was full, after which it reads the list again
(`SYS_CAP_REQUEST_LIST`, which is also how a restarted dialog finds what is
waiting). It answers with `SYS_CAP_REQUEST_DECIDE`. Its exit -- or an exec
-- unregisters it and refuses what was pending; with no handler a request is
refused as it is filed. The kernel shell's `capreq handler on` makes the
console the handler, a debugging aid a process replaces.

**Decide grants.** Approval inserts the capability into the asker's table
under the broker's lock, so no cancel, exit or timeout can land between the
grant and its record, and a request never reads Approved while nothing is
held. The asker learns the answer by waiting (`SYS_CAP_REQUEST_WAIT`) or
asking (`SYS_CAP_REQUEST_STATUS`, now its own requests' and the handler's
only).

**Choices, each with the alternative:**

1. **Events on a channel, plus a list** -- lane C's ask, against a waitable
   counter the handler re-reads. A channel works with `SYS_WAIT_MULTIPLE`
   beside the desktop's compositor connection, and an event says what
   changed; the list covers a dialog that starts late or loses events, so a
   full queue never loses a request, only its notice.
2. **The handler cannot answer its own requests** -- defence in depth, not a
   wall: the handler is trusted by construction (it can approve anything for
   anyone else), but a dialog bug should not turn into "the dialog grants
   itself whatever it likes".
3. **An exec cancels the process's requests, and unregisters a handler.**
   The user is asked on behalf of the program that asked; the program an
   exec puts in its place did not, and must not be handed what its
   predecessor asked for. A new program answers only by registering itself.
4. **`CapBroker` is class-only and the admin group holds it**, like every
   type (the admin group is defined as holding all of them); it implies no
   Linux capability. Granting it to the desktop's session is lane B's or
   lane D's, as init's `caps:` lines carry `InputDevice` for the
   compositor.

**Not decided here, and asked:** what the dialog's "Always allow" should
mean -- open-questions A-Q26. Until it is answered, an approval lasts as long
as the process holding it.

**Tested by:** the broker's boot self-test (`cap::request::self_test`, with
real processes: refusals, the events and records, allow grants, deny,
timeouts, exits, lost events, the console) and the native ring-3 test
`spawn::self_test_native_cap_broker` (`build/capbrokertest.c`: a handler and a
forked asker, end to end through the system calls).
