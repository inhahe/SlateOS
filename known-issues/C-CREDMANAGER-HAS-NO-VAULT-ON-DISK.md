### C-CREDMANAGER-HAS-NO-VAULT-ON-DISK — 2026-08-26 — LANE C, OPEN

**Status: FIXED 2026-09-27 (lane E, which owns `apps/` since the split).** The
cipher this waited on is vendored -- RustCrypto's XChaCha20-Poly1305 and
Argon2id under `rustcrypto/`, used through `rustcrypto/seal`
(`design-decisions.md` §539, §1218) -- and the vault is one file,
`<config>/credmanager/vault` (`apps/credmanager/src/vaultfile.rs`): a header
naming the Argon2id parameters, the salt and the nonce, bound as associated
data to the sealed contents. The three steps below happened, with one change
of plan: there is no stored verifier at all. The key that opens the file *is*
the check, so nothing checks a guess more cheaply than opening the vault does.
A first run makes the vault from a master password typed twice (at least ten
characters, strength shown); a file that is not a vault is shown as such and
never written over; locking now forgets the key and every entry, where it used
to change a flag; every change is saved as it is made, under a new nonce, and a
save that fails is said in the window and holds a close once.

**What happens.** credmanager now opens a real window, and that window opens an
**empty vault, every launch**. There is no persistence layer at all: `main`
calls `Vault::create("My Vault", "")` and hands it to `app::launch`. Anything
the user stores is gone when the window closes.

This is not a regression — the app had an empty `main` and stored nothing
before either — but it changes character now that it is launchable, because a
program that opens is a program someone may try to use.

**Note the empty master password.** `Vault::create(name, "")` derives a
verifier from an empty string, so the lock screen opens on Enter. That is the
right default for a vault with nothing in it and the wrong one for a vault with
something in it, which is the same statement as "there is no persistence yet".

**The proper fix**, and the order it has to happen in:

1. A vault file format: the `pwkdf` verifier written **with its salt and round
   count** (a verifier without them is unopenable), then the entries encrypted
   under `pwkdf::derive_key` with the same params.
2. First-run: no vault file means prompt to *create* a master password, not to
   enter one. The current lock screen has no create path.
3. `main` reads the file, and the empty-password default disappears with it.

Step 1 needs an encryption primitive **the whole tree does not have**. `pwkdf`
derives keys; it does not encrypt. Nor does anything else: `sha2`, `sha1`, `md5`
and `crc32` are one-way fingerprints, and there is no cipher crate in the
workspace at all. So this is blocked on adding one, filed as
`open-questions.md` → "SlateOS has no way to encrypt anything. Which cipher do
we add, and who owns it?" — because "which cipher" and "which lane writes it"
are not calls to make quietly in an app's `main`.

**Until then**, the crate doc says so, and the app is a working demonstration
of the UI rather than somewhere to keep a password.
