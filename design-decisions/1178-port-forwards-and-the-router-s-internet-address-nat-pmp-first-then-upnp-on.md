## 1178. Port forwards and the router's internet address: NAT-PMP first, then UPnP; only the default gateway believed; forwards kept by the dynamic-DNS service

**Date:** 2026-10-06
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** a computer at home can be reached from the internet only if
the home router passes ("forwards") a port on its internet side through to
it. `design.txt` asks for "port range passthroughs in router via UPnP or
NATPMP, whatever's detected", and for the router's address and the
network's internet address to be shown. The dynamic-DNS service
(`services/dyndns`) now does both:

- it finds the router and asks it its internet address;
- it asks the router for each forward listed in `/etc/portforwards.yaml`;
- it says what happened in `/run/portforwards.yaml`, which Settings reads.

A custom dynamic-DNS URL that names `{ip}` now takes the router's address.
The protocols are in a crate of their own, `services/dyndns/router`
(`dyndnsrouter`). Several choices had more than one reasonable answer.
Here they are, and why.

**1. NAT-PMP is asked first, then UPnP. PCP is not spoken yet.**

| Option | *What changes* | For | Against |
|---|---|---|---|
| **A. NAT-PMP (RFC 6886), then UPnP IGD** (chosen) | one datagram to the router, then a multicast search if it does not answer | NAT-PMP answers or refuses in one round trip; between them, the two cover Apple's routers, miniupnpd's (OpenWrt and its kin) and most consumer routers | a router that speaks only PCP is not found |
| B. PCP (RFC 6887) first | the same, with NAT-PMP's successor in front | PCP also opens IPv6 pinholes | IPv6 forwarding needs IPv6 here first; miniupnpd, the common PCP server, answers NAT-PMP too |
| C. UPnP first | a search, a description and a SOAP call before anything is known | the most widespread | three exchanges where one would do |

PCP's IPv6 pinholes are the reason to add it later, once the network stack
carries IPv6.

**2. Only the default gateway is believed.** Neither protocol is
authenticated, so anything on the network can answer a UPnP search. A
NAT-PMP answer must come from the gateway. A UPnP device must answer the
search from the gateway's address. Its description, and the control URL
inside it, must point at the gateway. miniupnpc, the common client, takes
any device that answers. Believing any device would let a machine on the
network claim to be the router: it would be told where forwards should go,
and could hand the dynamic-DNS service an address of its choosing to
publish. A router whose UPnP service is on another address (rare: a modem
in front of the router, say) is not used, and the status says why.

**3. Forwards are a second file kept by the same service, not a service of
their own.** `/etc/portforwards.yaml` holds a mapping keyed by each
forward's name, as `/etc/dyndns.yaml` does: `yamldoc`, the library that
keeps a user's comments when Settings edits the file, cannot read a list of
mappings. Each forward has:

- `protocol`: `tcp`, `udp` or `both`;
- `port`: one port, or a range such as `6881-6889`, at most 100 ports;
- `external_port`: the router's internet-side port, the same as `port`
  when left out;
- `enabled`.

A port can be forwarded by one forward only; a second is refused, and
named. The service is the dynamic-DNS one because both jobs need the
router: lane E's request put the forwards there, and lane D agreed on
2026-09-27. One process finds the router once for both.

**4. Leases, and when the router is asked again.**

| What | Chosen | Why |
|---|---|---|
| NAT-PMP lifetime asked | 2 hours, RFC 6886's recommendation | a forward this computer stops wanting (it crashed, it left the network) does not stay open long |
| UPnP lease asked | 1 hour, as miniupnpc asks | the same |
| Asked again | at half of what the router granted | RFC 6886's advice, used for both |
| A UPnP router that keeps only permanent forwards (error 725) | given a permanent one | it has no other kind. The service removes it when the forward is taken out of the file, and keeps asking until the router agrees, which a lease would have done by itself |
| The router's address asked again | every 15 minutes | a NAT-PMP router's restart (RFC 6886's epoch going backwards) means every forward is asked for again at once |
| A refusal asked again | after 30 minutes, or at once when the forward is changed | the change may be the fix |
| No router found | looked for again every 5 minutes, and at once when the network changes | |

**5. A port someone else holds is left to them.** When the router already
forwards the external port, the service asks it to whom. If that is this
computer, and the same port here, it is a forward left by an earlier run.
It is then removed and made again: some routers answer "conflict" even to
the computer that holds the forward, where the specification has them
overwrite it. Anything else -- another computer, or this one's other port
(a game may have asked for it) -- is left alone, and the status names who
holds it.

**6. NAT-PMP is given four sends, not nine.** RFC 6886 has a client send
nine times, the last wait 64 seconds, over two minutes in all, before it
concludes that the router does not speak NAT-PMP. The service sends four
times (0.25, 0.5, 1 and 2 seconds: 3.75 seconds in all). A router on the
same network that has not answered by then is not going to, and the service
asks again at its next check rather than stopping everything else for two
minutes. A router that answers "port unreachable" is taken at once.

**7. A router behind another router publishes nothing.** If the router
reports an internet address that is private (10/8, 172.16/12, 192.168/16)
or shared (100.64/10, an internet provider's own NAT), then another router
stands between it and the internet. The router's address is then not this
network's address, and its forwards do not reach this computer from
outside. The status says so. A custom dynamic-DNS URL waits instead of
publishing an address no one outside can reach. The forwards are still
made, so they work once the outer router forwards too.

**8. Only a custom URL takes the router's address.** No-IP, Dynu and
Cloudflare still learn the address from their own pages (decision 1175).
A page shows the address the internet sees, which is right even behind two
routers. A custom URL that names `{ip}` has no page of its own to ask, and
decision 1175 forbids asking a third party, so it asks the router.

**Revisit:** 1, when the network stack carries IPv6 (PCP's pinholes). 6,
if routers on slow links are found to miss the four sends.
