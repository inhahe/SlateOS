## F-Q6 — [F] Remote desktop: how does another computer prove it may watch and control this one? — Status: OPEN (raised 2026-10-04)

**In short:** SlateOS can now send its desktop to another screen -- windows
as their drawing commands, games and video as compressed video
(design-decisions §1343) -- and a viewer can redraw it. What is missing is the
program that lets another computer connect over the network, watch, and work
the mouse and keyboard. Whoever connects can do anything the user can, so
before it is built: how does a connecting computer prove it is allowed? (In
every option it is off until the user turns it on.)

**The options.**

| Option | *What changes:* |
|---|---|
| **A.** Through SSH, SlateOS's secure remote login | The viewer logs in as the user -- the account's password or an SSH key (a file proving who you are, set up once), the same as for a remote command line -- and asks for the desktop instead of a shell. Remote desktop works only where SSH login is turned on. No new security code: the encryption and the login are SSH's, already written and tested. |
| **B.** A pairing code, like Chrome Remote Desktop | The user turns remote desktop on in Settings and chooses a PIN. A viewer that enters it once is remembered by a key it keeps, so that device is not asked again; Settings lists remembered devices and can forget them. No account password crosses the network. New code for the connection's handshake (built from the cryptography SlateOS already carries for SSH), so more to review. |
| **C.** Both | B for ordinary use; A for people who already log in with SSH. The most work. |

**If never answered:** safe -- nothing is exposed, because there is nothing
to connect to; remote desktop simply does not exist yet. The pieces already
built (the stream, the video) are tested and wait. It does not get worse with
time.

**Claude's recommendation:** **B.** It is the experience design.txt asks for
("something pretty much exactly like Chrome Remote Desktop ... over the LAN or
over the internet"), and it keeps the account password off the network. A is
quicker and reuses proven code, but asks a desktop user to run a login server
and type their password into the viewer; it can be added later (C) at little
cost.

**Where it bites:** the remote desktop service and its viewer, the next step
of the roadmap's video-fallback item (lane F); the Settings page that turns it
on (lane C); the stream it serves (`gui/remote`, `gui/compositor`).
