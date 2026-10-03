## TD-B-THE-SSH-WIRE-LAYER-IS-WRITTEN-TWICE-AND-NOTHING-MAKES-THE-TWO-COPIES-AGREE (lane B) -- FIXED 2026-09-05

**Status:** FIXED, 2026-09-05. **All four items below are done.** Item 4 — the
interop test, the one that matters most and the reason this entry existed — is
`userspace/ssh-interop`, which links both peers as dev-dependencies, joins them
with `sshwire::memory_pair()`, runs each on its own thread and asserts they
derive the same RFC 4253 §7.2 session identifier from the same transcript. It
also pins that identifier to a recorded constant, because agreement alone cannot
see a change both ends make together — which is the exact state this stack was
in when the server hashed a placeholder client version and its own tests,
recomputing the hash the same wrong way, agreed with it. See the closing note.

**Items 1–3 are done.** Nothing in
the wire and transport layer is written twice any more: encoders, decoders, the
identification line, the key-exchange arithmetic, the transport crypto and — as
of the last change — the RFC 4253 §6 packet framing all live in
`userspace/sshwire`, and neither binary keeps a private copy of any of them.
The framing moved last and as `PacketCodec`, a stateful type rather than a pair
of free functions, because §6.4 makes the MAC `HMAC(key, sequence_number ||
packet)`: the sequence number is an *input to the framing*, not bookkeeping
kept beside it. See
`TD-B-THE-SERVERS-PACKET-DECODER-TRUSTED-A-LENGTH-IT-HAD-NOT-CHECKED-AND-A-MAC-IT-HAD-NOT-VERIFIED`
for the two faults that move fixed, and
`TD-B-THE-AES-CTR-COUNTER-IS-REUSED-BY-THE-CLIENT-AND-INVENTED-BY-THE-SERVER`
for the confidentiality bug that drove the crypto across.

`PacketCodec` narrowed the gap slightly on its own before item 4 landed:
`what_one_end_encodes_the_other_end_decodes` was the first test in this stack
that ran one end against the other rather than against its own output. It
covers the framing only, not the state machines above it.

Filed 2026-09-05 alongside the fix above, which is the first bug this
arrangement produced and the one that found it.

**In short:** the SSH client and the SSH server each contain their own private
copy of the same protocol plumbing — how to frame a packet, how to encode a
number, how to compute the handshake fingerprint. The two copies are supposed to
be identical, and nothing checks that they are. One of them drifted, and the
result was a server no client could connect to.

### Where

`userspace/ssh/src/main.rs` (4261 lines) and `userspace/sshd/src/main.rs`
(8775 lines). Duplicated between them, at minimum:

| Function | Purpose | Now |
|---|---|---|
| `compute_exchange_hash` | RFC 4253 §8 — **this is the one that drifted** | shared |
| `derive_key` / `derive_keys` | RFC 4253 §7.2 key derivation | shared |
| `ssh_string`, `encode_mpint` | wire encoding | shared |
| identification-line handling | RFC 4253 §4.2 — derives `V_C` / `V_S`, i.e. the *inputs* to the hash above; **drifted twice** | shared |
| `read_ssh_string`, `read_mpint`, `read_u32`, `read_byte`, `read_bool` | wire decoding — the server's copies were the *un-hardened* ones, see below | shared |
| `compute_mac`, `hmac_sha256`, `constant_time_eq` | RFC 4253 §6.4 packet MAC | shared |
| `aes128_encrypt_block`, AES-CTR | the cipher — **the second bug this arrangement produced**, and a confidentiality one | shared, as the stateful `Aes128Ctr` |
| `build_packet`, `read_packet`, `try_parse_packet` | RFC 4253 §6 framing | shared, as the stateful `PacketCodec` — **the third bug this arrangement produced**, two faults in the server's decoder |
| `BigUint` and its fourteen methods | RFC 4253 §8 key-exchange arithmetic | shared — **the fourth bug**, a pre-auth denial of service in the server |
| the group-14 prime and generator | RFC 3526 §3 | shared as `DH_GROUP14_P_HEX` — 512 hex digits that were transcribed twice and checked never |
| the KEXINIT cookie (RFC 4253 §7.1) | each end's only unpredictable contribution to `H` | **the fifth bug** — see below; the source is now shared as `sshwire::SecretSource`, the cookie itself is per-connection and stays each end's own |

