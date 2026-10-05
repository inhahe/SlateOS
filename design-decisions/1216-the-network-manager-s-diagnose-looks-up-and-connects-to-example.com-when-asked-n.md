## 1216. The Network Manager's Diagnose looks up and connects to example.com -- when asked, never on its own

**Date:** 2026-09-26
**Lane:** E
**Decided by:** Claude (autonomous) -- Claude's to revisit

**In short:** Diagnose used to invent a passing report, then refused to run.
It now checks the machine: whether a network card is up, whether it has an
address and a gateway (read from the kernel), and then -- off the window's
thread -- whether a name can be looked up and a connection made. Those last
two need somewhere to look up and connect to; it is `example.com`, port 80,
and nothing is contacted until Diagnose is pressed.

**Why `example.com`:** it is reserved by IANA for exactly this kind of
illustrative use (RFC 2606), has answered HTTP on port 80 for decades, and
belongs to no company whose logs would learn that a SlateOS machine exists.
A large provider's host (a search engine's, an OS vendor's connectivity
check) would be as reachable and would tell that provider on every click.

**Alternatives:** the DNS server the kernel names (a lookup there proves
little about the wider network, and a resolver need not answer TCP port 80);
the gateway (gateways rarely listen on anything); a host the operator
chooses (a setting nobody has yet, and the question can be reopened when
one exists -- `open-questions.md` E-Q2 is where the related "which remote
services" question already sits).

**What it costs:** one DNS query and one TCP handshake to IANA per click.
No request is sent over the connection; it is closed once made.
