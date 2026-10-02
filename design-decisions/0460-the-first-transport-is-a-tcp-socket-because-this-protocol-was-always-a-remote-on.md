## §460 — The first transport is a TCP socket, because this protocol was always a remote one

**Date:** 2026-08-17
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Applications draw pictures and the compositor (the program that
owns the screen) puts them on screen, but until now there was no *pipe* between
the two — the two halves of the conversation were both written, tested, and
unable to reach each other. Something had to carry the bytes. The choice was
between an ordinary network socket, an operating-system-local pipe of some
kind, and waiting until SlateOS itself can carry messages between processes.
A network socket was chosen: it works today on the development machine, it
works unchanged on SlateOS once its networking is up, and it is the only one of
the three that also gets remote desktops — which is what the crate carrying
this protocol says it is for in its very first line.

**The problem.** `guiremote` had two ends of a protocol and one transport:
`loopback::Pipe`, two queues inside a single process. Everything on both sides
was tested against it and nothing had ever crossed a process boundary. The
compositor's `main()` had no listener; `apps/editor`'s `connect()` was
literally `fn connect() -> Option<oswindow::Pipe> { None }` followed by a
four-line diagnostic explaining that nothing carries frames between processes
yet. So the question was not "which transport is best in the abstract" but
"what is the first one, given that whichever it is will be what every
application in the tree is written against."

**Options.**

1. **A TCP socket, with the address in an environment variable.**
   *What changes:* the compositor listens on `127.0.0.1:7373` (or whatever
   `SLATE_DISPLAY` says), applications dial it, and a compositor on another
   machine is reachable by changing one string.
2. **A local-only carrier — a Unix domain socket, or a Windows named pipe.**
   *What changes:* the same thing on one machine, and nothing across machines.
   The peer's process id becomes askable (`SO_PEERCRED`), and the address is a
   path with filesystem permissions rather than a port anyone can dial.
3. **Wait for SlateOS channel IPC.**
   *What changes:* nothing, until SlateOS can run this code — and today it
   cannot, so the editor stays unable to open a window and 138 application
   crates stay written against an imagined caller.

**Chosen: 1.** Three arguments, in descending weight.

*It is not a stopgap.* `gui/remote/src/lib.rs` opens by describing itself as a
remote display protocol; `design.txt` wants remote desktop as a feature, not as
an afterthought. A protocol that is designed to be serialised and shipped
somewhere else, and is then given a transport that cannot leave the machine,
has had a capability removed on purpose. Option 2 is the one that would be
thrown away later.

*It is the same code on both targets.* SlateOS is getting a TCP/IP stack
(`net/`, lane C's own zone), and `std::net` is what its `std` port will
implement. A named pipe on the hosted build is Win32; a Unix socket is
POSIX-only and needs `AF_UNIX` support the SlateOS netstack does not plan
first. Option 1 is the only one where the hosted development build and the real
build run the *same* transport rather than two that must be kept in agreement.

*It is not the last one.* `Transport` is a trait with four methods
(`read`, `write`, `is_open`, `wait`). A SlateOS channel becomes a second
implementation beside `socket::Socket` and `loopback::Pipe`, and no application
changes, because applications say `oswindow::connect()` and never name TCP —
the same discipline §457 imposed on the wire format. Option 3's benefit is
therefore obtainable *later*, without cost, which makes waiting for it pure
loss.

**What option 2 was genuinely better at, and what was done instead.** A local
socket can be asked which process is on the other end; TCP cannot, and for a
genuinely remote client the question is meaningless — a pid in another
machine's namespace names nothing here. The compositor wanted a `client_pid`
for window ownership and for the taskbar. `server::Server` therefore assigns
each connection a counter value and documents it as standing in for a pid.
That is honest about what it is, and it is sufficient: everything the
compositor does with the number needs it to be *unique per connection*, not to
be a real process id.

The other thing option 2 has is filesystem permissions on the address. The
mitigation is that `DEFAULT_DISPLAY` is `127.0.0.1:7373` — loopback, not
`0.0.0.0` — so a default install is not listening to the network. That is a
weaker boundary than a socket file's mode bits, and it is recorded here as a
known limit rather than claimed as equivalent: any local process can dial the
display. Closing that properly is a capability handshake on connect, which is
SlateOS's model anyway and belongs with option 3's transport rather than being
retrofitted onto this one.

**Revisit if** the hosted build ever needs to run two mutually-untrusted local
clients, which is when "any local process can connect" stops being acceptable
and a real authentication step (or option 2's mode bits) has to arrive.
