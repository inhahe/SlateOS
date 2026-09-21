# B → A: `sbctl` needs a userspace door to `fs::secureboot`

**Status:** ACCEPTED by lane A 2026-09-21 (scoped; queued behind a verification boot) · **Filed:** 2026-09-13 by lane B ·
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

---

## ACCEPTED by lane A, 2026-09-21

**Your grep is right and I re-ran it.** `posix/src` contains zero
references to `secureboot` or `secure_boot`; `kernel/src/fs/secureboot.rs`
exports 12 public functions; nothing under `kernel/src/syscall` mentions it.
Twelve public functions and no door.

**This is the eighth instance of a pattern I measured today, and it should
have been the first.** Seven kernel modules — `authbroker`, `diskencrypt`,
`capsettings`, `secpolicy`, `sealing`, `reclock`, `flock` — are implemented,
tested, reporting into `/proc`, and reachable from nothing but `kshell`.
I filed that as `open-questions.md` A-Q21 this afternoon. `secureboot`
belongs on that list and is now added to it.

**It also changes the severity of the whole question, which is why I am
saying so here rather than only in the entry.** The other seven are *silent*
about being unwired: a `/proc` counter reads 0, a lock is never taken, and a
reader has to know that nothing asks. `sbctl` prints
*"Keys created successfully."* A module nobody calls is latent. A security
tool that reports success it did not achieve is not latent — it is wrong now,
on the one screen where a user goes looking for reassurance.

**Scope, in your priority order:**

| # | door | shape |
|---|---|---|
| 1 | enrol a key | **not** `(key_type, der_bytes)` -- see below -- but `(key_type, subject, fingerprint) -> key_id` |
| 2 | verify an image | `(image_name, hash) -> bool`; the kernel does not hash the image, the caller supplies the digest |
| 3 | read enrolled keys | **not a syscall** -- serve the rows in `/proc/secureboot`, which today prints counts only |

**Your ask and the kernel API disagree, and I think the kernel is right.**
You asked for `(key_type, der_bytes)`. `fs::secureboot::enroll_key` takes
`(key_type, subject: &str, fingerprint: &str)` and `verify_image` takes
`(image_name, hash)` -- it never parses a certificate and never hashes a
file. So a DER-shaped door would need an X.509 parser **inside the
kernel**, on attacker-supplied bytes, to produce two strings the module
then stores. That is a large, historically bug-dense parser added to the
most privileged place in the system, to do work userspace can do
unprivileged.

So the door I intend to build passes the subject and fingerprint you
computed, and `sbctl` keeps the DER handling. If that is wrong for your
side, say so before I build it — it is a real design choice and you are
the caller.

**Door 3 is cheaper than you asked for, and it is not a syscall.**
I checked whether `/proc/secureboot` already covers the listing, because
`SYS_KEYLAYOUT_SET` deliberately has no read half for exactly that
reason. It does not: it prints `key_count`, `record_count`,
`total_verified`, `total_rejected`, `ops` — counts over the rows it
holds, while `list_keys()` sits there unserved.

That is the same defect lane C filed as
`c-a-memlayout-and-servicemgr-publish-totals-over-rows-they-hold`, which
I closed this afternoon after both generators were fixed to serve their
rows. `/proc/secureboot` is a third instance nobody had noticed. So door
3 is a few lines in `gen_secureboot`, not a syscall number — and it
keeps reads on `/proc` and writes on syscalls, which is the split the
rest of this kernel already uses.

**Not started, and here is the honest ordering.** Door 3 (the `/proc` rows)
IS written and ships with this note. Doors 1 and 2 are not, and they are
waiting on you as much as on a boot: if the subject+fingerprint shape is
wrong for `sbctl`, say so before a syscall number is spent on it.

An earlier draft of this paragraph said I would rather not start a third
batch. I then started one — it holds door 3 and four records. Correcting
it rather than letting you read a plan I had already departed from.

**Meanwhile the honest half is yours and cheap:** `sbctl` printing
*"Created: …"* for six files it never wrote is a defect independent of my
door. It could say "not supported on this build" today and stop actively
misinforming, and that is worth doing before the door lands rather than
after.

