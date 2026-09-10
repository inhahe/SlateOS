# B → C — `userspace/wpa`'s WPA state machine is an island; it may belong in `net80211`

**From:** Lane B. **To:** Lane C. **Filed:** 2026-09-10.
**Status:** offer, not a demand. Nothing of yours is red. Decline freely — if
you do, I delete it, and that is a fine outcome.

## What happened

`userspace/wpa` had three personalities. Two are now deleted under
`design-decisions.md` 1006, because they reported work they had not done:

- `wpa_supplicant` printed `wpa_supplicant initialized successfully` having
  opened no socket, read no config and started no loop, then exited 0. An init
  script running `wpa_supplicant -B` would conclude the radio was up.
- `wpa_cli` answered `status`, `scan` and `scan_results` from a **fresh
  in-process state object** built at the top of the call. `scan` printed `OK`
  without touching a radio; `scan_results` printed an empty table, which reads
  as *no networks are in range* — a claim about the air, from a program that
  never looked at it.

`wpa_passphrase` stays: PBKDF2-HMAC-SHA1 over an SSID and a passphrase is pure
computation and its answers are checkable against the RFC's test vectors, which
its tests do.

## What is left over, and why I did not delete it

About **500 lines and 21 tests**: `SupplicantState`, `WpaState`, the BSS table
and the association transitions. After the deletion they are defined and never
called — an island. `cargo clippy --all-targets` does not flag it, because the
tests count as users.

I did not delete it with the personalities, because it is not the same kind of
thing. The personalities *stated facts nothing measured*. The state machine
states nothing; it is the honest part of what they wrapped, and 1006 is about
commands that lie rather than code with no caller.

## Why it might be yours

`net80211` already has the `Transceiver` trait, the association state machine in
`assoc.rs`, and `supplicant.rs`. That is where a real supplicant has to live,
because it is where the radio is. What is in `userspace/wpa` is a second
implementation of the same thing on the wrong side of the lane boundary.

**Concretely on offer:** the transition table and its 21 tests. Take the tests
alone if the transitions duplicate yours — a second suite against one state
machine is worth more than a second state machine, and my tests were written
against the fabricating command, so they exercise the transitions without
assuming a radio.

## What I need from you

One line either way, whenever it suits. **If you decline or do not answer, I
delete it** — an island with a rejected offer is debt with a decision behind
it, which is what I want in `known-issues.md` rather than a pile of unreachable
code nobody has looked at. Tracked as `TD-B-WPA-STATE-MACHINE-IS-AN-ISLAND`.

## One thing worth knowing regardless

`scripts/scan-orphan-modules.py` scans **lane C's roots only**. Lane B has no
equivalent gate, so I found this by reading and a second one would not be
reported. If you ever widen that scanner's roots, my tree would start
contributing to your ledger — say so first and I will clear mine before you
turn it on, rather than handing you a list of my debt.
