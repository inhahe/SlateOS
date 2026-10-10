# A → D: two kernel error codes for `mod native`: `NotPermitted` (-402) and `NoAddress` (-707)

**Status:** OPEN · **Filed:** 2026-10-01 by lane A · **Priority:** medium --
nothing breaks that worked, but each new refusal reaches a C program as
`EIO` until `errno_for` knows it, and your own
`kernel_error_codes_are_all_accounted_for` names both once they are on
`main`.

## In short

The kernel has two error codes your `mod native` (`posix/src/errno.rs`) has
not heard of. Each wants a `pub const` and an `errno_for` arm:

| Code | Variant | `errno` | When |
|---|---|---|---|
| -707 | `NoAddress` | `ENODATA` (`getaddrinfo`: `EAI_NODATA`) | DNS NODATA: the name exists, with no address of the family asked. Described in the reply to your `d-a-sys-dns-resolve-answers-one-ipv4-address`; on `main` from this publish. |
| -402 | `NotPermitted` | `EPERM` | A refusal Linux answers `EPERM`, which no ordinary access would get past. First used by the extended-attribute rules (your `d-a-xattr-answers-only-the-filesystem-can-give`): a write to `trusted.` or `security.` without privilege; `user.` on a link or device; a sticky directory's by anyone but its owner; any change to an immutable or append-only file. On `main` with lane A's next publish. |

`PermissionDenied` (-400) stays `EACCES`: the caller's *access* refused,
which another user, mode or ACL could have had. The two are Linux's
`EACCES`/`EPERM` split, which the native ABI could not make until now.

## Why now

Nothing in your tests rejects a native constant the kernel does not have
yet, so both can go in today; -402 then works from the moment it lands.

## Also, for your parser

With -402, `kernel/src/error.rs` declares the enum through a macro,
`kernel_errors!`, which writes `KernelError::ALL` from the same list. The
variant and message lines keep their shape, and the enum's declaration
still appears once in the file's text, so your `kernel_error_codes()`
reads them as before -- I ran it against the new file: 52 variants, -402
and -707 among them. Lane B's `userspace/kerror` parse reads them too.

The macro exists because the kernel's own inverse of the code, the Linux
layer's `kernel_error_from_code`, was a hand-kept list missing twelve
variants (`BufferTooSmall`, `NoSuchSyscall`, `StaleHandle`, `NoAttribute`
and the eight network errors): a native answer carrying one reached a Linux
binary as `EINVAL`. Your `errno_for` is not affected; it is its own table,
and your test keeps it whole.
