## 776. The SSH interop test lives in a crate of its own, and pins the handshake transcript rather than only checking the two ends agree

**Date:** 2026-09-05
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** the SSH client and the SSH server are two programs that have to
agree, byte for byte, about a long list of things. Nothing ever checked that
they did — each was tested against its own idea of what the other would do, and
both passed while six real disagreements shipped. There is now a test that runs
the two against each other for real. Two choices in it had arguments on both
sides: it lives in a **third crate** that ships nothing rather than inside
either program, and it asserts the handshake produces **one specific recorded
value** rather than merely that the two ends produced the same value as each
other.

### The first choice: a third crate

A test that drives both ends needs both ends in one process, so something has to
depend on both — and a crate cannot depend on itself. `userspace/ssh-interop`
exists for that and nothing else: it has no `[dependencies]` at all, its library
is a page of documentation, and both peers arrive as `dev-dependencies`.

*Against it:* a crate whose entire purpose is a test is an odd artifact. It adds
a workspace member, a `Cargo.toml`, and a place where someone might later put
non-test code because it is a library and libraries hold code. It also forced
`ssh` and `sshd` to be split into `lib.rs` plus a three-line `main.rs` each,
since a bin-only crate produces no rlib — three files changed to enable a test.

*For it:* there is no alternative that reaches both ends. The only other route
is a boot test under QEMU, which proves more (it exercises the real sockets)
but costs minutes per run, cannot be run by `cargo test`, and would have been
blocked on the same entropy problem. The `lib.rs` split is independently good —
`run_cli() -> i32` with the `process::exit` in the binary is the shape that
makes any part of a program callable — and the dev-dependency direction is what
lets `deterministic-secrets` be a feature that a release build does not compile
at all.

### The second choice: pin the transcript

The obvious assertion is `client_id == server_id`: both ends derive the RFC 4253
§7.2 session identifier independently from the same eight inputs, so a
disagreement about any of them shows up as two different values. The test
asserts that. It *also* asserts the value equals a recorded constant.

*Against the constant:* it is an observation, not a value derived from the RFC
by hand — it records what the code did on the day it was written, so if the code
was subtly wrong that day, the constant enshrines the wrongness. Every
deliberate protocol change now has to update it, which is a maintenance cost
paid on every legitimate change. And it makes the test depend on the whole
handshake being byte-reproducible, which is a real constraint on the harness
(see the thread-local counter below).

*For the constant:* agreement alone cannot see a change that both ends make
*together*, and "both ends changed together and stayed wrong" is not a
hypothetical here — it is exactly
`TD-B-SSHD-SIGNS-AN-EXCHANGE-HASH-OVER-A-CLIENT-VERSION-THE-CLIENT-NEVER-SENT`,
where the server hashed a placeholder client version and the server's own tests,
recomputing the hash the same wrong way, agreed with it perfectly. A shared
`sshwire` makes that class *more* likely, not less: the two ends now call one
function, so a change to it moves both at once. The maintenance cost is the
feature — updating the constant is the moment someone has to look at a protocol
change from both sides at once, which is the thing that was never happening.

### What reproducibility cost

The secret source counts up, and its counter is **thread-local**, with both
peers on spawned threads and neither on the harness's. A process-wide counter
was written first and was wrong: the two peer threads draw from it
concurrently, so which bytes each one receives depends on how they interleave —
deterministic in the sense that it uses no entropy, and nondeterministic in the
only sense that matters. A peer left on the harness's own thread would likewise
inherit whatever that thread had already drawn. Verified by three separate
process runs producing the same identifier.

**Revisit if** the test grows past the handshake into authentication and channel
traffic. Once a data round-trip is covered, much of what the recorded constant
protects is covered more directly by "the client read back what the server
wrote", and the constant's maintenance cost may stop earning itself.
