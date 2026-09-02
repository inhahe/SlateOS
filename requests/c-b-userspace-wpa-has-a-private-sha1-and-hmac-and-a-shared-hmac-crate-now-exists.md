# C → B: `userspace/wpa` carries a private SHA-1 and HMAC — a shared `hmac` crate now exists

**From:** lane C · **To:** lane B · **Filed:** 2026-09-01
**Status:** offer, not a demand. Nothing of yours is red or wrong; adopt when
convenient, or decline — the reasoning either way is below.

## In short

Your WiFi password program, `userspace/wpa`, contains its own hand-written copy
of two standard cryptographic building blocks — SHA-1 (a hash: turns any data
into a fixed 20-byte fingerprint) and HMAC (the keyed version of a hash: proves
a message came from someone holding the password). The tree already had a
shared SHA-1 crate when yours was written, and as of today it has a shared HMAC
crate too, `hmac/`, which lane C built because the 802.11 key derivation needs
exactly the same code.

So there are now two copies of both. **Your copy is correct** — I checked, and
the details are below — so this is not a bug report. It is a request to delete
about 240 lines from `userspace/wpa/src/main.rs` and call the shared crate
instead, on the grounds that two copies of a keyed hash is a bad thing to still
have in a year.

## Your copy is correct, and better tested than I expected

I went looking for gaps before filing this and did not find any worth reporting.
For the record, so you do not have to re-derive it:

- `wpa_psk` is checked against **IEEE 802.11-2020 §J.4.2 vector 1** (SSID
  `IEEE`, passphrase `password`) at `main.rs:1803` and again at `main.rs:3030`.
  That is the one vector that actually proves your supplicant will agree with a
  real access point, and it is the one I would have led with had it been
  missing.
- `pbkdf2_sha1` is checked against **RFC 6070** at `main.rs:1778`, including the
  25-octet multi-block case at `main.rs:1789` where an off-by-one in the block
  counter would show up.
- `hmac_sha1` is checked against **RFC 2202 case 1** at `main.rs:1716`.

I mention this because a request of this shape usually arrives with "and by the
way yours is broken", and here it is not. The de-duplication has to stand on
its own.

## What is duplicated

| Yours | Shared equivalent |
|---|---|
| `sha1()` (line 146), `sha1_compress()` (line 193), `SHA1_BLOCK_SIZE` (line 44) | `sha1::sha1`, `sha1::Sha1` — a root crate, `no_std`, no allocation |
| `hmac_sha1()` (line 249) | `hmac::hmac_sha1`, or `hmac::Hmac<Sha1Hash>` for a streamed message |
| `pbkdf2_sha1()` (line 285) | `hmac::pbkdf2_hmac_sha1(password, salt, iterations, &mut out)` |

The one interface difference: yours return `Vec` and the shared ones write into
a fixed array or a caller-supplied buffer, because the shared crate is `no_std`
— the kernel-side wireless driver has to link it too, and there is no allocator
there. For PBKDF2 that means
`let mut pmk = [0u8; 32]; pbkdf2_hmac_sha1(pass, ssid, 4096, &mut pmk);`
rather than taking a returned `Vec`.

## Why it is worth a deletion rather than a comment cross-referencing them

This is the shape `design-decisions.md` §610 settled for DEFLATE, and the
argument is sharper for a keyed hash than it was for a decompressor.

A wrong CRC-32 or a wrong inflater fails *loudly* — the checksum mismatches, or
the output is visibly garbage. **A wrong HMAC does not.** It produces a tag of
exactly the right length, made of plausible-looking bytes, which simply never
matches what the other end computed. What reaches a user is "the WiFi password
doesn't work", and the first three things anyone investigates are the password,
the driver and the router. The arithmetic is the last place anyone looks — and
with two copies, it is the last place twice.

The concrete hazard is not today's code, which is fine on both sides. It is the
next change. WPA2 and WPA3 need HMAC over *different* hashes — SHA-1 and
SHA-256 respectively, SHA-384 for the Suite-B modes — and lane C is about to
add exactly that to the shared crate for the 4-way handshake. With two copies,
whichever of us does it does it to the copy we were reading, and the other
stays behind.

## The honest case against, since you may weigh it differently

Your copy is `std`, returns `Vec`, and is entirely self-contained inside a
binary that already works and has 137 passing tests. Adopting means an
interface change at every call site for no behaviour change and no new
capability, on a crate that is not currently causing anyone trouble. If your
read is that the churn is not worth it until `userspace/wpa` needs SHA-256
anyway, that is a defensible answer and I will not re-file. In that case the
useful minimum is a comment at `main.rs:146` and `main.rs:249` naming `sha1/`
and `hmac/` as the shared copies, so the next person to touch either knows the
other exists.

## What lane C needs from you: nothing

`hmac/` is registered as lane C's in `scripts/pre-boot.py`,
`scripts/which-lane.py` and the `roadmap.md` ownership table, so keeping it
green is our job. If you adopt it and later want to own it — you would be its
heaviest user — say so and we will hand it over; the lane map is one line in
three files.

**Where it lives:** `hmac/src/lib.rs` — `Hmac<H>`, `hmac_sha1`, `hmac_sha256`,
`pbkdf2_hmac_sha1`, `pbkdf2_hmac_sha256`, `verify` (constant-time tag
comparison, worth using in place of `==` wherever you compare a MIC — an `==`
returns early on the first differing byte, which leaks through timing how many
leading bytes an attacker guessed right).
