//! memfd -- a file in memory with no name: what Linux's `memfd_create(2)`
//! makes (design-decisions §1573).
//!
//! A memfd is an unnamed file on a memfs the kernel mounts for itself,
//! outside every mount namespace ([`crate::fs::Vfs::mount_internal`], Linux's
//! `shm_mnt`), so that making one does not depend on `/tmp` or `/dev/shm`
//! being where its caller looks. What a program gets is an `fs::handle`
//! handle open read-write on it, and everything a file does a memfd does
//! through the file paths: read and write, `lseek`, `ftruncate`,
//! `fallocate`, `sendfile`, record locks, `mmap` shared and writable
//! (design-decisions §1572), an `exec` of it. It goes when its last
//! descriptor or mapping does, as any unnamed file.
//!
//! Its name is shown, never used: `/proc/self/fd/N` and `/proc/self/maps`
//! read `/memfd:<name> (deleted)`, as on Linux, and nothing resolves that.
//!
//! Its seals are Linux's memfd seals, kept by `fs::sealing` beside the ones
//! of other files (`memfd_created`, `memfd_add_seals`): `SEAL` from the start
//! unless `MFD_ALLOW_SEALING`, `EXEC` with `MFD_NOEXEC_SEAL`.
//!
//! Until 2026-10-17 a memfd was an object of its own here, its bytes in a
//! private buffer, which no mapping could share: a writable shared mapping of
//! one answered `ENOSYS`, and the Linux system calls had some sixty special
//! cases for it.

use crate::error::KernelResult;
use crate::fs::handle::OpenFlags;
use crate::fs::path::PathBuf;
use crate::fs::sealing::SealFlags;
use alloc::vec::Vec;

/// How `memfd_create`'s flags shape the file.
#[derive(Debug, Clone, Copy, Default)]
pub struct CreateFlags {
    /// `MFD_ALLOW_SEALING`: seals may be added. Without it the file is born
    /// sealed with `SEAL`, so none ever can be.
    pub allow_sealing: bool,
    /// `MFD_NOEXEC_SEAL`: mode `0666` instead of `0777`, and sealed with
    /// `EXEC` so it cannot be made executable; sealing allowed.
    pub noexec_seal: bool,
}

/// The kernel's memfs that memfds are made on: its mount id
/// ([`crate::fs::Vfs::mount_internal`]), mounted by the first memfd. A
/// `Once`, not a spin lock held across the mount: the mount takes the VFS's
/// own locks, which nest under no leaf lock.
static MOUNT: spin::Once<u64> = spin::Once::new();

/// The memfd memfs's mount, mounted at the first call.
fn mount() -> u64 {
    *MOUNT.call_once(|| {
        crate::fs::Vfs::mount_internal(alloc::boxed::Box::new(crate::fs::memfs::MemFs::new()))
    })
}

/// The name a memfd made as `name` is shown under: `/memfd:<name> (deleted)`.
fn shown_as(name: &[u8]) -> PathBuf {
    let mut shown = Vec::with_capacity(name.len().saturating_add(18));
    shown.extend_from_slice(b"/memfd:");
    shown.extend_from_slice(name);
    shown.extend_from_slice(b" (deleted)");
    PathBuf::from(shown)
}