**The fifth bug: neither end's KEXINIT cookie was random.** RFC 4253 §7.1 wants
sixteen random bytes because both KEXINIT payloads in full are fields of the
exchange hash, so the cookie is the only thing each end contributes that the
other cannot predict — the mechanism that stops either end steering `H` on its
own. `ssh` sent sixteen zero bytes. `sshd` sent
`sha256(b"sshd-kex-cookie")[..16]`, one constant compiled into the binary and
identical on every connection it had ever served, with a comment calling it
"pseudo-random". The client's was fixed on 2026-09-04; the comment left behind
there says the constant "gave that power to the server alone", written on the
assumption that the server's cookie was real. It was not, so `H` was a function
of the two DH public values alone.

This is the second fault of the shape *a fix reached one copy and had no way to
reach the other* — the first being the wire readers, which sshd carried in
their un-hardened form long after the client's had been rewritten. It is the
shape that most argues for item 4: a test that ran the two ends together would
not have caught this one either (both ends accept any cookie), but the class of
"I fixed the client, and believed something about the server that I had not
read" is exactly what an interop test makes impossible to sustain.

sshd's test for this asserted that the payload began with `SSH_MSG_KEXINIT` and
was longer than seventeen bytes — both of which a constant cookie satisfies for
ever. It certified the bug rather than catching it. It now asserts the cookie's
*provenance*: that the sixteen bytes are the ones the secret source supplied,
and that two connections do not share them.

The first extraction stopped at the line between total functions and fallible
ones: everything that returns a value moved as-is, while everything returning
`Result<_, SshError>` or `Result<_, SshdError>` could not, because a shared
crate cannot name either error type. That is now resolved by
`sshwire::WireError` (`Truncated { what, needed, available }` /
`LengthOutOfRange { len }`) plus a `From<WireError>` impl on each binary's error
enum, so every call site keeps using `?` and keeps reporting failures the way
the rest of that binary does. The readers moved on the back of it, the cipher
followed, and the framing last.

**One thing the framing move deliberately did not take.** `PacketCodec::encode`
takes the padding as a parameter rather than generating it, and offers
`padding_len` to say how much is wanted. Entropy is a syscall and it can fail;
a shared crate has no business deciding whether a daemon that cannot get any
should panic, send zeros, or drop the connection. Each binary answers that where
it can also report it.

**What the reader move turned up.** The two copies were not merely duplicated,
they were *unequal*, and the server had the worse one. Both had originally
guarded with `if offset + 4 > data.len()`, then indexed `data[offset]`. That
guard is itself the hazard: the addition it performs in order to decide whether
indexing is safe can wrap, and on wrapping it concludes that it is — so a large
`offset` produces an approved out-of-bounds index and a panic. The client's
copies had been rewritten to `get(..).and_then(first_chunk)` / `checked_add` at
some earlier point; sshd's had not, and there was no mechanism by which the fix
could reach it, because the function could not be shared. sshd reads these
fields *before* the client has authenticated, so the consequence there is worse
than in the client where it was fixed. `sshwire`'s test
`an_offset_near_the_top_of_the_address_space_does_not_wrap_into_a_read` pins the
case for both ends; neither crate's own tests had ever covered it.

Eight tests were deleted from sshd and one from ssh in the same change. They
tested each crate's private readers against the shared writer, which was the
right test while the readers were private; keeping copies of them would now test
the same functions twice and, worse, would go on passing if either crate grew a
private reader again.

`sha2`, `randrange` and `posix::ed25519` were already extracted into shared
crates for exactly this reason, so the precedent and the mechanism both exist.
The extraction simply stopped short of the protocol layer.

### Why it matters

Every one of these functions is a *contract between two programs*, and a
divergence in any of them is invisible to both test suites: each end tests its
copy against its own expectations and passes. The failure only appears when the
two are made to talk, which for the whole life of this stack nothing did.
`userspace/ssh-interop` is now the thing that does.

