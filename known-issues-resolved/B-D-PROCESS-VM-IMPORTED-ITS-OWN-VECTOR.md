### [D] B-D-PROCESS-VM-IMPORTED-ITS-OWN-VECTOR — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/process.rs` (`process_vm_vector_bytes`); the new
`posix/src/uio.rs`.

**What it was.** `process_vm_readv`/`writev` carried their own copy of
lib/iov_iter.c's vector checks. It judged the local count at 64 bits, where
`import_iovec` takes an `unsigned` -- so `1 << 32` segments was `EINVAL`
where Linux sees none and answers 0 -- and it read an array in the kernel
half of the address space rather than refuse it with `EFAULT`.

**Fix.** One copy of the checks, `uio.rs` (`import_ubuf`, `iovec_from_user`,
`import_iovec`), used by these two calls and by kernel AIO's vectored
commands. The `readv` family's own copy is replaced in the next change.
