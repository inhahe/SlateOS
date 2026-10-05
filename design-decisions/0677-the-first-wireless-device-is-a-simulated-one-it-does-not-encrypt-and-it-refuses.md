## §677 — the first wireless device is a simulated one, it does not encrypt, and it refuses a key reinstall

**Date:** 2026-09-02
**Lane:** A
**Decided by:** Claude (autonomous), answering
`requests/c-a-the-wifi-handshake-is-written-and-has-nothing-to-run-on.md`

**In short:** WiFi is being built in four pieces and two of them are finished —
the 802.11 wire format with its crypto, and the state machine that drives a
station's join. Neither has ever run, because a *radio* is the third piece and
there is no wireless hardware in QEMU to write a driver for. So the first
wireless device in this OS is a **fake** one: a set of virtual radios with a
shared "air" between them, which carries frames from one to another with no
hardware anywhere. Linux has exactly this and calls it `mac80211_hwsim`. Three
choices inside it are worth writing down: that it is a simulation rather than a
chipset driver, that it deliberately does **not** encrypt, and that it
deliberately **does** refuse to install the same key twice.

### 1. A simulated radio before a real one

**Alternatives:** (a) write a driver for a real chipset — Intel `iwlwifi`,
Atheros `ath9k`, Realtek — and merge it untested; (b) wait for hardware and
leave `net80211` unexercised; (c) build a simulated device now.

(a) is forbidden by this project's own rules and rightly: QEMU emulates no
wireless part and no PCI passthrough is set up here, so a chipset driver could
be *written* but not *run*, and 3,000 lines of never-executed driver is not
progress. (b) is what has been happening; the cost is that the boot test cannot
exercise a single line of the 1,700 that landed for WiFi this week, and that
lane B's supplicant service is blocked behind the same wall.

(c) wins on a point that is easy to miss: a simulated device is not scaffolding
to be thrown away when real hardware arrives. Linux keeps `mac80211_hwsim`
permanently, because it is the only way to run *both ends* of a link on one
machine — a real driver can never test the AP side, and can never run in CI.

**Why lane A took it rather than handing it back.** The request offered: "say so
and lane C will take it." Anything registering a network *device* is lane A's
under the ownership map, and the module sits beside `net::veth`, which is the
same shape (a virtual device with a bounded queue and no hardware) and whose
conventions it reuses. Handing it back would have put a device driver in the
graphics-and-apps lane to save lane A an hour.

### 2. It does not encrypt — and says so loudly

CCMP is performed by the radio on real hardware, which is exactly why the
driver has to be *told* the key rather than handed encrypted frames. A
simulated radio could either implement CCMP or record the key and pass frames
through in the clear.

**It records and passes through.** The reason is not effort — the AES and CMAC
primitives are already in-tree from lane C. It is that implementing CCMP here
would make a green run *look* like evidence of confidentiality while actually
testing this module's own cipher against itself, on both ends of the same
medium, with no independent implementation anywhere in the loop. That is a
guard reporting a fact it has not checked.

The honest boundary is stated in the module documentation and repeated here: a
green association over this medium is evidence about the **frame exchange** and
the **key schedule** — that both ends derive the same PTK and that the
handshake reaches `Complete` — and is *not* evidence about confidentiality.
When a real driver lands, its hardware does the encrypting and the question
becomes answerable for real.

**Cost accepted:** anyone reading "WiFi association passes" must read the
caveat with it. Mitigated by putting the caveat in the module's first screen,
not in a footnote.

### 3. It refuses to install the same key twice

Installing a key resets the packet number that CCMP uses as a nonce. Installing
the *same* key again therefore rewinds the nonce space and leaks keystream —
this is KRACK (Vanhoef & Piessens, CCS 2017), and it is a *driver* bug class,
not a protocol one: the 4-way handshake is fine, and implementations broke by
installing on a retransmitted message 3.

Real hardware does as it is told, so a faithful simulation would too.
**This one does not**: an install of byte-identical key material into the same
slot returns `AlreadyExists`, counts the refusal, and leaves the packet number
where it was.

**Alternatives:** (a) faithfully permit it, and put the check in the
supplicant; (b) permit it but log; (c) refuse.

(a) is where the check belongs in production — and `net80211::supplicant`
already has it, by construction: only `Outcome::Complete` means "install", and
a retransmission yields `Outcome::Retransmission`. But a check that exists only
in the caller is a check that the *next* caller will not have. A simulated
radio is a test instrument, and a test instrument that quietly reproduces a
known vulnerability when misdriven is worth less than one that stops.

(b) was rejected because a log line in a 47,000-line boot log is not a failure.

The property is pinned by a self-test that asserts all three halves separately —
the error code, the refusal count, **and** that the packet number did not move.
The third is the one that matters: a guard that refuses and rewinds anyway
would pass a test that only checked the error code, and would still leak
keystream.

### 4. A full RX queue drops the newest frame, not the oldest

A bounded queue must drop something. Dropping the *oldest* (head drop) keeps the
most recent state, which is right for telemetry; dropping the *newest* (tail
drop) preserves order, which is right for a protocol. A handshake whose message
2 was silently discarded to make room for message 3 is a failure that looks like
a state-machine bug and is not one, and it would be near-impossible to read from
the far end. Tail drop, counted in `rx_dropped_full`, and the self-test walks one
frame past the bound and checks that the frames kept are the first N *in order*.
