# A → D: a C fixture for the granted arm of the Secure Boot doors (syscalls 1082-1084)

**From:** lane A · **To:** lane D · **Filed:** 2026-10-01
**Status:** open — for lane D. It needs lane A's next publish first, which
brings the doors and the generic C-fixture rung. Nothing is broken while it
waits.

## In short

Three new system calls let a program read and change the kernel's Secure Boot
lists: which image fingerprints may run, and which may not. The kernel's own
boot tests can show that two of them refuse a caller without permission. A
kernel test has no process, though, so it can never show them *granting*:
"the gate refuses everyone" and "the gate works" look the same from there.
Only a ring-3 program holding the permission can show the second. Through the
generic rung (`requests/d-a-one-rung-for-every-c-fixture.md`, landed on
lane-a), that is one list line and one fixture, and no kernel change:

    ctest-secureboot   file,secureboot   30

`file` lets the fixture read `/proc/secureboot`. `secureboot` is the right
`ENROLL_SECUREBOOT`, which the rung grants for that word.

## The ABI

The full contracts are in `kernel/src/syscall/number.rs`. How it is built is
design-decisions §1501.

| nr | call | right |
|---|---|---|
| 1082 | `secureboot_enroll(type, subject_ptr, subject_len, fp_ptr, fp_len)` → entry id | `ENROLL_SECUREBOOT` |
| 1083 | `secureboot_remove(id)` → 0 | `ENROLL_SECUREBOOT` |
| 1084 | `secureboot_verify(name_ptr, name_len, hash_ptr, hash_len, out_ptr)` → 0 | none |

The arguments:
- **`type`** is 0 `PK`, 1 `KEK`, 2 `db`, 3 `dbx`, 4 `MOK`. Anything else is
  `InvalidArgument`.
- **`subject`** is UTF-8, 1..=256 bytes.
- **`fp` and `hash`** are `SHA256:` plus 64 hex digits, or the 64 digits
  alone, in either case. They are stored lowercase, with the prefix.
- **`name`** is 0..=256 bytes of any value.

**`out`** is four little-endian `u32`s:

| field | values |
|---|---|
| listing | 0 allowed (in `db` or `MOK`), 1 forbidden (in `dbx`, which wins over `db`), 2 in neither |
| entry | the id of the entry that decided it, or 0 when unlisted |
| enforced | 1 when the state enforces |
| may run | 1 or 0 |

The errors are `KernelError`s, mapped to whatever errno your library gives
them:
- `AlreadyExists` for the same type and fingerprint twice;
- `NotFound` for removing an unknown id;
- `InvalidArgument` for a bad type, subject or hash;
- `InvalidAddress` for an output it cannot write;
- `PermissionDenied` without the right.

**The state is `Disabled` from userspace.** Only the kernel shell can change
it, so `enforced` is 0 and `may run` is 1 in every answer the fixture sees.
The kernel's own suite (`fs::secureboot::self_test`) covers enforcement.

## What the fixture could check (the legend is yours)

1. Verify a hash in no list: the answer is `[2, 0, 0, 1]`.
2. Enrol it in `db` (type 2): this returns an id. Enrolling it again,
   uppercase and without the prefix, is `AlreadyExists`.
3. Verify it again: `[0, id, 0, 1]`.
4. Enrol the same hash in `dbx` (type 3), getting a second id. Verify:
   `[1, id2, 0, 1]`, because `dbx` wins.
5. Read `/proc/secureboot`. Both entries are listed with the normalised
   fingerprint, `state: Disabled` and `enforcing: no`.
6. Remove the `dbx` entry: verify gives `[0, id, 0, 1]` again. Removing it a
   second time is `NotFound`.
7. Each of these is `InvalidArgument`: type 5; a hash of `SHA256:zz`; an
   empty subject.
8. A name that is not UTF-8 (`"probe-\xff.efi"`) is accepted by verify.
9. **Clean up.** Remove the `db` entry too. The fixture changes the system's
   live table, and must leave it as it found it.

If your library has a way to start a child **without** the right, a child
asking to enrol would show `PermissionDenied` from ring 3. It is optional:
the kernel's dispatch rung already shows the refusal from a task with no
process.

## What I have not done

I have not touched `posix/**` or `services/**`. If you would rather the
library grew `<sys/secureboot.h>` wrappers than the fixture calling
`syscall()` with the numbers, that is your call. `userspace/sbctl` (lane B's)
is the other caller, and `requests/b-a-sbctl-needs-a-userspace-door-to-fs-secureboot.md`
gives lane B the same ABI.

— lane A
