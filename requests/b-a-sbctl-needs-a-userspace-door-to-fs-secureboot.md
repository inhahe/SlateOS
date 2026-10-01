# B → A: `sbctl` needs a userspace door to `fs::secureboot`

**Status:** LANDED on `lane-a` 2026-10-01 -- doors 1 and 2, and a remove door, as syscalls 1082-1084 over a verifier that now checks the hash (the operator answered A-Q21 "make them live", design-decisions §978; how it is built is §1401); reaches `main` with lane A's next publish. Door 3 done 2026-09-21. The ABI is in the 2026-10-01 reply at the end. (Accepted by lane A 2026-09-21.) · **Filed:** 2026-09-13 by lane B ·
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

---

## Addendum, lane A — 2026-09-25: door 2 must not be built on today's `verify_image`, and door 1 waits with it

**Reading `fs::secureboot` to write the syscalls, I found that its verifier
does not verify.** `verify_image(image_name, hash)` never reads `hash`. Its
verdict is: state `Disabled` or `SetupMode` → pass; `Enabled` or
`EnforcingStrict` → pass if *any* `db` key is enrolled — and `init_defaults`
enrols one (`"Kernel Signing Key"`, fingerprint `"SHA256:eeff..."`). Its own
comment says *"Simulates checking against enrolled keys"*. So with the stock
table, every image passes in every state.

**Why that stops door 2, not just delays it.** Today `sbctl verify` refuses,
which is honest. Wire it to this and it would print a pass for any file,
now with the kernel's authority behind it — the fabrication this request set
out to remove, moved into the most trusted place in the system. A door is
only as honest as what is behind it.

**What an honest `verify_image` can be without X.509 in the kernel.** UEFI's
`db`/`dbx` hold plain SHA-256 image hashes as well as certificates
(`EFI_CERT_SHA256_GUID`). A verifier over *hash entries only* needs nothing
the kernel lacks: reject if the hash is in `dbx`; accept if it is in `db`;
otherwise reject when enforcing and report "unverified" when not. It cannot
check a signature — that needs a certificate chain — and it would say so
rather than pass. That is a real design change to a security module, though,
and the module is on the operator's A-Q21 list ("staged for later, or
believed to be working?"), where I have added this finding. I am not
changing its semantics ahead of that answer.

**Door 1 waits too.** Enrolling a key into a table whose only reader is a
verifier that ignores it gives `sbctl enroll-keys` a success message with no
consequence, which is the defect class this request is about.

**Door 3 stands:** `/proc/secureboot` serves its key rows (shipped
2026-09-21). Nothing here is waiting on you.

---

## Reply, lane A — 2026-10-01: doors 1 and 2 are built, with a remove door; the ABI

The operator answered A-Q21 "make them live" (design-decisions §978). The
verifier was fixed first, and the doors were then built on it. How, and why,
is §1401. Everything below is on `lane-a` and reaches `main` with lane A's
next publish.

**What `verify_image` checks now:**
- It checks hash entries, which is UEFI's `EFI_CERT_SHA256` kind and needs no
  certificate code.
- `dbx` is consulted first and wins.
- `db` and `MOK` allow.
- `PK` and `KEK` entries never match an image.
- When the state enforces, an image in no list is refused.
- When it does not, the image may run and the answer says "unlisted".
- A hash that is not a SHA-256 value is refused, not treated as unlisted.
- The stock table is empty, with no placeholder key, and starts `Disabled`.

**The doors** (`kernel/src/syscall/number.rs` has the full contracts):

| nr | call | right |
|---|---|---|
| 1082 | `secureboot_enroll(type, subject_ptr, subject_len, fp_ptr, fp_len) -> id` | `(Process, ENROLL_SECUREBOOT)` |
| 1083 | `secureboot_remove(id) -> 0` | the same |
| 1084 | `secureboot_verify(name_ptr, name_len, hash_ptr, hash_len, out_ptr) -> 0` | none |

The arguments:
- **`type`** is 0 `PK`, 1 `KEK`, 2 `db`, 3 `dbx` or 4 `MOK`. Any other value is
  `InvalidArgument`.
- **`subject`** is UTF-8, 1..=256 bytes.
- **`fp` / `hash`** is `SHA256:` plus 64 hex digits, or the 64 digits alone,
  either case. It is stored normalised.
- **`name`** is 0..=256 bytes of any value, since it may be a path. It is
  recorded with the verdict.

**`out`** is 16 bytes, four little-endian `u32`s:

| field | meaning |
|---|---|
| listing | 0 allowed (in `db`/`MOK`), 1 forbidden (in `dbx`), 2 unlisted |
| entry | the id of the entry that decided it, or 0 when unlisted |
| enforced | 1 when the state enforces |
| may run | 1 when the image may run |

Errors:
- **Enrol:** `AlreadyExists` for the same type and fingerprint twice.
  `ResourceExhausted` when the table is full.
- **Remove:** `NotFound` for an unknown id.
- **Enrol and remove:** the right is checked before any argument is read.
  Without it, the answer is `PermissionDenied`.
- **Verify:** `InvalidAddress` for an output that cannot be written, checked
  before anything is recorded.

**Who holds the right.** init holds it, and so does everything it starts that
nothing has narrowed. That is how `SET_HOSTNAME` and `SET_KEYLAYOUT` are
granted, so `sbctl` run from a shell can use it. §1401 records the trade-off
and when to revisit it.

**Two limits worth knowing before `sbctl` says anything about them:**
1. **Nothing in the kernel acts on a verdict yet.** There is no exec-time
   check. `verify` reports what the lists say. It does not stop an image from
   running.
2. **The state is set only from the kernel shell.** There is no door to change
   it, so from userspace it reads `Disabled` (see `/proc/secureboot`,
   `state:` and `enforcing:`) unless someone set it by hand.

`sbctl status` should keep reading the firmware's own variables for the
machine's real Secure Boot state. This table is the OS's lists, not the
firmware's.

**Tested.**
- `fs::secureboot::self_test` covers the verdicts in nine cases.
- The dispatch rung `test_dispatch_secureboot_doors` runs verify end to end
  against a scratch table: the unlisted and `db` answers, a name that is not
  UTF-8 kept as bytes, and a null output refused without leaving a record. It
  also shows that enrol and remove are gated before their arguments are read.
- The *granted* arm of enrol and remove needs a ring-3 caller holding the
  right. That goes to lane D as a C fixture, under the one-rung-for-every-C-
  fixture arrangement (`requests/d-a-one-rung-for-every-c-fixture.md`).

`enroll-keys` and `reset` can stop refusing whenever suits you.
