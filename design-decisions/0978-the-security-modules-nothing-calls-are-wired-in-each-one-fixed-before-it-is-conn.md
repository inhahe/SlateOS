## 978. The security modules nothing calls are wired in, each one fixed before it is connected

**Date:** 2026-09-27 · **Decided by:** Operator (Claude recommended documenting them as staged first; the operator chose to make them live) · **Lane:** A

Answering A-Q21. Relayed by lane F, 2026-09-27; the answer was one word, "B".

**In short:** several pieces of kernel code whose job is to say "no" are
written and tested. They check passwords, unlock encrypted disks, decide who
may reach a file, refuse writes to sealed files and check boot images. Nothing
uses them except commands typed by hand into the kernel's own shell. The
operator chose to make them live. Each one is fixed first and then connected,
because several have faults that are harmless only while nothing calls them.

**What it obliges, module by module.** Each connection is its own change with
its own test. Each module's `/proc` counters then start to move, which is how
a boot shows that a module is live.
- **`secureboot`.** `verify_image` ignores the image's fingerprint and passes
  everything.
  - Fix first: a real check against allow and deny lists of image
    fingerprints. These are UEFI's `db` and `dbx`, and need no certificate
    code in the kernel.
  - Then the syscall door that lane B's parked request asks for
    (`requests/b-a-sbctl-needs-a-userspace-door-to-fs-secureboot.md`).
  - `userspace/sbctl` no longer claims success, as A-Q21's text had it. Its
    commands have refused since 2026-09-15, per lane B's correction of
    2026-09-27. Under the operator's B-Q17 answer, lane B is deleting the
    four that need RSA/X.509. `enroll-keys` and `reset` stay as refusals
    until this door lands.
- **`diskencrypt`.** `unlock_volume` ignores the passphrase. Fix first: real key
  derivation with a ported, vetted password hash (§539). Then connect it to
  the mount path.
- **`sealing`, `capsettings`, `secpolicy`.** Several key their tables by path
  name, so two names for one file get two answers. Fix first: re-key them by
  file identity. Then connect them to the VFS permission and write paths.
- **`authbroker`.** Connected to the login path, once lane B's login has a door
  to call.
- **The per-file metadata tables:** `acl`, `fcomment`, `queryable` and `tags`.
  The question named them as the same shape. They get syscall-layer doors as
  part of the same work.
