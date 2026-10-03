## TD-B-NO-SSH-TOOL-CAN-READ-A-PASSPHRASE-PROTECTED-PRIVATE-KEY (lane B)

**In short:** A private key file can be *encrypted with a passphrase*, so that
stealing the file is not enough to steal the key — the thief also needs the
words. Real `ssh-keygen` offers to do this every time it makes a key, so a user
arriving from any other machine is likely to have one. No tool in this tree can
read one. The diagnosis is already good — `key is encrypted with aes256-ctr;
there is no passphrase prompt here, re-create it with an empty passphrase` — so
this is a missing feature, not a misleading error. It is filed anyway because
of what the missing feature *pushes users to do*.

**Where it lives:** `sshwire::decode_openssh_private_key`
(`userspace/sshwire/src/lib.rs`) reads the `openssh-key-v1` container's
`ciphername` field and returns `PrivateKeyError::Encrypted { cipher }` for
anything but `none`. All three callers surface it verbatim:
`userspace/ssh/src/lib.rs` `load_identity_from`,
`sshd::HostKey::from_openssh_text`, and `ssh-keygen`.

**Reproduce:** take any key written by real `ssh-keygen -t ed25519` with a
non-empty passphrase (its container names `aes256-ctr` and `bcrypt`) and hand it
to `ssh -i`.

**Why this is more than an inconvenience:** the advice the message gives —
"re-create it with an empty passphrase" — is the only thing a user can do, and
it is the *less safe* configuration. So the gap does not merely block a feature;
it steadily converts protected keys into unprotected ones, on the disks of the
users who cared enough to set a passphrase in the first place. The file on disk
is the whole secret.

**What it would take:** `bcrypt-pbkdf` (a deliberately slow key-derivation
function, Blowfish-based) to turn the passphrase into a key, then AES-256-CTR
over the private section. The AES is already in `sshwire`; `bcrypt-pbkdf` is the
real work and exists nowhere in this tree. Writing an encrypted key, and
`ssh-keygen -p` to change a passphrase, are the same machinery in reverse.

**The proper fix:** `bcrypt-pbkdf` and the private-section decryption go in
`sshwire`, not in the three binaries — for the reason that crate exists: a key
file one program can open and another cannot is exactly the class of defect this
stack has produced fourteen times. On top of that, a passphrase prompt in `ssh`
and `ssh-keygen`, and a way to supply one non-interactively for scripts.

`sshd` is the exception and must stay one: it is started by init with no
terminal to prompt on, so an encrypted *host* key has to remain an error there
however this is implemented. That asymmetry is the reason the decoder reports
the fact and each caller decides what it means, rather than prompting itself.

**Interaction with design-decisions.md §778:** that rule makes an explicit `-i`
naming an unusable key a hard error rather than a silent fall back to a password
prompt. For an encrypted key that is currently right — there is no passphrase
prompt to offer — but it stops being right the moment there is one. Revisit §778
together with this entry.