The severity of a divergence is not uniform, which is the trap — a drift in
`ssh_string` would break loudly and instantly, while the `V_C` drift produced a
signature-verification failure that reads like a *security* problem (wrong host
key? tampered handshake?) rather than a bug in our own encoder.

### What the proper fix looks like

1. Add `userspace/sshwire`, a library crate depended on by both `ssh` and
   `sshd`, alongside the existing `randrange`/`authlib` precedent.
2. Move the table above into it, with the RFC section for each item in its doc
   comment, and its tests with it.
3. Have both binaries call it. Neither keeps a private copy — a "just this one
   is different" exception is how the next drift starts.
4. Add a real interoperability test that drives the client against the server
   and asserts a session is established. This is the test whose absence let the
   `V_C` bug ship, and it stays valuable after the extraction: a shared crate
   makes the two ends agree about the parts they share and says nothing about
   the parts they do not — message ordering, state machines, who speaks first.

Item 4 was worth doing **even before** items 1–3: it is the check that catches
this whole class, extraction or no extraction. Items 1–3 got done first anyway,
and the extraction immediately demonstrated its own limit — reading the two ends'
version-line parsing side by side afterwards turned up
`TD-B-SSH-RE-ENCODED-THE-SERVER-VERSION-BEFORE-HASHING-IT` and then
`TD-B-THE-TWO-ENDS-DISAGREED-ABOUT-WHICH-CARRIAGE-RETURNS-ARE-FRAMING`: two more
never-agree-on-`H` bugs that a shared hash function does nothing about, because
what each end *passes* to the shared function was still each end's own business.
That is what drove the fourth row of the table above — the identification line is
not merely adjacent to the hash, it *is* two of the hash's eight inputs, so
deriving it is as much a two-program contract as hashing it, and it now lives in
`sshwire` on the same reasoning as everything else there.

Seven such bugs, plus the server's un-hardened readers, have now been found by
one person reading two files at once, and none by a test. The fourth
(`TD-B-THE-AES-CTR-COUNTER-IS-REUSED-BY-THE-CLIENT-AND-INVENTED-BY-THE-SERVER`)
is the one that shows how far a test suite can be from the truth here: *both*
crates had an AES-CTR test, both passed, and the test each had was structurally
incapable of seeing that the cipher was reusing one keystream for the whole
session — because encrypt-then-decrypt with the same wrong counter returns the
plaintext perfectly. The fifth and sixth
(`TD-B-THE-SERVERS-PACKET-DECODER-TRUSTED-A-LENGTH-IT-HAD-NOT-CHECKED-AND-A-MAC-IT-HAD-NOT-VERIFIED`)
make the neighbouring point: a wrong guard is invisible for as long as the only
caller happens to make it unreachable, and both of those were pre-auth. The
seventh
(`TD-B-THE-SERVER-SPENT-EIGHTY-SECONDS-OF-CPU-PER-HANDSHAKE-BECAUSE-THE-FIX-HAD-NOWHERE-TO-REACH-IT`)
is the one where the divergence was not a mistake at all: someone found the
client's key-exchange arithmetic unusably slow and fixed it properly, and the
fix simply had no route to the copy in the server, which is a worse place to be
slow. Duplication does not only propagate bugs; it blocks fixes. That ratio was
the argument for item 4, which has since been built.

The count above stops at seven because it was written before `ssh-keygen` was
brought into the same arrangement. It is now **eight live divergences out of
fourteen duplications**, and the eighth is the one that breaks the "found by
reading, never by a test" streak — see the `ssh-keygen` note at the end of this
entry. It was found by a test, on that test's first run, and reading would not
have found it.

**Known obstacle to item 4:** both crates' `syscall0/1/3/4` are host stubs that
return `-ENOSYS` on the Windows host build *and* on the WSL Linux build, so
neither `ssh` nor `sshd` can open a socket in a `cargo test`. An interop test
therefore either needs the two ends driven over an in-process transport, or has
to run under QEMU as a boot-test stage. The first is more work and more useful;
the second proves more. Neither is blocked on another lane.

