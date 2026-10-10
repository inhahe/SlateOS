## 1374. Remote desktop pairs a device by PIN, as Chrome Remote Desktop does, and turning it on sets up reaching the machine

**Date:** 2026-10-09
**Lane:** F
**Decided by:** Operator, answering `open-questions/F-Q6.md` in two parts: "F-Q6: B. But Chrome Remote Desktop has the advantage that it goes through Google's servers so computers can find each other. We may not have central servers for this, and many or most users may have dynamic IP addresses and no domains. I think roadmap-detailed calls for offering a dynamic IP service like dynu.net out of the box, and this should be integrated with setting up/enabling the remote desktop feature? Also btw, I want the client to run in the browser like Chrome Remote Desktop does and to be compatible with mobile, tablet and desktop (with expanding and scrolling the remote screen for small screens)." (`operator-answers/2026-10-09-open-questions-answers.txt`); and "F-Q6: Make a note that turning on remote desktop should allow configuring a dyndns service and/or running letsencrypt too as part of the process, whatever hasn't been done that needs to be done" (`operator-answers/2026-10-09-open-questions-answers.2.txt`). Claude recommended B.

**In short:** remote desktop will work like Chrome Remote Desktop. The user
turns it on in Settings and chooses a PIN; a device that enters it once is
remembered by a key it keeps, and Settings lists remembered devices and can
forget them. The account's password never crosses the network. Because there
is no central server to introduce two computers, turning remote desktop on
also sets up how to reach this machine -- a dynamic DNS name, the router's
port forward, and a certificate -- whatever of that is not done yet. And the
viewer runs in a web browser, on a phone, tablet or desktop, zooming and
scrolling the remote screen on small ones.

**Decision.**
- **Pairing by PIN** (F-Q6's option B): a handshake of its own, built from the
  cryptography SlateOS already carries for SSH; remembered devices by key.
- **Turning it on sets up reachability**, from the pieces lane D has built:
  `services/dyndns` keeps a Dynu, DuckDNS, No-IP or FreeDNS name pointed at
  the network and asks the router (NAT-PMP, then UPnP) for the forwards in
  `/etc/portforwards.yaml` (§1178). The enable flow adds remote desktop's port
  there, offers to set up the name, and gets a Let's Encrypt certificate for
  it, which a browser needs before it will run the viewer. Where no port can
  be opened (no UPnP, or a carrier's shared address), Settings says so, and
  says whether IPv6 still reaches the machine (lane A's note of 2026-10-09).
- **The viewer is a web page** the machine serves, usable on mobile, tablet
  and desktop, which zooms and scrolls the remote screen on small screens.

**Rationale.** It is what design.txt asks for ("something pretty much exactly
like Chrome Remote Desktop ... over the LAN or over the internet") and keeps
the password off the network; and without Google's servers, reaching a home
machine needs the name and the port set up, which the user should not have to
know to do separately.

**Alternatives.** A, through SSH: reuses proven code, but asks a desktop user
to run a login server and type their password into the viewer. C, both: can
still be added later at little cost.

**What it asks, and where it will be done.** `roadmap.md` (lane F): the remote
desktop service and its browser viewer, under the video-fallback item. The
Settings page that turns it on is lane C's, and the name and port pieces are
lane D's: `requests/f-cd-turning-on-remote-desktop-sets-up-reaching-the-machine.md`.

**How to reverse.** The pairing handshake is the service's own; replacing it
with SSH (A) or adding SSH beside it (C) leaves the stream and the viewer as
they are.
