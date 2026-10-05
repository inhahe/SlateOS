## 819. The vault cipher is ChaCha20-Poly1305, and it will not become the disk cipher because that one already exists

**Date:** 2026-09-07
**Lane:** C
**Decided by:** Operator (the criterion and its exception were the operator's; lane B established that the exception applies, and Claude confirmed the one condition that would have overridden it does not hold)

**In short:** SlateOS had no way to encrypt a saved password. The choice was
between two standard ciphers: AES-256-GCM, which is far faster on any modern
processor because the hardware implements it directly, and ChaCha20-Poly1305,
which is uniformly fast everywhere without hardware help. The operator's rule
was "pick whichever is fastest with the hardware acceleration, unless the real
bottleneck is the disk anyway". For what is actually being encrypted here, the
speed of neither is measurable -- so the rule's own exception applies, and the
choice falls to the simpler one to get right: ChaCha20-Poly1305.

**Glossary, because the original question did not gloss these and the operator
said so.** *AES-NI* -- instructions built into the processor that do AES
directly, making it several times faster than software. *In-tree* -- the code
lives in this repository and is compiled with it, as opposed to being fetched
from an external package registry. *Audit* -- someone reading the code
deliberately looking for flaws.

**Why crypto needs auditing at all**, which the operator also asked. Because
it fails silently. A bug in a renderer is a wrong pixel; a bug in a cipher
produces output that still decrypts correctly with your own code and still
passes a round-trip test, and is simply readable by someone else. Two classes
are invisible to ordinary testing: a construction error (a reused nonce, a
truncated tag, a counter that wraps), and a **timing side-channel**, where the
code takes measurably longer depending on the secret. The second is invisible
to *every* functional test by definition, because all the outputs are correct.
The only defence is someone reading it with intent.

**Why in-tree C is harder to audit**, the third thing asked. Not because C is
unreadable. Because of who reads it here: this tree is Rust with a strict
unsafe policy, so the reviewer reads Rust constantly and C rarely, and a
subtle change lands with less scrutiny. A port also carries provenance to
track by hand -- which upstream version, which local patches, what changed on
a re-sync. The claim is about the review pipeline, not the language.

**Why the speed argument does not decide it.** With AES-NI plus PCLMULQDQ,
AES-256-GCM is decisively faster -- around a cycle per byte, typically two to
three times a portable ChaCha20. But what is being encrypted is credmanager's
vault and saved Wi-Fi passwords: kilobytes, written when something changes. At
that size the cipher is not measurable next to the syscalls, and both are
dwarfed by the password-to-key derivation, which is *deliberately* slow --
tens to hundreds of milliseconds. The operator's exception was written for
exactly this shape of case.

**The one thing that would have flipped it, and why it does not.** Lane B
noted that if this cipher were to become the basis for full-disk encryption
later, throughput would be the whole point and AES-NI the reason it is
feasible. It will not: disk encryption **already exists** in this tree --
`kernel/src/fs/diskencrypt.rs`, with AES-256-XTS, key slots, TPM sealing and
LUKS tooling on top. XTS is a mode for block devices and is not
interchangeable with an authenticated-message cipher in either direction, so
the vault cipher and the disk cipher are separate choices and always were.
Checking that was cheaper than asking, and it removes the condition.

**What this means in practice.** One implementation rather than a fast path
plus a portable fallback, which is the correctness argument the size of the
data leaves standing on its own.
