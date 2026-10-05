## 539. Cryptographic primitives are ported from vetted implementations; the format and plumbing around them stay ours

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Operator (Claude recommended this option) — `open-questions.md` → C-Q5, answered `c`

**In short:** the code that protects saved passwords, login and the lock screen is cryptography this project wrote itself — including eleven separate hand-written copies of the same hash function. From now on, the *primitives* (the hash, the cipher, the password hash) are ported from implementations that other people have already spent twenty years attacking and repairing. Everything built on top of them — the vault file format, the credential service, the login flow — stays ours. The reason for the split is that hand-written crypto fails in a way this project's usual defence does not catch: the code computes the right answer and still leaks the secret, through how long it took to compute it. No test we can write notices that.

### The problem this answers

Nothing was ever decided about crypto. The OS has no third-party crypto dependency, so each feature that needed a hash wrote one, it worked, and it kept happening. Turning the workspace lints on for `gui/credentials` surfaced four defects, all logged in `known-issues.md`:

| Problem | What it means |
|---|---|
| The password vault scrambles every secret with an identical repeating pattern | two saved passwords cancel each other out; the vault can be read without the master password |
| The master password is hashed once, with an extra ingredient identical on every SlateOS machine | guessable at billions of tries per second, and one precomputed table cracks every user everywhere |
| Nothing in userspace can obtain an unpredictable number | the built-in password *generator* produces guessable passwords |
| Eleven hand-written copies of SHA-256 | eleven chances for one of them to be wrong, forever |

Those four are being fixed regardless of this decision, as far as hand-written primitives allow. What the decision settles is the **end state** — specifically the two things that were not going to be written by hand: **authenticated encryption**, so an attacker who cannot read the vault also cannot silently alter it, and **a deliberately-slow password hash** (Argon2id or scrypt), so guessing costs an attacker real money.

### Why the line falls between primitive and glue

The argument for borrowing is strongest exactly where the failure is invisible. A cipher or a password hash has one property that cannot be established by checking its output: that it takes the same amount of time no matter what the secret is. The classic break is a comparison that returns a fraction sooner when the first byte is wrong; an attacker measures the timing and recovers the secret one byte at a time, against code that is perfectly correct by every test. This project's entire quality model is "if you did not test it, it is untested" — and here that model does not reach.

The argument for *not* borrowing is strongest where the failure is visible. A vault file format that loses a record, a service that hands a credential to the wrong caller, a login flow that accepts an empty password — these are ordinary bugs, catchable by ordinary tests, and they are also the parts that have to fit this OS's capability model, its IPC, and its no-allocator constraints. Vendoring someone else's opinion about those would import a design, not a proof.

So: borrow where testing cannot reach, write where it can. That is also what production operating systems do, and it is the same rule `design.txt` already states for the filesystem — port battle-tested code rather than writing a new one. That rule was written about ext4 and applies here with more force, because an ext4 bug crashes and a crypto bug just quietly stops protecting anything.

### Rejected

- **A — keep writing our own, carefully.** Implement AES-GCM and Argon2id in-tree against the official test vectors and collapse the eleven hash copies into one shared crate. For: no outside code in the trust base, works in the kernel's no-allocator environment by construction, consistent with how the rest of the OS is built. Against: "passes its tests" and "is secure" are different sentences here, and only here. This was the honest reason the question was asked at all rather than decided in-lane.
- **B — port a vetted implementation for everything,** primitives and protocol both. For: the borrowed code is written to be constant-time on purpose by people who do only this. Against: it imports a whole design where only the arithmetic was wanted, and the surrounding layers are precisely the ones that must fit this OS's capability model rather than a general-purpose one. C gets the same protection over a smaller vendored surface.

### What this obliges

- The eleven SHA-256 copies collapse to one, and that one is ported rather than written.
- New crypto written before the port lands is written knowing it is temporary; prefer deferring a crypto feature over adding a twelfth copy of something.
- The vendored code is vendored, not depended on over the network, and carries a note recording which upstream revision it came from — otherwise "keep it current" has nothing to compare against.
