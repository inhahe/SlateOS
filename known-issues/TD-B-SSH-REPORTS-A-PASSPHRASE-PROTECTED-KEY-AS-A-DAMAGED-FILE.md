## TD-B-SSH-REPORTS-A-PASSPHRASE-PROTECTED-KEY-AS-A-DAMAGED-FILE (lane B)

**In short:** A private key file can be *encrypted with a passphrase*, so that
stealing the file is not enough to steal the key — the thief also needs the
words. Neither `ssh` nor `sshd` nor `ssh-keygen` in this tree can read one, and
none of them says so. `ssh -i ~/.ssh/id_ed25519 host` on a passphrase-protected
key — which is what a user who copied a key over from a real machine will have —
reports the file as unusable, which reads as "your key is corrupt". The user's
key is fine; this client cannot open it.

**Where it lives:** `sshwire::decode_openssh_private_key`
(`userspace/sshwire/src/lib.rs`) reads the `openssh-key-v1` container's cipher
name and refuses anything that is not `none`. `userspace/ssh/src/lib.rs`
`load_identity_from` turns that refusal into an error naming the file, per
design-decisions.md §778; `sshd::HostKey::from_openssh_text` and `ssh-keygen`
do the equivalent.

**Reproduce:** take any key written by real `ssh-keygen -t ed25519` with a
non-empty passphrase (its container names `aes256-ctr` and `bcrypt`), and hand
it to `ssh -i`. The message describes the container, not the passphrase.

**Two defects, and only the second is large:**

1. *The message is wrong.* An encrypted container is a recognisable thing — its
   cipher-name field says `aes256-ctr` where an unencrypted one says `none` —
   so the decoder can distinguish "this key is locked" from "this file is
   damaged" without implementing anything. Saying so is a small change and
   should happen regardless of when (2) does.
2. *The feature is missing.* Reading one needs bcrypt-pbkdf (a deliberately slow
   key-derivation function) and AES-256-CTR over the private section. The
   AES is already in `sshwire`; bcrypt-pbkdf is not, and is the real work.
   `ssh-keygen -p` (change a passphrase) and writing an encrypted key are the
   same machinery in reverse.

**Why this matters beyond convenience:** the current arrangement quietly
pressures users toward *unencrypted* private keys, because those are the only
ones that work. That is a security regression dressed as a missing feature — the
file on disk is the whole secret.

**The proper fix:** implement (1) now as a distinct `PrivateKeyError::Encrypted`
variant carrying the cipher name, and (2) as `bcrypt-pbkdf` in a shared crate
with `sshwire` decrypting the private section, plus a passphrase prompt in `ssh`
and `ssh-keygen` and a `-P` flag for scripts. Both halves belong in `sshwire`,
not in the three binaries, for the reason the whole crate exists: a
key file one program can open and another cannot is precisely the class of bug
this stack has produced fourteen times.

**Until then:** design-decisions.md §778's rule — an explicit `-i` that cannot
be used stops the client — is right for a damaged file and wrong for a locked
one, which should prompt. Revisit that rule when (2) lands.
