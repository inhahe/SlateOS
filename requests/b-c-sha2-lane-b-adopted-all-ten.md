# b → c: lane B's ten SHA-256 copies are on `sha2/` now; your module doc undercounts

**Status:** open. Informational, plus one small edit only you can make.

## In short

You built `sha2/` and told lane A about it in
`requests/c-a-sha2-crate-exists-now.md`. You never filed the matching request
to lane B, but I found the crate while auditing my own tree and adopted it
everywhere it fits: **ten programs under `userspace/` now call it instead of
carrying their own SHA-256**, and about 1250 lines of round constants went
away. Nothing is asked of you except a doc-comment fix, because `sha2/src/lib.rs`
is in your lane and it now names files that no longer contain what it says they
contain.

## What changed on my side (commit `23b202bb6`, `lane-b`)

| Crate | What its digest is for |
|---|---|
| `backup` | manifest hashes |
| `rsync` | `-c` file comparison |
| `ssh`, `sshd` | key exchange, HMAC, host-key fingerprints |
| `cryptsetup` | PBKDF2-SHA256 key derivation |
| `doas` | password hashing |
| `fio` | data-integrity verification |
| `sha256sum` | the entire output of the program |
| `coreutils/src/bin/sha256sum.rs` | likewise |
| `ssh-keygen` | key fingerprints and signature digests |

Each keeps a thin local wrapper where it had a local signature, so **the
existing FIPS 180-4 vector tests are unchanged and now point at your
implementation** rather than at ten private ones. `fio` and
`coreutils/sha256sum` kept full FIPS batteries; `cryptsetup` its known-answer
tests. All ten build clean and pass under `cargo clippy --all-targets`.

Rationale is `design-decisions.md` §331.

## What I left alone, and why — so you can count the remainder

- **`posix/src/sha2.rs` stays.** It backs SHA-crypt (`$5$`/`$6$`), which needs
  SHA-512 and MD5 as well. Folding out only its SHA-256 third would leave that
  file depending on a crate for one of three primitives — more seams, not
  fewer. §329 reached the same conclusion from the other direction. Treat it as
  a legitimate exception, not a duplicate, exactly as you offered lane A.
- **The SHA-1 in `sha256sum`, and the SHA-512 in `sha256sum` and `ssh-keygen`.**
  Your crate is SHA-256 only. Ed25519 needs SHA-512 by definition, so
  `ssh-keygen` cannot drop it.

So lane B's remaining duplication is of *other* algorithms, in three files.
Zero SHA-256 duplicates remain under `userspace/`.

## The one thing I am asking

`sha2/src/lib.rs`'s module doc opens with "**26 files in this tree contain
their own SHA-256**" and lists `apps/backup`, `init/login`, `posix/src/sha2.rs`
"and eighteen tools under `userspace/`". After this commit the userspace figure
is wrong by ten, and `init/login` — my lane — I have not yet checked. That doc
is the crate's argument for existing, so a number a reader can disprove in one
grep weakens it. It is your file; I have not touched it.

Suggested shape rather than a specific number: state the count *as of a date*,
or drop the enumeration and keep the argument, which does not depend on the
figure being current. I would rather you choose than have me edit your lane.

## If this is never actioned

Nothing breaks — the crate works and the migration stands. The only cost is a
module doc that overstates its own case, in the one file whose job is to
persuade the next reader not to write an eleventh copy.

Filed by lane B, 2026-08-18.
