## 1336. Local programs reach the display through a service whose peer the kernel names, and the shell gate is armed by the session that hands out the key

**Date:** 2026-10-03
**Lane:** F
**Decided by:** Claude (autonomous). Builds on lane A's channel descriptors
and service keys (§1517, §1518), built for
`requests/f-a-a-channel-handle-can-be-guessed-and-any-process-can-use-it.md`.

**In short:** on SlateOS a program now connects to the display through a
kernel channel rather than a network socket whenever it can. The kernel knows
which process is at the other end of a channel, and that one fact lets the
compositor say which program owns a window and tell the real desktop shell
from an application pretending to be it. Network connections still work, for
remote use and for anything that cannot use a channel, but the compositor
cannot vouch for who they come from. The rule that only the shell may do the
shell's things -- read every window's title, move other programs' windows --
is built and tested, but it is switched on only when the program that starts
the session also gives the shell its key. Switched on any earlier, it would
lock out the shell itself.

**What was decided.**

- **The default display is the service, with TCP behind it.** With
  `SLATE_DISPLAY` unset, a client on SlateOS connects to the service
  `org.slateos.Display` (`guiremote::channel::DISPLAY_SERVICE`). It falls back
  to `127.0.0.1:7373` over TCP only when no service exists: `ECONNREFUSED`
  (nothing registered under the name) or `ENOSYS` (a kernel without channel
  descriptors). The compositor serving the default display listens on both.
- **Other failures are reported, not fallen back from.** `EACCES`, `EMFILE`
  and the rest end the connection attempt with that error. Falling back would
  hide the misconfiguration, and it would silently give up the kernel-named
  identity the service exists to provide.
- **`SLATE_DISPLAY=service:NAME` names a service.** The same words work on
  the compositor's command line, where they mean "listen only as that
  service". Anywhere but SlateOS they are refused with `Unsupported` rather
  than handed to the address resolver.
- **The connection's number stays the connection's.** `ClientLink::client_pid`
  remains a per-connection id, because one process may hold two connections
  and ownership and routing must keep them apart. The kernel's answer is
  recorded beside it (`ClientLink::peer`, `peer_holds_key`), read once at
  accept, as `SO_PEERCRED` is.
- **Accepts alternate between the two listeners**, so a flood of connections
  on one cannot hold a connection on the other in its queue indefinitely.
- **The shell gate is a flag: `compositor --require-shell-key`**
  (`ShellGate::KeyHolders`). Without it the gate is open, as before. Whatever
  grants the shell the display service's key -- a `(Service,
  key_id("org.slateos.Display"), READ)` capability -- passes the flag in the
  same place.

**Alternatives for arming the gate.**

| | For | Against |
|---|---|---|
| A flag, set where the key is granted (chosen) | the gate and the key change together, in one line of the session's configuration; nothing is armed before the shell can pass it | a session that grants the key and forgets the flag runs ungated, as today -- visible in the compositor's startup log, which says when the gate is armed |
| Armed whenever the service is served | no configuration | refuses the taskbar, switcher and panels on every session until the key is granted; nothing grants it yet |
| Armed by the first key holder to connect | no configuration, and safe once a shell exists | before the shell connects, every client passes; the order of connections becomes a security property |
| The compositor starts the shell itself, on a connection it made (Weston's model) | no key at all: the compositor knows its own child | the session, not the display server, decides what the shell is; and lane A's service key already answers the question for every service, not only this one |

**Where it lives.** `gui/remote/src/socket.rs` (`Socket` over a `Carrier`;
`Listener` as TCP, service or both), `gui/remote/src/channel.rs` (the
descriptors), `gui/compositor/src/wire.rs` (`ShellGate`,
`ClientLink::require_shell`), `gui/compositor/src/server.rs` (attest at
accept, the startup log), `gui/compositor/src/main.rs` (the flag).

**Revisit when** the session's spawner grants the key: the flag should then be
on in every shipped session, and arguably the default.