> **The in-process route is now open, 2026-09-05.** The obstacle above used to
> continue "the handshake logic would have to be separable from
> `tcp_send_all`/`tcp_recv`, which today it is not". It now is. Both binaries
> talk to `sshwire::Transport` — four methods, `send`/`recv`/`readable`/`close`,
> with `send_all` as a provided method so the short-write loop is written once —
> and each holds a `Box<dyn Transport>` rather than a `u64` socket handle. The
> only code in either program that knows the protocol runs over TCP is a
> `TcpTransport` struct of about seventy lines at the top of each file.
>
> `sshwire::memory_pair()` is the other end of that: two connected in-memory
> transports, each direction a `VecDeque<u8>` behind a `Mutex` with a `Condvar`
> beside it. It **blocks**, exactly like the socket it stands in for — a `recv`
> with nothing to read waits until the peer writes or hangs up — so both ends
> can run unmodified on two threads. A non-blocking stand-in would have forced
> both programs to be restructured around polling *in order to be testable*,
> which is the tail wagging the dog, and would have meant the code under test
> was not the code that ships.
>
> Two things came out of the conversion itself, before any interop test exists:
>
> - **A ninth duplicated type.** `StreamBuffer` — the thing that holds a partial
>   packet, since a stream has no packet boundaries — existed in both binaries
>   with the same two fields and the same four methods, and as with the other
>   eight the two were not equal: the server's `fill_once` ended `&tmp[..n]`,
>   indexing a length the kernel reported, under the crate-wide panic-lint
>   suppression that used to sit at the top of that file. It is now
>   `sshwire::StreamBuffer`, and the kernel's number becomes a range in exactly
>   one place per binary, inside `TcpTransport::recv`, where it is refused if it
>   does not fit.
> - **A latent correctness bug in sshd.** The session loop decided whether a
>   client had hung up *orderly* or had failed by testing whether the error's
>   message string contained `"connection closed"` — in two places. Any future
>   error whose text happened to contain that phrase would have been reported to
>   the operator as a normal disconnect. It is now an `SshdError::PeerClosed`
>   variant, matched as a variant.
>
> What remains for item 4 is the test itself: `ssh` and `sshd` are both
> bin-only crates, so neither can be depended on. Each needs a `lib.rs` with a
> thin `main.rs` over it before a third crate can drive one against the other.

