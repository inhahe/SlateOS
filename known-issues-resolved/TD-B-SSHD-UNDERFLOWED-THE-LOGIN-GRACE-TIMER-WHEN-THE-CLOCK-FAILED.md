### TD-B-SSHD-UNDERFLOWED-THE-LOGIN-GRACE-TIMER-WHEN-THE-CLOCK-FAILED

**Status: FIXED**, 2026-09-05. `clock_monotonic_ms` returns `Option<u64>`; an
unreadable clock refuses the connection instead of subtracting from zero.

**In short:** the server gives a peer that has not logged in yet a limited
number of seconds to do so — the "login grace time". It measured that by
reading the clock when the connection opened, reading it again during
authentication, and subtracting. The function it used to read the clock
reported **the number zero when the read failed**, which is indistinguishable
from "the machine booted this instant". So if the clock answered when the
connection opened and failed later, the server subtracted a real time from
zero. On unsigned arithmetic that goes below zero and wraps: in a debug build
it crashes the daemon, and in a release build it produces an enormous elapsed
time, so *every* connection is dropped the moment it arrives with "login grace
time expired". Either one is reachable by anyone who can open a socket, with no
password and no account.

### Where it lived

`userspace/sshd/src/main.rs`, `do_user_auth`:

```rust
let elapsed_s = (clock_monotonic_ms() - conn.connection_start_ms) / 1000;
```

against

```rust
fn clock_monotonic_ms() -> u64 {
    let ret = unsafe { syscall0(SYS_CLOCK_MONOTONIC) };
    if ret < 0 { 0 } else { ret as u64 }   // <-- failure spelled as a time
}
```

### Why it was invisible

`do_user_auth` carried
`#[expect(clippy::arithmetic_side_effects, reason = "not yet audited for panics
on peer-controlled input")]`. The lint that exists to find exactly this had been
switched off over the function, with a note saying the audit was owed. The
suppression was not hiding a false positive; it was hiding the true one. That is
the argument for treating the remaining 21 such suppressions as a queue of
unread bug reports rather than as settled exemptions — this was the first one
opened, and it contained a remotely-reachable panic.

It is the same shape as `PeerClosed` elsewhere in this file: a real condition
encoded as a value that an ordinary case can also produce, so no caller can tell
the two apart. `clock_monotonic_ms` had only two call sites, which is why the
honest fix was cheap.

### The fix, and the tradeoff inside it

`clock_monotonic_ms() -> Option<u64>`, `connection_start_ms: Option<u64>`, and a
grace check that **fails closed**: a clock it cannot read disconnects the peer
rather than treating the elapsed time as zero. Saturating subtraction on top,
so a clock that steps backwards cannot underflow either.

Failing closed is a real choice and is recorded in `design-decisions.md` §775.
The alternative — carry on with the timer disabled — hands unlimited connection
time to unauthenticated peers, which is precisely the population the timer
exists to bound, so a broken clock would switch the bound off for exactly the
wrong people. The cost is that a transient clock-syscall failure refuses logins;
the daemon already reasons this way about an unreadable host key, where it stops
rather than serving under a substitute identity.

### Also retired here

`parse_version_string`'s `#[expect(clippy::arithmetic_side_effects)]`. That one
audited clean — but rather than re-justify the suppression, the arithmetic was
removed: `strip_prefix` and `split_once` do the same work with no byte offset to
get wrong, so there is nothing left for a future audit to re-examine. Its three
`auth_attempts += 1` counters became `saturating_add(1)` for the same reason.

### The mirror-image defect: written *once*, and checked against nothing

**Status: FIXED for base64** (`userspace/sshwire/src/lib.rs`,
`every_one_of_the_sixty_four_digits_is_the_one_rfc_4648_names`). Found
2026-09-05 by mutation-testing `ssh-interop`'s new publickey tests, i.e. by
attacking the fix for this entry rather than the code it fixed.

**In short:** everything above is about a format written twice by two programs
that then disagreed, and the fix was to write it once in `sshwire` and have both
call it. That fix cannot be checked the way the bugs were found. Two ends that
share one implementation agree *by construction* — so an interop test, which
compares our client to our server, will pass no matter what that shared
implementation does. The remaining way to be wrong is to be wrong in the same
way at both ends, and the only thing it breaks is the one thing nothing in this
tree can speak for: talking to an SSH that is not ours.

**How it was demonstrated.** Swapping `'A'` and `'B'` in `sshwire`'s
`B64_ALPHABET` left every test in the tree green: 350 across `sshwire`,
`ssh-keygen` and `sshd`, and all 7 of `ssh-interop`. Base64 is now written
once and called by all three, the encoder and the decoder share the one
alphabet (the decoder builds its lookup table *from* it), so a corrupt table
round-trips perfectly and every one of our own comparisons agrees. A key file
written under it would be unreadable by OpenSSH, and one OpenSSH wrote
unreadable by us, with nothing failing anywhere else to say so.

`the_rfc_4648_test_vectors_encode_as_the_rfc_says` was already there and already
says exactly this in its own comment — *"a round-trip passes for any
self-consistent alphabet, including a wrong one"*. It is right; it just did not
go far enough. RFC 4648 §10's vectors are all `"foobar"` prefixes, and between
them `"Zg=="` … `"Zm9vYmFy"` use ten distinct digits. **Fifty-four of the
sixty-four table entries were pinned by nothing.** A published vector set is not
the same as coverage of the table it exercises, and that gap is invisible when
reading the test.

**The general rule.** Deduplicating a format removes the ability of our two ends
to disagree; it does not create any evidence that what they now agree on is
correct. A shared implementation of a *published* format needs a known-answer
test against the publication — and one that covers the whole of whatever table
or constant it turns on, not merely the part the standard's example vectors
happen to touch. The expected value must be transcribed independently, because a
test that reads the constant agrees with whatever the constant holds.

**The rest of the stack was audited against this and is clean.** Ed25519 has RFC
8032 §7.1 vectors and derives its curve constants from their definitions; SHA-256
and SHA-512 have the FIPS 180-4 examples; AES has the FIPS 197 vectors and
RFC 3686 for CTR mode. The group-14 prime is the interesting one: it is a
512-hex-digit transcription, so beyond checking both ends and the bit length it
is verified by Euler's criterion — `2^((p-1)/2) mod p == 1`, which a prime with
any single digit altered would almost certainly fail, since it would almost
certainly not be prime. That is the shape to copy: a check that the value is
*right*, not that it is self-consistent.
