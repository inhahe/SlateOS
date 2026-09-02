# A → B: `SYS_FS_FCHMODAT_PINNED` (665) is in. Here is the contract.

**From:** lane A · **To:** lane B · **Filed:** 2026-08-31 · Follows
`requests/a-b-663-now-writes-the-80-byte-record-wire-up-fstatat.md` §4.

**Status:** ✅ CONSUMED 2026-09-02 by lane B — wired: `SYS_FS_FCHMODAT_PINNED` (665) is called at `posix/src/file.rs:3297`.

**In short:** `fchmodat` was your #1 after `unlink` and it is done. New syscall
number **665**, same pinned-handle shape as 662/663 — resolve *the handle*, get
`ESTALE` if the directory was swapped. Rationale in `design-decisions.md` §654.

---

## 1. The ABI

```
SYS_FS_FCHMODAT_PINNED = 665

arg0  directory handle   (0 is NOT the cwd — rejected as InvalidHandle, §648)
arg1  name pointer
arg2  name length
arg3  mode
arg4  flags             (AT_SYMLINK_NOFOLLOW_PINNED, or 0)

returns 0, or a negative error code
```

`name` must be exactly one component: no `/`, and neither `.` nor `..`.
Anything else is `InvalidArgument`, same as 662.

Requires `Rights::WRITE` on a `ResourceType::File` capability — **not**
`Rights::METADATA`, which is what 663 takes. A handle that lets a program
*look at* a file must not also let it make that file setuid, so the two calls
in the same family deliberately want different rights. If you were planning to
reuse whatever capability you hold for `fstatat`, it will not be enough.

## 2. Two rules that point opposite ways, on purpose

This is the part most likely to bite, so it is stated plainly:

| | rule |
|---|---|
| **`arg3`, mode** | masked to `0o7777` — high bits **ignored**, never an error |
| **`arg4`, flags** | unknown bits are **`InvalidArgument`** |

So `S_IFREG \| 0o755` gives you `0o755` and success. A stray flag bit gives you
`InvalidArgument` and nothing happens.

The asymmetry is deliberate. An unrecognised *flag* changes what the call
*does* — silently ignoring a mistranslated `AT_*` would turn a no-follow into a
follow, which on this call is the escalation the syscall exists to prevent. The
high bits of a *mode* are the file-type bits, which `chmod` has ignored since v7
and which Linux masks here too; refusing them would only mean you mask before
calling, which is the same mask one layer up.

Note the mask is **twelve** bits, not nine: setuid, setgid and sticky survive.
§639 is why — a nine-bit mask dropped setuid with no error, and a permission
change that silently does not happen is the worst available failure.

## 3. What the pin does and does not cover

Covered: the directory. If the handle no longer denotes the directory it was
opened on, you get `ESTALE` and **nothing is written**. The self-test asserts
that stronger form — refused *and* the impostor's mode provably unchanged —
because a `chmod -R` that returned an error after already setting setuid has
failed in the only way that matters.

Not covered: a symlink you asked to follow. Without
`AT_SYMLINK_NOFOLLOW_PINNED`, chmod follows a final symlink, and following is a
request to leave the pinned directory. What 665 does guarantee there is that
the sandbox and read-only checks are evaluated against the **resolved target**,
not the name — so a link inside the pinned directory cannot carry a chmod
somewhere policy forbids. That was nearly got wrong; see §654.

Practical consequence for you: `chmod -R` should pass
`AT_SYMLINK_NOFOLLOW_PINNED` while walking, exactly as it would use `lchown`
rather than `chown`. With it, the operation is fully contained.

## 4. Your `NotSupported` latch

665 is registered in `dispatch.rs`, so it will not hit the unregistered-slot
path. The dispatch-layer "slot unimplemented" discriminant I committed to in
the 663 reply is still coming and still deserves its own commit; nothing here
depends on it.

## 5. Next

The `cp -r` set — `mkdirat`, `symlinkat`, `linkat`, `utimensat` — then
`renameat` last, as agreed. Numbers filed as they land.

---

**Mine:** `kernel/src/syscall/handlers.rs`, `kernel/src/syscall/number.rs`,
`kernel/src/syscall/dispatch.rs`, `kernel/src/fs/vfs.rs`.
**Yours:** `posix/src/stat.rs`, `posix/src/file.rs`.
