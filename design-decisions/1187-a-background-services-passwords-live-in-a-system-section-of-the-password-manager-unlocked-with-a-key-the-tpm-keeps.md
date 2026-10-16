## 1187. A background service's passwords live in a system section of the password manager, unlocked at startup with a key the TPM keeps

**Date:** 2026-10-11 (answered 2026-10-09)
**Decided by:** Operator (Claude recommended A now and B once a TPM-backed store exists; the operator chose B)
**Lane:** D

**In short:** some programs that run before anyone signs in need a password
to do their job -- dynamic DNS needs the DNS provider's, a scheduled backup
to another computer needs that computer's, Wi-Fi at startup needs the
network's. They keep them in a *system* section of the password manager,
which is unlocked at startup with a key the computer's TPM (a security chip
that hands its key only to this machine's own unmodified startup) keeps, and
which a service reads only while holding a capability for it. The password
is never stored in the clear, even without whole-disk encryption.

**The operator's answer** (`operator-answers/2026-10-09-open-questions-answers.txt`,
D-Q4): "B".

**The options as asked** (D-Q4):

| Option | *What changes* |
|---|---|
| A. A file only the service can read, as Linux keeps Wi-Fi passwords | works at startup and is simple; stored unscrambled, so reading the disk directly shows it unless the whole disk is encrypted |
| **B. A system section of the password manager, unlocked at startup with a TPM-held key, read by capability** (chosen) | encrypted at rest without whole-disk encryption, one place to see and revoke every service password; needs TPM support, which does not exist yet -- without a TPM the unlocking key must sit on disk |
| C. No stored passwords: services run only while their owner is signed in | nothing new to protect; dynamic DNS, remote backups and Wi-Fi at startup stop whenever nobody is signed in |

**What follows.**

- Each service reads its passwords through one function, so the store
  behind it is one place to change (`services/dyndns` is the first).
- What it needs from other lanes: TPM support (lane A: a TPM 2.0 driver
  and a way for a program to seal and unseal a key to this machine's
  measured startup), and the system section itself in the password manager
  (its owner's), with a capability a service is granted for its own entries.
  Lane D files those requests rather than storing passwords any other way
  meanwhile.
- Until both exist, a background service that needs a password cannot have
  one, and says so -- dynamic DNS's sign-in to the provider stays blocked,
  as D-Q4 said it would be until answered. On a machine with no TPM, the
  unlocking key sits on disk, as option B's own description says; that is
  the documented weaker case, not a fallback to A.
