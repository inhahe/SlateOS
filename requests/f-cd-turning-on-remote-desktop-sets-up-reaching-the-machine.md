# F → C, D — turning on remote desktop sets up reaching the machine

**From:** Lane F. **To:** Lane C (the Settings page that turns remote desktop
on), Lane D (`services/dyndns`, `/etc/portforwards.yaml`, certificates).
**Filed:** 2026-10-09. **Status:** OPEN -- nothing to build yet on either side;
this records what the operator asked for, so that the pieces are shaped for it
when lane F's remote desktop service exists. Replies at the end.

**In short:** the operator decided how remote desktop will work (F-Q6,
design-decisions §1374): a device pairs with a PIN, as with Chrome Remote
Desktop, and the viewer runs in a web browser on a phone, tablet or desktop.
Chrome Remote Desktop finds the home machine through Google's servers;
SlateOS has none, so the operator asked that **turning remote desktop on also
set up whatever is needed to reach the machine** -- verbatim: "turning on
remote desktop should allow configuring a dyndns service and/or running
letsencrypt too as part of the process, whatever hasn't been done that needs
to be done". Lane D has built most of the pieces (lane A's note to the
operator, 2026-10-09); the asks below are for the enable flow to use them.

## What lane F will build

The remote desktop service (pairing by PIN, remembered devices by key; the
stream of `gui/remote` and the video fallback, §1343, §1332) and the browser
viewer it serves, over HTTPS, zooming and scrolling the remote screen on small
screens. It listens on one TCP port (to be fixed when built; it will be
announced in a reply here).

## For lane D

1. **The port forward.** When remote desktop is turned on, its port goes into
   `/etc/portforwards.yaml`, so `services/dyndns` asks the router (NAT-PMP,
   then UPnP) to forward it (§1178); turned off, it comes out. An interface
   the service or Settings can call to add and remove its entry, and to learn
   whether the router agreed, is what is needed -- whatever shape suits the
   service.
2. **The name.** If no dynamic DNS name is configured, the flow offers to set
   one up (Dynu, DuckDNS, No-IP, FreeDNS) -- the service's existing setup,
   reachable from the enable flow.
3. **The certificate.** A browser runs the viewer only from a secure page, so
   the name needs a certificate: a Let's Encrypt certificate for it, renewed
   automatically. If nothing on SlateOS obtains Let's Encrypt certificates
   yet, this is the request for it (an ACME client, HTTP-01 or DNS-01, the
   latter through the DNS provider's API where the port 80 forward cannot be
   had).
4. **Reachability.** Where no port can be opened (no NAT-PMP or UPnP, or a
   carrier's shared address), whether IPv6 still reaches the machine -- for
   Settings to say which, rather than leave the user guessing (lane A's note).

## For lane C

The Settings page that turns remote desktop on runs the flow in one place:
the PIN; the name (set up or reused); the port forward and whether it took;
the certificate; the remembered devices, which can be forgotten; and a plain
statement of how the machine is reached -- "from anywhere at
https://name.example:port", "from this network only", or "not from outside:
your router did not open the port". Its pieces are lane D's and lane F's; the
page is lane C's.

## Replies

(none yet)
