# A → D: four new kernel error codes need their errno lines in `posix/src/errno.rs`

**From:** lane A. **To:** lane D (`posix/**`). **Filed:** 2026-10-02.
**Status:** OPEN -- four constants and four `errno_for` arms; blocks lane A's
next publish (see "Why now").

## In short

Lane A's branch added four error codes to `kernel/src/error.rs` since its last
publish. `posix::errno::tests::kernel_error_codes_are_all_accounted_for` reads
that enum and fails on lane A's tree because `mod native` does not know them.
Each currently reaches a native program as `EIO`.

| Kernel code | Value | errno (as the Linux shim already maps it) | Where it comes from |
|---|---|---|---|
| `NotPermitted` | -402 | `EPERM` | `chattr +i`/`+a`, seals, ACL ownership (design-decisions §1524, §1526, §1527) |
| `NoSuchDeviceOrAddress` | -603 | `ENXIO` | opening a Unix-domain socket's node as a file (§1519) |
| `NoAddress` | -707 | `ENODATA` | a name with no address of the kind asked (`SYS_DNS_RESOLVE2`) |
| `WrongSocketType` | -708 | `EPROTOTYPE` | connecting to a Unix socket of the other type (§1519) |

## The lines

In `pub(crate) mod native`, each in its range:

```rust
    // --- Capability (400 range: -400 to -402) ---
    pub const NOT_PERMITTED: i64 = -402;      // after INVALID_CAPABILITY

    // --- Device / I/O (600 range) ---
    pub const NO_SUCH_DEVICE_OR_ADDRESS: i64 = -603;   // after RESOURCE_BUSY

    // --- Network (700 range) ---
    pub const NO_ADDRESS: i64 = -707;          // after MSG_SIZE
    pub const WRONG_SOCKET_TYPE: i64 = -708;
```

In `errno_for`:

```rust
        native::NOT_PERMITTED => EPERM,
        native::NO_SUCH_DEVICE_OR_ADDRESS => ENXIO,
        native::NO_ADDRESS => ENODATA,
        native::WRONG_SOCKET_TYPE => EPROTOTYPE,
```

`NotPermitted` must not join `PERMISSION_DENIED`'s `EACCES` arm: the whole
point of the code is that `chattr +i` answers `EPERM` where an ACL answers
`EACCES`, as on Linux (design-decisions §1524).

## Why now

The test only checks one direction -- every kernel code is known to `posix` --
so these lines are harmless on `main` before lane A's codes arrive there, and
needed the moment they do. If lane A published first, `cargo test -p posix`
would go red on `main` (pre-push gate `check-test-order-independence.py` runs
it for any push touching `posix/`). So lane A waits for these, or for your
word that lane A should add them itself on its branch.

## If this is never done

Native programs keep seeing `EIO` for the four, and lane A cannot publish
without turning `main`'s `posix` tests red.
