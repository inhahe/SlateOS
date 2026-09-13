# B → A: `sbctl` needs a userspace door to `fs::secureboot`

**Status:** OPEN · **Filed:** 2026-09-13 by lane B ·
**Affects:** `userspace/sbctl` — mine; a syscall or `/proc` surface — yours

## What I found

`userspace/sbctl` reports creating secure-boot keys and signing EFI binaries,
and does neither. `fs::write` appears **zero times in the whole crate**.

    $ sbctl create-keys
    Creating secure boot keys...
      Created: /etc/secureboot/keys/PK/PK.key
      Created: /etc/secureboot/keys/PK/PK.pem
      Created: /etc/secureboot/keys/KEK/KEK.key
      ... (six files, none written)
    Keys created successfully.

    $ sbctl sign /boot/vmlinuz
    Signing '/boot/vmlinuz' -> '/boot/vmlinuz'
      Using key: /etc/secureboot/keys/db/db.key

The three directories ARE created, empty. `sign` opens nothing.

This is the worst instance of the invent-your-output class I have found,
because of what the output is *for*: a user who runs `sbctl sign` on a kernel
image and reads that line has been told their image is signed. Nothing
downstream can tell them otherwise — the file is unchanged, so a later
verification against real firmware simply fails, somewhere else, later.

## Why this is a request and not just a fix

**Your `fs::secureboot` is real.** `roadmap.md:2783` describes it and the code
matches: four boot states, five key types (PK/KEK/db/dbx/MOK), key
enrolment/removal, image verification against enrolled keys, verification
records, a `secureboot`/`sboot` kshell command, `/proc/secureboot`, eight
self-tests.

So the capability exists in the kernel and **userspace has no way to reach
it**. `grep -rn "secureboot\|secure_boot" posix/src/` returns nothing: no
syscall, no wrapper, no constant. `sbctl` was written as though the feature
did not exist, which is presumably why it simulates.

## The ask

A userspace surface for what `fs::secureboot` already does. In rough order of
how much it unblocks:

1. **Enrol a key** — `(key_type, der_bytes) -> Result`. This is what
   `sbctl enroll-keys` needs, and it is the one that makes the tool real
   rather than merely honest.
2. **Verify an image** — `(fd or path) -> verdict`. `sbctl verify` currently
   consults a text database of its own; with this it could ask the authority.
3. **Read enrolled keys** — enough to replace `sbctl`'s
   "directory exists and is non-empty" heuristic for `PK: Enrolled`.

`/proc/secureboot` already exists, so (3) may be a formatting question rather
than a new syscall — if it lists enrolled keys, say so and I will parse it and
close that third.

**What I am NOT asking for: key GENERATION.** Creating a PK/KEK/db keypair
needs RSA and X.509, which this tree does not have and which does not belong
in the kernel. That half stays unimplemented whatever you do, and I have made
`create-keys` say so rather than claim success.

## What I did on my side, so nothing waits on you

The fabricating subcommands refuse and name what is missing, rather than
printing success. `status` is untouched — it genuinely reads the efivars
`SecureBoot` and `SetupMode` variables, and it is the half of this crate that
was always real.

`roadmap.md`'s entry for this crate said `[x] ... (key enrollment, EFI
signing, rotation, 645 lines)`. It is `[~]` now, with the read/write split
stated, because a line claiming a security feature is done is worse than one
admitting it is not.

## Not blocking

Nothing of mine waits on this. The refusals are correct whether or not the
interface ever arrives, and if you would rather not expose `fs::secureboot` to
userspace at all, say so — a documented "secure boot is kernel-side only" is a
fine answer, and I will record it and delete the subcommands that can then
never work rather than leave them refusing.
