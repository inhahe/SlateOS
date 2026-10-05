## 575. WiFi starts as two shared root crates — `net80211` for the wire format and `aes` for the cipher — before any radio exists to run them

**Date:** 2026-09-01

**Lane:** C

**Decided by:** Claude (autonomous)

**In short:** The roadmap says the WiFi supplicant (the program that types the
password into a wireless network for you) is done. It is not: what exists
turns a password into a key and stops there. There is no code anywhere in the
tree that can read or write a single WiFi packet, and — checked, not assumed —
no AES anywhere either, AES being the cipher every modern WiFi network's
security is built on. The decision is *where the missing pieces go*: two new
small libraries at the top of the tree, `net80211/` and `aes/`, rather than
code inside the supplicant or inside the future wireless driver. Nothing
observable changes yet; this is the foundation the rest of WiFi is built on.

**Why this is the first thing built, and not the radio.** Operator decision
§264 puts WiFi ahead of Mesa and Chromium, on the grounds that a machine that
cannot reach a network has nothing for a browser to display. Within WiFi,
the wire format is the part that has no prerequisites: a beacon frame has
the same layout whether it arrives from real hardware, from a simulated
radio, or from a test vector typed into a unit test. So it can be built,
and fully tested, while the thing that would carry it does not exist. The
radio itself is kernel-side and therefore lane A's; a request for a
`mac80211_hwsim` equivalent follows this entry rather than preceding it, so
that it can cite working code instead of a plan.

**Why `net80211` is a root crate and not a module of the supplicant.** Three
separate consumers need exactly these bytes, and they live in three different
places:

| Consumer | Where | Needs it for |
|---|---|---|
| the wireless driver | `kernel/` — lane A | building probe requests and parsing beacons, before any of the rest of the stack exists |
| the supplicant | `userspace/wpa` — lane B | the EAPOL-Key frames its state machine has nothing to run over today |
| the netstack | `services/netstack` | seeing a data frame as Ethernet once the LLC/SNAP shim is off |

This is precisely the shape §610 settled for DEFLATE, arrived at from the
other direction: there, two copies already existed and one was promoted;
here, no copy exists yet and the promotion is done up front. The lesson §610
draws — that a parser of untrusted input is the worst thing to duplicate,
because each copy is independently an attack surface and a fix to one leaves
the other broken — applies with more force here, not less. Every byte
`net80211` parses arrives from the air, from anyone, unauthenticated, before
any key has been agreed. A malformed frame is the *expected* case.

**Why `aes` is a separate crate from `net80211`.** WiFi is its first caller
but plainly not its last: disk encryption already names `Aes256Xts` in
`kernel/src/fs/diskencrypt.rs` with nothing behind it, and TLS will want the
same cipher. Folding it into `net80211` would mean the disk encryptor
eventually depends on a WiFi library, which is the same inversion §610
rejected. The `crc32` crate's own history is the argument: that polynomial
had been written **four separate times** before it was factored out. A block
cipher is a far worse thing to get four copies of than a checksum — a
mistyped S-box entry does not produce a detectably wrong answer, it produces
a cipher that interoperates with nothing, and the symptom looks like a
network fault rather than a crypto bug.

**Members, not `exclude`d.** `netproto`, `netipc`, `netring` and `tzrules` are
excluded from the workspace, and mirroring them here would have been the
obvious move — it was in fact the original plan. It is wrong. Those four are
excluded for one specific reason: they link into `services/netstack`, which is
built as a bare-metal target *outside* the workspace. Every consumer of
`net80211` and `aes` is inside it. And exclusion has a real cost: an excluded
crate does not inherit `[lints] workspace = true`, so it silently opts out of
the tree's `indexing_slicing` / `arithmetic_side_effects` / `unwrap_used`
lints — the exact lints that matter most in a parser of hostile input and in
a cipher. Membership is the default; exclusion needs the netstack-shaped
excuse.

**The AES here is not constant-time, deliberately, and says so.** It uses
table lookups indexed by key-dependent bytes, which leak through the cache to
anything that can measure it. Constant-time AES on x86 means AES-NI, which
means `unsafe` intrinsics, and both crates are `#![forbid(unsafe_code)]`
today. The tradeoff is acceptable for what actually calls it — unwrapping a
group key once per handshake, and keys at rest — and would not be for bulk
encryption on a machine running untrusted local code. That boundary is
written in the module docs at the top of the crate, not buried in a comment,
because the next caller is the one at risk from it. Revisit when either a
bulk-data caller appears or the tree grows a place where `unsafe` intrinsics
are allowed.

**The inverse S-box is derived, not transcribed.** `INV_SBOX` is built at
compile time by inverting `SBOX` in a `const` block. Two hand-typed 256-entry
tables are two chances to make a typo that no test catches unless it happens
to touch the wrong byte; one table plus an inversion makes the entire class of
error unrepresentable, and a test asserts the round-trip on all 256 values
anyway.

**Where it lives:** `net80211/` — `frame` (MAC header), `mgmt` (beacon, auth,
assoc, deauth bodies), `ie` (information elements), `rsn` (the security
element), `llc` (the SNAP shim to Ethernet), `fcs` (the frame checksum, via
`crc32`), `eapol` (802.1X key frames and the 4-way handshake classifier).
`aes/` — `lib` (the cipher) and `keywrap` (RFC 3394, how the group key
arrives inside message 3). Registered in the root `Cargo.toml` `members`, in
`scripts/pre-boot.py`'s lane map and `scripts/which-lane.py`, and in the
`roadmap.md` ownership table, all as lane C.