/// Make a memfd named `name` (the caller has checked its length): answers an
/// `fs::handle` handle open read-write on it, its only reference.
///
/// # Errors
///
/// The filesystem's (`DiskFull` when memory for the file cannot be had); the
/// handle table's.
pub fn create(name: &[u8], flags: CreateFlags) -> KernelResult<u64> {
    let mode = if flags.noexec_seal { 0o666 } else { 0o777 };
    let shown = shown_as(name);
    let hold = crate::fs::Vfs::create_unnamed_internal(mount(), mode, shown.clone())?;
    let seals = if flags.noexec_seal {
        SealFlags::EXEC
    } else if flags.allow_sealing {
        SealFlags::NONE
    } else {
        SealFlags::SEAL
    };
    // Before a handle exists to seal or write it through. Should the handle
    // fail, the hold goes, and with the file its seals (`fs::perfile`).
    crate::fs::sealing::memfd_created(hold.id(), seals);
    crate::fs::handle::open_held(shown, OpenFlags::READ.union(OpenFlags::WRITE), hold)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// A memfd is a file: written and read through its handle, cut and grown
/// whatever its `WRITE` seal (Linux's rule for a memfd), refused a write
/// under `WRITE` and under `FUTURE_WRITE`; born sealed with `SEAL` without
/// `MFD_ALLOW_SEALING`, and with `EXEC` and mode `0666` with
/// `MFD_NOEXEC_SEAL`; shown as `/memfd:<name> (deleted)`; and gone, its seals
/// with it, at its last close. `SEAL` alone leaves the write paths' fast path
/// on (`fs::sealing::any_sealed`); `WRITE` takes it off until the file goes.
///
/// # Errors
///
/// `InternalError` for a failed check; the file paths' own.
pub fn self_test() -> KernelResult<()> {
    use crate::error::KernelError;
    use crate::fs::handle;

    crate::serial_println!("[memfd] Running memfd self-test...");
    let fail = |what: &str| -> KernelResult<()> {
        crate::serial_println!("[memfd]   FAIL: {}", what);
        Err(KernelError::InternalError)
    };
    let seals_of = |h: u64| -> KernelResult<Option<SealFlags>> {
        let id = handle::held_identity(h)?.ok_or(KernelError::InternalError)?;
        Ok(crate::fs::sealing::memfd_seals(id))
    };

    let restricting = crate::fs::sealing::restricting_memfds();
    let h = create(
        b"selftest",
        CreateFlags {
            allow_sealing: true,
            noexec_seal: false,
        },
    )?;
    let id = handle::held_identity(h)?.ok_or(KernelError::InternalError)?;
    let result = (|| {
        if handle::handle_name(h)?.as_bytes() != b"/memfd:selftest (deleted)" {
            return fail("a memfd is not shown as /memfd:<name> (deleted)");
        }
        handle::write_at(h, 3, b"abc")?;
        let mut buf = [0xFFu8; 6];
        let n = handle::read_at(h, 0, &mut buf)?;
        if n != 6 || buf != [0, 0, 0, b'a', b'b', b'c'] {
            return fail("a write through a memfd's handle did not read back, its hole zeros");
        }
        let meta = handle::fstat(h)?;
        if meta.size != 6 || meta.permissions & 0o777 != 0o777 {
            return fail("a memfd is not a 6-byte file of mode 0777");
        }
        crate::fs::sealing::memfd_add_seals(id, SealFlags::WRITE, true)?;
        if handle::write_at(h, 0, b"x").is_ok() {
            return fail("a WRITE-sealed memfd was written");
        }
        if crate::fs::sealing::restricting_memfds() != restricting.saturating_add(1) {
            return fail("a WRITE-sealed memfd is not counted for the write paths");
        }
        handle::ftruncate(h, 2)?;
        handle::ftruncate(h, 9)?;
        if handle::fstat(h)?.size != 9 {
            return fail("a WRITE-sealed memfd could not be cut and grown (Linux's rule)");
        }
        // EXEC on an executable memfd brings the write seals.
        let all = crate::fs::sealing::memfd_add_seals(id, SealFlags::EXEC, true)?;
        if !all.contains(SealFlags::FUTURE_WRITE) || !all.contains(SealFlags::GROW) {
            return fail("EXEC on an executable memfd did not bring the write seals");
        }
        if crate::fs::sealing::memfd_add_seals(id, SealFlags::SEAL, true).is_err()
            || crate::fs::sealing::memfd_add_seals(id, SealFlags::NONE, true)
                != Err(KernelError::NotPermitted)
        {
            return fail("a SEAL-sealed memfd took a seal, or SEAL could not be added");
        }
        Ok(())
    })();
    let _ = handle::close(h);
    result?;
    if crate::fs::sealing::memfd_seals(id).is_some()
        || crate::fs::sealing::restricting_memfds() != restricting
    {
        return fail("a memfd's seals outlived its last close");
    }

    let born_sealed = create(b"sealed", CreateFlags::default())?;
    let checks = (|| {
        if seals_of(born_sealed)? != Some(SealFlags::SEAL) {
            return fail("a memfd made without MFD_ALLOW_SEALING is not sealed with SEAL");
        }
        if crate::fs::sealing::restricting_memfds() != restricting {
            return fail("a memfd sealed with SEAL alone took the write paths' fast path away");
        }
        Ok(())
    })();
    let _ = handle::close(born_sealed);
    checks?;
    let noexec = create(
        b"noexec",
        CreateFlags {
            allow_sealing: false,
            noexec_seal: true,
        },
    )?;
    let checks = (|| {
        let meta = handle::fstat(noexec)?;
        if seals_of(noexec)? != Some(SealFlags::EXEC) || meta.permissions & 0o777 != 0o666 {
            return fail("an MFD_NOEXEC_SEAL memfd is not mode 0666 sealed with EXEC");
        }
        Ok(())
    })();
    let _ = handle::close(noexec);
    checks?;
    crate::serial_println!("[memfd] memfd self-test PASSED");
    Ok(())
}
