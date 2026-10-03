## 774. The SSH client and daemon take their randomness as a parameter, so a handshake can be reproduced without weakening one

**Date:** 2026-09-05
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** The SSH client and the SSH server each need unpredictable bytes in
three places — the padding on every packet, the sixteen-byte "cookie" in the
first handshake message, and the secret exponent that makes the session key
secret. Both used to call the kernel's random-number generator directly, which
means neither can be run at all on a machine where that generator is
unavailable — and the machine our tests run on is exactly such a machine, on
purpose. They now take the source as a parameter, defaulting to the kernel and
changeable only from inside each crate's own tests. That makes the one test this
stack most needs — run the real client against the real server and see whether
they agree — possible to write, and makes it *stronger* than it could otherwise
have been, because a handshake driven by a known source produces the same bytes
every time.

**Where:** `sshwire::SecretSource` and `sshwire::KERNEL_SECRETS`;
`SshSession::secrets` / `with_secret_source` in `userspace/ssh/src/main.rs`;
`ConnectionState::secrets` / `with_secret_source` in `userspace/sshd/src/main.rs`.

### The problem this solves, and the one it does not

`randrange::fill_from_kernel` refuses on any non-unix host by design, so that
"fails closed when there is no entropy" is a property something can assert. The
consequence for this stack is total: no exponent, so no Diffie-Hellman, so no
handshake, so no interop test — and four permanently-red tests in
`userspace/ssh` besides.

There are two ways out and they are not alternatives; they answer different
questions.

- Giving the *host* entropy (parked on `lane-b-randrange-entropy`, queued for
  the operator in `open-questions.md`) would let this stack use the real source
  in tests. It is not this lane's call to make, because about eighteen tests in
  `apps/` and `gui/` depend on the host having none.
- Making the *source* a parameter, which is this entry, is lane-local and is
  the better answer for a different reason: it removes the dependency on the
  platform altogether rather than changing which platform is depended on.

Even with a real host CSPRNG, the second is what an interop test wants. This is
the same mistake `randrange` made one layer down — using a property of the build
platform as a test fixture — and fixing it in `randrange` while repeating it
here would be fixing the instance and keeping the pattern.

### Why it is not a way to weaken the client

The obvious objection: a security-critical program that lets its entropy source
be replaced has grown a downgrade attack. The seam is built so that it cannot
be reached from anywhere an attacker or an unwary operator could stand:

- `with_secret_source` is **private and `#[cfg(test)]`** in each binary crate.
  Not `pub`, not reachable from the command line, the ssh config file,
  `sshd_config`, an environment variable, or the network — and not *present* in
  the shipped binary at all, because it is not compiled into it. That last part
  is the one that matters: it turns "the only callers are tests" from a
  statement about today's code into one the compiler enforces about every
  future edit. (When the interop test moves into a crate of its own it will
  need to reach this, and the gate should widen to a Cargo feature that only a
  `dev-dependency` enables. A feature is still absent from a release build; a
  bare `pub` would not be.)
- The *only* constructors, `SshSession::new` and `ConnectionState::new`, set
  `sshwire::KERNEL_SECRETS` unconditionally. There is no `Default` and no
  builder that leaves it unset, so "forgot to set the source" is not a
  reachable state.
- Both crates assert the default by **pointer identity**
  (`core::ptr::fn_addr_eq(s.secrets, sshwire::KERNEL_SECRETS)`), not by
  behaviour. The risk worth guarding is a binary that ships with a test source
  wired in, and no test of "the bytes look random" would ever catch that —
  every such test passes against a counter.

A `fn` pointer rather than a `dyn` trait object, because a source with no state
cannot accidentally acquire any, and both structs can hold one in a `Copy` field
with no lifetime and no allocation.

### Why the type lives in `sshwire` rather than twice

`sshwire` exists because every function in it is a contract between two
programs. A secret source is not a wire format, so the fit is not exact — but
the interop test has to hand *one* source to both ends, and two independently
declared `fn(&mut [u8]) -> Result<(), EntropyError>` aliases would be a third
copy of something this stack has already been bitten by copying nine times.
`sshwire` gains a `randrange` dependency for the error type, which is the
alternative to inventing a second spelling of "the CSPRNG said no".

### What it buys the interop test, beyond making it possible

A deterministic source turns *"both ends derived the same session identifier"*
into *"both ends derived exactly this session identifier."* The weaker
assertion only fails when the two ends drift **apart**; the stronger one fails
when **either** end drifts at all. Given that the entire tracked problem here is
two copies of one protocol that agree with nobody, an assertion that pins the
value is worth more than one that pins the agreement.

### The case against

**"Test-only code in the shipping path."** The field is read on every packet
send, so it is not `cfg(test)`-gated. The cost is one indirect call per packet
against the SHA-256 and AES the same packet already pays for, which does not
register; and gating it would mean the tested configuration is not the shipped
one, which is the more expensive kind of saving.

**"It makes the two most security-critical values in the protocol
substitutable."** True, and the mitigations above are the answer — privacy, an
unconditional default, and a test that asserts the default by identity. Against
it: the values were *already* substitutable in the sense that mattered, because
one of them, sshd's KEXINIT cookie, was a compile-time constant and no test
noticed. A seam a test can reach is what caught that.
