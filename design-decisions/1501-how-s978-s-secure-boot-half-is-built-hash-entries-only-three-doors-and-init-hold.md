## 1501. How §978's Secure Boot half is built: hash entries only, three doors, and init holds the right to change the lists

**Date:** 2026-10-01 · **Decided by:** Claude (operator-approved scope: §978 decided that `secureboot` is fixed and then given a syscall door; what the fix checks, the doors' shape and who holds their right are Claude's calls, and Claude's to revisit) · **Lane:** A

**In short:** the kernel's Secure Boot lists now really check an image's
fingerprint (its SHA-256 hash) against a list of allowed hashes and a list of
forbidden ones. Programs can reach them through three new system calls.
- **Asking** whether an image may run is open to every program.
- **Adding or removing an entry** needs a new permission. For now every program
  the system starts holds it, because nothing yet exists that could hand it to
  just one administrative tool.

Nothing in the kernel acts on the answer yet. A changed list changes what the
`sbctl` tool reports, not what the machine runs.

**1. Hash entries only.** `verify_image(name, hash)` looks the hash up in `dbx`
first, and a forbidden entry wins. It then tries `db` and `MOK`. `PK` and `KEK`
entries never match an image, because they name the keys that sign the lists,
not images.
- When the state enforces, an image in no list is refused.
- When it does not, the image may run, and the answer says it is unlisted
  rather than pretending it was checked.
- A hash that cannot be read is refused, not treated as unlisted.

Rejected options and boundaries:
- *Rejected: certificate entries.* Matching a signature needs an X.509
  (certificate format) parser in the kernel, run on caller-supplied bytes. §978
  and lane B's request both rule that out.
- *The kernel never hashes the image; the caller supplies the digest.* The
  answer therefore means "this hash is listed as ...", not "this file is ...".
  A caller that lies about the hash fools only itself, as long as nothing in
  the kernel acts on the answer. An exec-time check would have to hash the
  image itself, and is not built.

**2. Three doors** (`kernel/src/syscall/number.rs`):

| number | door | right |
|---|---|---|
| 1082 | enrol: type, subject, fingerprint, returning an entry id | `ENROLL_SECUREBOOT` |
| 1083 | remove an entry by id | `ENROLL_SECUREBOOT` |
| 1084 | verify: name, hash; writes a 16-byte answer (listing, entry, enforced, may run) | none |

How the arguments are handled:
- The image name is taken as **bytes**, because it may be a path. CLAUDE.md
  rule 7 forbids forcing a path into UTF-8. The kernel shell, which lists the
  verification records, shows any byte that is not UTF-8 as `\xNN` and a
  backslash as `\\`, so a displayed name reads back unambiguously.
- The subject and the hash are identifiers, not paths. They must be UTF-8, and
  anything else is refused rather than replaced.
- The right is checked before any argument is read. An unprivileged caller
  therefore learns nothing from which error it gets.
- The output is checked before anything is recorded. A call refused for its
  output leaves no verification record.
- *Reading the lists is not a syscall.* `/proc/secureboot` serves the entries,
  and now the state and whether it enforces. Writes go through syscalls and
  reads through `/proc`, as the kernel does elsewhere (lane B's request, door
  3).
- *No door sets the state.* Only the kernel shell can change it. Nothing on the
  system needs to, and it is the most dangerous change of all, because it turns
  enforcement off. Such a door gets added, behind a right of its own, when
  something needs it.

**3. Who holds `ENROLL_SECUREBOOT`: init, and everything it starts that
nothing has narrowed.** This is how `SET_HOSTNAME`, `SET_KEYLAYOUT` and
`SET_CREDENTIALS` are granted (§930). The pin in `cap/rights.rs` records it
class by class.
- *Rejected for now: withhold it until `authbroker` can grant it to one tool.*
  The door would be dead until then. §978's answer ("make them live") rules
  that out, and `authbroker` is the next module on the same list.
- *Rejected: also require uid 0.* The hole that check would patch is the same
  for every setting right: a process that gives up root keeps the rights it
  inherited (`requests/d-a-a-process-that-gives-up-root-keeps-roots-authority.md`).
  It is fixed once there, not right by right.
- *What it costs meanwhile:* any process nothing has narrowed can change the
  lists. That is bounded by nothing acting on an answer yet.

**Revisit** when `authbroker` can grant a right to one tool, or when anything
starts acting on a verdict, whichever comes first.
