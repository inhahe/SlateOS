## §458 — A connection owns the windows opened over it, and that set is both the address book and the permission check

**Date:** 2026-08-17
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** When you press a key, the compositor (the program that draws the
screen) has to decide which application to hand the keystroke to. It also has
to decide whether an application asking it to "close window 7" is allowed to —
window 7 might belong to a different program. Both questions are answered the
same way: the compositor remembers, per connection, which windows were opened
over that connection. Keystrokes go to the connection that owns the window they
were aimed at, and a request naming a window the sender doesn't own is refused
as if that window didn't exist. The alternative — trusting the process id an
application sends — would let one program read another's typing.

**Context.** §457 settled that one connection carries every window a program
opens. That makes both questions non-trivial: the compositor's
`EventNotification`s each name a window, and every control request except
"create" and "what display is this?" carries a window id that arrived as a
plain number from an untrusted peer.

### Decision 1 — route input by the connection's own window set, not by process id

**Options.**

| | *What changes:* |
|---|---|
| **(1) The link's window set** ✅ | Events go to the connection that opened the window; a program with two connections gets each window's events on the connection it opened it over. |
| (2) The window's recorded `client_pid` | Events go to whichever connection claims that pid — ambiguous when one process holds two connections. |

**Chose (1).** (2) looks equivalent and is not. One process may hold two
connections (a plugin host, a library that opens its own), so a pid does not
identify a connection; and a pid is a number the compositor is *told*, not one
it observes, until a real transport gives it an authenticated peer credential.
Getting this wrong means delivering one application's keystrokes to another,
which is a password-shaped bug rather than a cosmetic one — so the routing key
should be the one thing the compositor knows first-hand, which is which
connection it was serving when it created the window.

`client_pid` is still recorded and still travels with the window, because the
taskbar and process-level policy want it. It is taken from the link rather than
from the frame for the same reason: a client that could state its own pid could
claim another process's windows in the window list.

### Decision 2 — the same set authorises requests, and a refusal does not distinguish "not yours" from "not there"

A request naming a window the link did not open is answered `no such window`,
byte for byte the same as one naming a window that never existed. The
alternative — a distinguishable "that exists, but is not yours" — is a probe:
any client could count the desktop's open windows, and watch them come and go,
by walking the id space. Since ids are sequential, that is a live inventory of
what the user is running.

The check happens in the translation layer, *before* the request reaches the
compositor, so the compositor's own API keeps its existing meaning ("do this to
this window") and does not grow an authorisation parameter that every internal
caller would have to supply something for.

### Decision 3 — a rejected picture is dropped silently, a rejected request is answered

`SURF` (an addressed draw frame) carries no sequence number and has no reply, so
a picture submitted for a window the sender does not own is discarded without
comment. That asymmetry is deliberate rather than an oversight: drawing is the
highest-frequency thing a client does, and giving it a reply to wait on would
put the compositor's acknowledgement on the critical path of every repaint
(§457 decision 4 made the same call for the honest case).

### Decision 4 — a malformed or wrong-way frame ends the connection

**Options.**

| | *What changes:* |
|---|---|
| **(1) Terminal error** ✅ | A client that sends garbage is disconnected. |
| (2) Skip the bad frame and resynchronise | The compositor tries to find the next valid frame in the stream. |

**Chose (1).** (2) cannot be implemented honestly: the length that would say
how far to skip is itself part of what is not trusted. A scanner looking for
the next magic would be a decoder for an attacker-chosen grammar, and the
"recovered" stream would be whatever the attacker arranged to appear next.

The same applies to a frame that only the compositor sends (`CRSP`, `SCEN`,
`INPT`, or a bare unaddressed `ORDR`): it means the peer is confused about
which end of the connection it is on, and treating it as a no-op would defer
that discovery to a much later symptom — a window that never draws.

### Decision 5 — a hung-up client is not obeyed and not written to

A closed link's buffered bytes are left alone rather than acted on: a client
that has gone cannot receive replies, so honouring its last requests would
create windows nobody can drive. Input addressed to its windows is dropped
rather than encoded — an outbox no one will read is a leak with a queue in
front of it — and specifically *not* left pending for another link to claim,
because no other link owns those windows. The windows themselves outlive the
link until the server destroys them, which keeps teardown the server's decision
rather than a side effect of a decode pass.

**Reversibility.** Easy. All five are confined to `gui/compositor/src/wire.rs`;
none changes the wire format, so a client built against this needs no change if
any of them is revisited. Decision 1 is the one worth revisiting when a real
transport arrives with an authenticated peer credential — at that point pid
becomes something the compositor *observes*, and could reinforce the link set
rather than substitute for it.