> **The second obstacle is entropy, and the fix chosen for it is injection,
> not a better host — 2026-09-05.** A key exchange needs a Diffie-Hellman
> private exponent, and drawing one goes through `randrange::fill_secret`,
> which on this Windows host returns `Unavailable` for every request. That is
> deliberate: `fill_from_kernel` is `#[cfg(not(unix))] -> Err(Unavailable)`, so
> that a test reaching for the system source on the host sees it decline, and
> "fails closed when there is no entropy" is a property something asserts.
> Downstream of it: four permanently-red tests in `userspace/ssh` (the DH
> exponent and the KEXINIT cookie), and no handshake, so no interop test.
>
> There are two ways out, and only one of them is lane B's to take.
>
> - **Give the host a real CSPRNG.** `fill_from_kernel` gains a
>   `#[cfg(windows)]` arm over `BCryptGenRandom`, and the fail-closed property
>   moves from "a platform we compile for" to "a filler a test hands over",
>   which then also exercises it on the SlateOS target, where it never ran.
>   This is written, tested and green, but it is **parked on branch
>   `lane-b-randrange-entropy` and not merged**: "the host has no entropy" is a
>   documented testing convention in lane C's tree, and about eighteen tests
>   across seventeen `apps/`/`gui/` crates assert it. Overturning another
>   lane's convention and reddening `main` is not a call lane B makes alone, so
>   it is queued for the operator in `open-questions.md` ("The test machine
>   cannot produce random numbers, on purpose…"), with the list in
>   `requests/b-c-a-pending-question-would-red-eighteen-of-your-tests-that-assert-the-host-has-no-entropy.md`.
> - **Make the SSH code take its byte source as a parameter.** `SshSession` and
>   `ConnectionState` carry a filler defaulting to `randrange::fill_secret`,
>   covering all three uses — the DH exponent, the KEXINIT cookie, the
>   per-packet padding. This is entirely inside lane B, it is the better design
>   on its own terms (what is under test stops depending on which platform it
>   was compiled for), and it yields a **deterministic** handshake, so the
>   interop test can assert the exact session identifier both ends derive
>   rather than merely that they agree on one.
>
> The second is what unblocks item 4, so item 4 is **not** waiting on the
> operator. The first would additionally un-red the four `userspace/ssh` tests
> for the whole tree rather than only along the injected path, which is why it
> is still worth asking about.
>
> `ssh` also gained `-o UserKnownHostsFile=` (`7a3bbb969`), which the test
> needs for a reason worth stating separately from the option's own merits: the
> client verifies the server's host key against `$HOME/.ssh/known_hosts`, and a
> test that drove the real client would either have to skip verification —
> testing the wrong program — or write into the operator's real trust store,
> which is a side effect no test is entitled to have.

> **Item 4 landed, 2026-09-05: `userspace/ssh-interop`.** The real client and
> the real server now complete a version exchange and a key exchange against
> each other in one process, and are asserted to derive the same session
> identifier. Two tests, both green.
>
> **Why a third crate.** A crate's own tests can only reach one side of a
> two-party protocol; to have both ends in one process something has to depend
> on both, and a crate cannot depend on itself. The crate ships nothing — its
> library is a page of documentation — and both peers arrive as
> `dev-dependencies`. That is also the last thing the `lib.rs`/`main.rs` splits
> in `ssh` and `sshd` were for: a bin-only crate produces no rlib.
>
> **Why the session id is the assertion.** RFC 4253 §7.2 makes it the exchange
> hash of the first key exchange, and both ends compute it independently from
> the same eight inputs: the two version strings, the two KEXINIT payloads, the
> host key, both DH public values, the shared secret. A disagreement about any
> of them — a field order, a length prefix, an integer encoding, a version
> string one side remembers differently — yields two different values. It is the
> sharpest single comparison available between these two programs, and it is the
> one that was missing while six divergences shipped.
>
> **Why agreement alone was not enough.** Two ends can agree on a value neither
> computed the way the RFC says; that is precisely what
> `TD-B-SSHD-SIGNS-AN-EXCHANGE-HASH-OVER-A-CLIENT-VERSION-THE-CLIENT-NEVER-SENT`
> was. So the transcript is pinned as well: with the host key seed and both
> secret sources fixed, every byte either end hashes is fixed, and the session
> id is one recorded constant. A deliberate protocol change is expected to
> update it, which is the point — it forces the change to be looked at from both
> sides at once.
>
> **What makes it reproducible.** A counting `SecretSource` whose counter is
> **thread-local**, and both peers on spawned threads. A shared counter would be
> drawn from by the two peer threads concurrently, so each peer's bytes would
> depend on the interleaving — a source that is deterministic in the sense that
> it uses no entropy and nondeterministic in the only sense that matters. A peer
> driven on the harness's own thread would likewise inherit whatever that thread
> had already drawn. Verified by three separate process runs producing the same
> identifier, not assumed.
>
> **What it does not cover yet.** It stops after key exchange. `NEWKEYS` is
> inside that (both `do_key_exchange` and `key_exchange` send and receive it),
> but nothing yet drives authentication, channel open, or a data round-trip
> through the encrypted transport — so the *state machines* above the handshake
> are still each end tested against its own expectations. Extending it that far
> is the obvious next increment, and it is cheap now that the harness exists.

> **The third program: `ssh-keygen`, 2026-09-05.** Everything above concerns two
> programs. There is a third, and it had diverged worse than either — twice, in
> ways that made it not merely inconsistent but *broken*, and both found in one
> sitting once it was pulled into the same arrangement.
>
> **Divergences 12–14: base64, the key container, and the public key blob.** All
> three were the same shape as the table above. `ssh-keygen` carried a fourth
> copy of base64 (the tree's third), a fourth copy of RFC 4253 §6's
> length-prefixed string, and a private key container of its own invention. All
> three now come from `sshwire`, and the crate gained a `lib.rs` with a
> three-line `main.rs` over it, for the same reason `ssh` and `sshd` did.
>
> **The seventh live divergence, and the first that a user could hit by
> following the documentation.** `ssh-keygen` wrote
> `-----BEGIN ED25519 PRIVATE KEY-----` around a bare `seed || public ||
> comment` blob. Nothing else in this tree — or anywhere else — can read that,
> so the documented two-command setup
>
> ```text
> ssh-keygen -t ed25519 -f /etc/ssh/ssh_host_ed25519_key
> sshd
> ```
>
> ended with the daemon refusing to start on the key its own key tool had just
> written. Both crates' suites were green: each tested its encoder against its
> own decoder, which is the one arrangement that structurally cannot notice a
> disagreement. Fixed in `7e43e99ce`; the container is now `openssh-key-v1`,
> unencrypted, from `sshwire`.
>
> **The eighth live divergence is the worst one in this entry, and it is the
> only one a test found rather than a reading.** The interop test written to
> close the item above — build a key with `ssh-keygen`, hand it to
> `sshd::HostKey`, then make the daemon serve a whole handshake on it — failed
> on its first run, but not on the container. `sshd` said *"the public key in the
> file does not match the private seed"*.
>
> `ssh-keygen` implemented Ed25519 itself: a `Fe` field element over five 51-bit
> limbs, an `EdPoint` in extended coordinates, a scalar multiply and a private
> SHA-512, about 560 lines. Given RFC 8032 §7.1's first test vector it derived
>
> ```text
> got e000725923fbbcd2f42112493aaf11599423a8fadb3a1e5630b6704e53591403
> rfc d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a
> ```
>
> So **the public half of every key this tool ever generated did not correspond
> to its private half.** An `authorized_keys` line copied from its output
> rejects its own owner; a host key it writes makes every client report a bad
> signature. This was not a formatting problem with a recoverable key inside —
> the keys themselves were not usable keys.
>
> Its suite was green and could not have been otherwise. Every test of the
> derivation compared one of its outputs against another of its outputs. The
> three that looked like coverage of exactly this were
> `test_ed25519_keygen_from_zero_seed`, `..._from_ones_seed` and
> `test_ed25519_deterministic` — "non-zero", "non-zero", "the same seed gives
> the same answer" — all three of which a function returning a fixed constant
> passes. Below them, the field and curve tests checked `x + 0 == x`,
> `x - x == 0`, `-(-x) == x`, `0*G == identity`: internal identities that a
> self-consistent implementation of the *wrong curve* satisfies by construction.
> They were deleted with the code they tested, and nothing was lost.
>
> Fixed in `e74f049d9` by the same move as everything else here: the private
> Ed25519 is gone and `posix::ed25519` is called instead — the copy `sshd`
> already verifies host key signatures with, checked against all four RFC 8032
> §7.1 vectors for derivation, signing *and* verification. The private SHA-512
> went with it, having had no other caller. 543 lines removed.
> `the_public_key_derived_here_is_the_one_rfc_8032_specifies` replaces the three
> self-referential tests and deliberately keeps the vector on `ssh-keygen`'s
> side of the call, so a future reintroduction of a private derivation is
> noticed in the crate that did it rather than two crates away.
>
> **This is the sentence the whole entry has been building toward.** Eleven
> duplications of this stack were found by a human reading two files side by
> side, and none by a test. The eighth live divergence was invisible to that
> method — the arithmetic *looks* right, and checking it by eye means
> reimplementing Ed25519 in your head — and the first cross-crate test that
> could see it found it in under a second, before anyone went looking. The
> argument for item 4 was always that it catches the class. It also catches
> things reading never will.
>
> **The general lesson, stated plainly because it generalises past SSH:** a test
> that compares an implementation against itself certifies whatever the
> implementation does. Round-trip tests, "same input gives the same output"
> tests, and internal algebraic identities all have this property. At least one
> assertion per algorithm must compare against a number that came from outside
> this tree — a published test vector, or a second independent implementation
> made to agree. Everything else is a consistency check, and consistency with a
> mistake is a mistake.
