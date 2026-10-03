## §202 — When the answer does not fit, `SYS_CAP_QUERY` returns an error and writes nothing, rather than truncating and reporting the size it wanted

**Date:** 2026-08-15
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** A program can ask the kernel "list the permissions I hold." It
passes a buffer — a chunk of its own memory for the kernel to write the list
into — and says how many entries fit. Sometimes the list is longer than the
buffer. There are two conventional ways to handle that, and this records which
one this call uses and why: **it fails with a distinct error and writes nothing
at all**, rather than filling the buffer with as much as fits and returning a
number the caller might not check.

**Terms:** *truncation* = writing a partial answer. *`ERANGE`* = the POSIX error
number meaning "result too large for what you gave me" — as opposed to `EINVAL`
("your request was malformed"), which tells a caller to stop rather than retry.
*Probe* = calling with a null buffer purely to learn the required size.

### The two shapes

| Shape | On overflow | Failure mode when the caller is careless |
|---|---|---|
| **A — POSIX `listxattr` style** (used by `SYS_FS_LIST_XATTR`, `handlers.rs:8882`) | Return **success**, with the *required* size as the return value; write nothing | The caller treats a success as "here is your list", reads the untouched buffer, and sees whatever was there before |
| **B — chosen here** | Return `BufferTooSmall` (`-9` → `ERANGE`); write nothing | The caller gets a negative return it must handle; ignoring it cannot be mistaken for data |

### Why B, and why the codebase now has both

The deciding argument is what a *silent* wrong answer means for this particular
call. `SYS_CAP_QUERY` enumerates authority. Lane B's libc projects its result
onto Linux `CAP_*` bits (§312). A truncated list does not read as "an error
happened" — it reads as **"this process does not hold that capability."** So the
failure lands as a *false negative on a permission check*, which is the
direction nobody notices: things quietly do not work, or worse, a security
decision is made on a short list. Under-reporting authority is the same class of
bug as over-reporting it, minus the alarm.

Shape A is not a mistake where it is used — `SYS_FS_LIST_XATTR` implements the
POSIX `listxattr` contract, and that contract is not ours to redesign; callers
are ported code that already expects it. This call has **no legacy contract**,
so it takes the shape where ignoring the failure is impossible.

The convenience A buys — learning the required size from the failed call — is
retained without the hazard: **probe mode**. Passing a null pointer or a zero
capacity returns the count and writes nothing, so "ask how big, then ask for the
data" is two cheap calls rather than one call with two meanings. That also keeps
the probe path off the expensive route entirely: it is answered from
`cap_count()` without ever building the snapshot.

### The cost, stated plainly

A caller racing against its own capability set (one that gains a capability
between the probe and the fetch) gets `ERANGE` and must loop. Shape A would have
handed it the new size in the same call. The loop is two lines and terminates —
capability grants are not adversarially fast — and this is the trade taken.

### Consequence for Lane B

`posix/src/errno.rs` mirrors kernel codes as a **non-exhaustive** constant
table, so adding `-9` cannot break their build; it falls through to `EIO` until
they map it. Until they do, an overflow reports as a generic I/O error rather
than the retryable `ERANGE` — annoying, not wrong. Filed in the reply to
`requests/b-a-cap-enumerating-query-syscall.md`.
