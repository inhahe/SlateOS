### A-KERNEL-ERROR-FROM-CODE-MISSED-TWELVE-VARIANTS -- 2026-10-01 -- FIXED the same day (lane A)

**In short:** when a native handler's error reaches a Linux program, the
Linux layer turns the error's number back into the error, then into an
errno (`linux_from_native`). The list that turned numbers back was kept by
hand, and twelve errors added after it were missing:
- `BufferTooSmall` and `NoSuchSyscall`;
- `StaleHandle` and `NoAttribute`;
- the eight network errors, `ConnectionRefused` through `NoAddress`.

Each reached the Linux program as `EINVAL`. A native `NoAttribute`, for
example, said "invalid argument" instead of `ENODATA`.

**Fixed:** `kernel/src/error.rs` declares the enum through a macro,
`kernel_errors!`, which also writes `KernelError::ALL` from the same list.
`KernelError::from_code` searches it, and `kernel_error_from_code` is that
function. A new variant cannot be missed. The enum's text keeps its shape
for lane B's and lane D's parsers. The Linux layer's self-test (12)
round-trips every variant and checks that a native `NoAttribute` is
`ENODATA`.
