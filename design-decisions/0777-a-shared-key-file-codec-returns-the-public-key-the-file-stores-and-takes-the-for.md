## 777. A shared key-file codec returns the public key the file stores, and takes the format's `checkint` as a parameter

**Date:** 2026-09-05
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** SSH private key files store the private key *and* a copy of the
public key that goes with it. Those two ought to agree, and the whole reason
this came up is that once they did not — `ssh-keygen` wrote files whose two
halves disagreed, because its arithmetic was wrong. So when the reading and
writing of those files moved into one shared library, two questions came up
with genuine arguments on both sides: should that library check the two halves
agree, and should it invent the file's random integrity field itself.

The answer to both is **no** — it hands back what the file says and lets the
caller decide, and it takes the random field as an argument. Both answers cost
something real, which is why this is written down.

### The first choice: the codec returns the stored public key, it does not derive one

`sshwire::decode_openssh_private_key` returns the seed *and* the public key
exactly as the file records them, without checking that the second follows from
the first. Verifying that is the caller's job, and `sshd::HostKey` does it.

**For deriving it inside the codec:** it is the more obviously safe interface. A
caller that forgets the check accepts a file whose two halves disagree, and gets
a key that cannot sign anything anyone will accept — which is precisely the bug
that motivated all of this. Making the check impossible to skip is worth a lot.

**Against, and decisive:** deriving the public key means computing on a curve,
and `sshwire` cannot. The only Ed25519 in this tree is `posix::ed25519`, and
`posix` compiled as an rlib is a second libc whose every syscall is stubbed to
`-ENOSYS` in a program build (see
`TD-B-THE-POSIX-RLIB-IS-A-SECOND-LIBC-WITH-EVERY-SYSCALL-STUBBED-OUT`). Making
`sshwire` — the crate both peers link *for wire encoding* — drag that in would
put a stubbed libc into two programs in order to save one line at one call site.

The alternative to accepting that cost was worse in a subtler way: `sshwire`
could grow *its own* Ed25519. That is exactly the arrangement this entire
sequence of work exists to dismantle, and it would have been the third copy of
the curve in the tree — one of which had already been found deriving the wrong
answer.

*What this costs:* every reader of a private key file must verify the two halves
itself. Today there is one such reader, `sshd::HostKey::from_openssh_text`, and
it does. If a second appears, this obligation is a thing to be forgotten. The
mitigation is that the obligation is stated in the codec's doc comment as an
obligation, not merely implied by its absence.

### The second choice: `checkint` is a parameter, not generated inside

The `openssh-key-v1` format puts the same random 32-bit word twice at the head
of its private section. Decrypting with the wrong passphrase makes the two
copies disagree, which is how the format tells "wrong passphrase" from "corrupt
file". `sshwire::encode_openssh_private_key` takes that word as an argument
rather than drawing it.

**For generating it inside:** it is a random number that the format requires,
and a caller has no reason to care what it is. Every caller passing one is
boilerplate, and a caller that passes a constant weakens the field.

**Against, and decisive — the same reasoning as the packet padding in §773.**
Entropy is a syscall and it can fail. A shared encoder has no business deciding
what a program should do when it cannot get any: refuse to write the file, write
zeros, panic. Each program answers that where it can also report it. `ssh-keygen`
draws four bytes from `randrange` and propagates the failure as a `KeygenError`,
which is the answer for a tool whose entire job is producing a key file.

The second argument is testability, and it turned out to matter more than
expected. The interop test that found the wrong-public-key bug had to produce a
byte-identical key file on every run, on a host where `randrange` refuses every
request on purpose. With `checkint` generated internally that test could not
have been written at all — and it is the test that found the most serious bug in
this stack.

*What this costs:* a caller can pass a constant, and one does. The test passes
`0x1234_5678` deliberately. Nothing prevents production code doing the same, and
the field's value only matters for encrypted keys, which this tree does not yet
write — so the day it does, this is a thing to check.

**Revisit if** `sshwire` ever gains a legitimate reason to depend on a curve
implementation — for instance if signature verification moves there, which would
be a defensible place for it — because at that point deriving the public key
inside the decoder costs nothing and the first choice above should flip.
