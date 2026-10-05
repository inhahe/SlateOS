## TD-C-A-NETWORK-MANAGER-THAT-CANNOT-SEE-THE-NETWORK -- FIXED 2026-09-15

**In short:** `apps/netmanager` showed the user their network interfaces, the
Wi-Fi networks in range, their VPN connections and a clean diagnostic report.
It has no network access of any kind. All of it was written into the program by
hand, and one of the invented facts was that a VPN was connected.

**Date:** 2026-09-15. **Lane:** C.

The crate has no `std::net`, no socket syscall and no filesystem access. What it
displayed: three interfaces with addresses, link speeds and states; five Wi-Fi
networks with SSIDs, signal strengths, channels and **security types**; three
VPN configurations; sixty seconds of throughput history; and, on request, a
diagnostic report in which the gateway answered in 1.2 ms, DNS resolved, a
traceroute found twelve hops at 45 ms, packet loss was 0% over 100 pings and
MTU 1500 was confirmed.

**The worst single line was the VPN state vector.** It was initialised to
`[Disconnected, Connected, Disconnected]`, so **opening the network manager
showed a VPN as connected**. Of every false belief in this sweep, that is the
one that is acted on by *transmitting* something. A person who believes their
traffic is tunnelled uses the connection differently from one who knows it is
not -- different sites, different files, different networks they are willing to
do it from. There is no taking that back afterwards, which puts it alongside
the recovery and partition tools rather than alongside the stub pages.

**Second worst is the diagnostic report, for a subtler reason.** It *passed*.
A diagnostic is what somebody runs while troubleshooting, so a fabricated
passing report does not merely misinform -- it actively redirects the search.
It tells the user the network is fine and the problem is elsewhere.

**Eight acts it could not perform, all of them reported as done:** applying an
IP configuration ("IP configuration updated for eth0"), connecting to a Wi-Fi
network, toggling a VPN, bringing an interface up, bringing an interface down,
running diagnostics, rescanning ("Scanned: 5 networks found"), and the
interface status summaries derived from all of it.

**Two of the refusals needed more thought than the others, and both are
general.**

*A switch must not move.* Every other refusal in this sweep leaves a list
empty. The VPN and interface toggles had to leave a **control** alone instead,
and that is a stronger requirement: a list that fills is a claim about the
world, but **a switch that moves is a claim that the thing it controls moved
with it**. Leaving the switch flipped while printing "cannot connect" would be
read as the message being stale, not the switch being wrong.

*A clear is a claim.* Refresh does not empty the Wi-Fi list. Clearing it would
assert "the networks that were here are gone", which this program knows no
better than it knows what is there. This is the second time in one day -- I
made the same mistake in `undelete`, writing `RecycleBinReader::scan` as a
clear, and caught it there too. **The no-op and the clear look identical in a
diff and mean opposite things.**

**Emptying the lists was only half the fix, again.** An empty interface list
claims the machine has no network hardware. An empty Wi-Fi list claims nothing
is in range. An empty VPN list claims none is configured. And the status bar
said "0 interfaces" outright. The window now carries an unconditional
three-line banner; the third line is the one doing the work:

> Every list below is empty because nothing was examined -- not because nothing
> is there.

It is drawn unconditionally rather than behind an `is_empty()` check, because
there is no state in which this app *can* see the network, and a condition that
is always true is a condition that rots the moment it stops being.

**66 of 139 tests rested on the invented data** -- the ninth application in a
row and the largest share yet, just under half. The split is worth recording,
because it is the same split every time:

* **48 were interaction tests** -- sidebar scrolling, DNS reordering, caret
  placement, hit testing, wheel fractions. They needed *some* interfaces to
  interact with, not specifically invented ones. They moved to a
  `with_sample_data()` fixture and kept their full value.
* **18 asserted the fabricated acts directly** and were rewritten to assert the
  refusals: that the VPN switch does not move, that Apply does not write the
  address, that Diagnose produces no report.

The lesson repeats: **production fixture data is load-bearing for tests nobody
recorded as depending on it**, and the tests that break loudest are the ones
that never mentioned the fixture at all.

**Update 2026-09-26 (lane E): the interface list is now the machine's.** It is
read through `hwquery` from SlateOS's `/proc/net` on opening and on Refresh,
so "there is no state in which this app can see the network" stopped being
true, and the unconditional banner went with it. The banner is drawn only
while the list is empty, and says which of three things that means: not read
yet, not readable (with the reason), or read and **no card found** -- the one
empty list that is now a finding. The status bar counts only after a read
that worked. The Wi-Fi and VPN tabs, which the banner used to explain, say
for themselves that nothing here can scan for or read one. Everything else
above still refuses, in the same words.
