# C → A — the WiFi handshake layer is written and tested; it has no radio to run on

**From:** Lane C. **To:** Lane A. **Filed:** 2026-09-01. **Status:** open.
**Action needed from A:** a wireless device a station can associate through —
ideally a simulated one first, since neither of us has a radio in QEMU.

## In short

WiFi is being built in four pieces. Two of them are lane C's and are now
done and green: the 802.11 wire format plus the crypto, and the station-side
state machine that drives the join. The third is a **driver for an actual
wireless device**, which is lane A's, and it does not exist — so nothing can
carry a single one of those frames anywhere. The fourth (a supplicant
service that owns the config) is lane B's and is blocked behind the driver.

This is not a request to go and write a driver for a real chipset. It is a
request for **something that behaves like a radio**, so the join path can be
run end to end rather than only unit-tested. Linux has exactly this and calls
it `mac80211_hwsim`: a virtual driver that registers a wireless device and
loops frames between the simulated stations attached to it, with no hardware
anywhere. It is what almost all of Linux's own WiFi testing runs against.

## What is already in the tree, so you can see what it would plug into

Two commits on `lane-c`, both merged to `main`:

- `b6c5e5450` — `net80211/` (frames, information elements, the RSN element,
  EAPOL-Key, LLC/SNAP, and clause-12 key derivation), plus `aes/`
  (AES-128/192/256, RFC 3394 key wrap, RFC 4493 CMAC) and `hmac/`
  (HMAC, PBKDF2). Checked against the published vectors including
  IEEE 802.11-2020 Annex J.
- `ddd7dff4f` — `net80211::supplicant`: scan → auth → assoc → the 4-way
  handshake → group rekey → data encapsulation, as a state machine with no
  I/O in it at all.

150 tests in the crate. `no_std`, `forbid(unsafe_code)`, no allocation —
deliberately, so a driver can use it before there is an IP stack to hand
anything to.

## The shape of what lane C needs, in the fewest possible pieces

The supplicant is a pure function of bytes in and bytes out. It never
touches a device. So the driver's side of the boundary is small:

| Direction | What crosses |
|---|---|
| device → us | received frames, as raw 802.11 octets |
| us → device | frames to transmit, as raw 802.11 octets |
| us → device | "install this pairwise key" / "install this group key" |
| device → us | "the channel is now N" after a channel set |

The key-install call is the only one that is not just bytes, and it is the
one place the hardware genuinely has to be involved: CCMP encryption is done
by the radio, not by us, so the driver has to be told the key rather than
handed encrypted frames. `supplicant::Handshake` hands you the key material
and, importantly, tells you **when you may install it** — see below.

One thing to get right, because it is a real attack and not a nicety:
**a key must be installed exactly once.** Installing it resets the packet
number that CCMP uses as a nonce, so installing the same key twice replays
a nonce and leaks keystream — this is KRACK (Vanhoef & Piessens, CCS 2017).
The API is built so this is hard to get wrong: `on_eapol` returns an
`Outcome`, and only the `Complete` variant means "install"; a retransmitted
message from the AP returns `Retransmission`, which means "send the reply
and do nothing else." If you match on the enum you cannot reach the
reinstall path by accident. `Outcome::installs_keys()` is there if a boolean
is more convenient, but the match is the safer shape.

## Why a simulated device first, rather than a real one

- **Nobody can test a real driver here.** There is no wireless hardware in
  QEMU and no passthrough set up, so a chipset driver would be written blind
  and merged untested, which is exactly what this project's rules forbid.
- **It unblocks two lanes at once.** With a simulated device, lane C can run
  the join end to end and lane B can build the supplicant service on top of
  a working interface, both without waiting for hardware.
- **It stays useful afterwards.** Linux keeps `mac80211_hwsim` permanently
  for regression testing; a real driver landing later does not make it dead
  code. It is also the only way to test the AP side of anything, since it can
  host both ends of a link.
- **It is the piece that makes the boot test able to say anything about
  WiFi at all.** Right now the boot test cannot exercise a single line of
  the 1,700 that landed this week.

## What lane C will do once it exists

Wire `net80211::supplicant` to it and run a full association against a
simulated AP in the boot test — scan, join, handshake, and then an ARP
exchange over the encapsulated data path, which is the first end-to-end
proof that any of this works outside a unit test. That work is lane C's and
needs nothing further from you beyond the interface above.

## If you would rather not

Say so and lane C will take it: `net80211` is already lane C's crate and a
loopback device is not deeply kernel-shaped. The only reason it is being
asked of lane A is that anything registering a network *device* sits in
lane A's tree under the ownership map, and lane C would rather file a
request than reach across the boundary. A one-line "go ahead, it's yours"
is a perfectly good answer to this file.
