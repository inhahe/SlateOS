//! Reading the process's file-mode creation mask **without changing it**.
//!
//! POSIX gives `umask(2)` and nothing else: it sets the mask and returns the
//! previous one, so the only portable way to *read* it is
//!
//! ```text
//! old = umask(0);   /* or umask(0777) */
//! umask(old);       /* put it back */
//! ```
//!
//! which is what gnulib and every GNU utility that needs the value does
//! (`copy.c`'s `cached_umask`, `chmod.c`, `mkdir.c`). Between those two calls
//! the mask is wrong, and anything the process creates in that window is
//! created at the wrong mode. In a single-threaded utility that window is
//! provably empty and the idiom is correct — which is why upstream is entitled
//! to it and this module is not about a bug in GNU.
//!
//! ## Why that is not good enough here
//!
//! Our utilities are exercised by `cargo test`, which runs the tests of one
//! binary **on threads of one process**. The mask is per-process, so the window
//! is no longer empty: one thread's read of the mask is another thread's
//! `open(…, 0666)` landing at mode `0000`. That is not a hypothetical. Two
//! tests in `cp` failed intermittently for weeks —
//!
//! ```text
//! copies_a_file_into_a_directory                    assert!(ok) failed
//! dereference_fails_on_a_dangling_link_in_the_tree  read: Permission denied
//! ```
//!
//! — because `cp`'s copy of the idiom probed with `umask(0777)`, denying
//! everything, and under `#[cfg(test)]` it probed on *every copy* rather than
//! once. A file another test created in that window arrived unreadable by its
//! own owner. The failure looked like a bug in the code under test and was a
//! bug in the instrument.
//!
//! No lock can fix it. A mutex only orders the threads that agree to take it,
//! and the thread being harmed here is not reading the umask at all — it is
//! creating a file, which is every other test in the binary.
//!
//! ## The windowless read
//!
//! Linux publishes the mask in `/proc/self/status` as a `Umask:` line (since
//! 4.7), and so does SlateOS — `kernel/src/fs/procfs.rs` writes
//! `Umask:\t{mask:04o}` into the same file for the same reason. Reading it
//! there answers the question without a write, so there is no window to lose a
//! race in, and the answer is live rather than remembered.
//!
//! The probe remains as the fallback, for a Unix that has no `/proc` mounted.
//! Where it is reached the old hazard is back, so it probes with `umask(0)` —
//! *permitting* everything — rather than `umask(0777)`. Both are wrong for the
//! duration; a file that briefly comes out too permissive is a race that some
//! other test may fail to notice, while one that comes out mode `0000` is a
//! race that fails whoever opens it. Between two bad windows, take the one that
//! does not deny.

/// The process's file-mode creation mask, in the low nine bits.
///
/// Read live, not remembered: a caller that changes the mask and asks again
/// gets the new value. Callers that need the value many times in one run (a
/// deep `cp -r` consults it per file) should cache it themselves, which is
/// also what GNU does.
///
/// On a host with no such concept the answer is `0` — "mask nothing" — so that
/// code written against the target's rules produces the mode it asked for
/// rather than a mode narrowed by an invented mask.
#[must_use]
pub fn current() -> u32 {
    #[cfg(unix)]
    {
        from_proc().unwrap_or_else(probe)
    }
    #[cfg(not(unix))]
    {
        0
    }
}

/// The mask as `/proc/self/status` reports it, or `None` if that file is
/// absent, unreadable, or from a kernel too old to carry the field.
///
/// The line is `Umask:\t0022` — octal, four digits, no `0` prefix — and is
/// parsed as octal for that reason. A decimal read would turn `0022` into 22
/// and quietly mask the wrong bits.
#[cfg(unix)]
fn from_proc() -> Option<u32> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let field = status.lines().find_map(|l| l.strip_prefix("Umask:"))?;
    let value = u32::from_str_radix(field.trim(), 8).ok()?;
    // A mask with bits outside the permission bits is not something this file
    // should be able to say; treat it as an unrecognised format rather than
    // narrowing modes by a bit pattern nobody set.
    (value & !0o777 == 0).then_some(value)
}

// SAFETY (declaration): `umask` is POSIX, takes and returns `mode_t`, and has
// no failure mode. `mode_t` is `u32` on Linux and on x86_64-slateos.
#[cfg(unix)]
unsafe extern "C" {
    fn umask(mask: u32) -> u32;
}

/// The POSIX idiom, used only where [`from_proc`] could not answer.
///
/// See the module docs for why `0` and not `0777`.
#[cfg(unix)]
fn probe() -> u32 {
    // SAFETY: `umask` takes and returns a plain integer, touches no memory and
    // cannot fail. The second call restores what the first found, so the mask
    // is left exactly as it was.
    unsafe {
        let old = umask(0);
        umask(old);
        old
    }
}

#[cfg(test)]
mod tests {
    /// Set the mask, run `f`, put it back — the tests' own use of the very
    /// call this module exists to avoid. Serialised against itself so two of
    /// these cannot interleave.
    ///
    /// The masks used below are all *permissive-ish* (`0022`, `0077`, `0002`):
    /// a file another thread creates while one is installed is still readable
    /// and writable by its owner, so this helper cannot cause the failure the
    /// module docs describe.
    #[cfg(unix)]
    fn with_mask<T>(mask: u32, f: impl FnOnce() -> T) -> T {
        use std::sync::Mutex;
        static TURN: Mutex<()> = Mutex::new(());
        let _guard = TURN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: as in `probe` — a plain integer in, a plain integer out.
        let old = unsafe { super::umask(mask) };
        let out = f();
        // SAFETY: as above.
        unsafe { super::umask(old) };
        out
    }

    /// Whatever the mask is set to is what comes back — which is the whole
    /// contract, and is also the check that `/proc` is being parsed as octal:
    /// `0022` read as decimal would come back as 22 (`0o26`).
    #[cfg(unix)]
    #[test]
    fn the_mask_reads_back_as_it_was_set() {
        for mask in [0o022, 0o077, 0o002, 0o000] {
            let got = with_mask(mask, super::current);
            assert_eq!(got, mask, "set {mask:04o}, read {got:04o}");
        }
    }

    /// The fast path and the fallback must agree, or a host with `/proc` and a
    /// host without would compute different modes from the same mask.
    #[cfg(unix)]
    #[test]
    fn proc_and_the_probe_agree() {
        for mask in [0o022, 0o077, 0o002] {
            with_mask(mask, || {
                assert_eq!(super::probe(), mask);
                if let Some(from_proc) = super::from_proc() {
                    assert_eq!(from_proc, mask, "at mask {mask:04o}");
                }
            });
        }
    }

    /// Reading must not *write*. This is the regression test for the whole
    /// module: it installs a mask, reads it a hundred times, and checks that
    /// the probe still finds it — a `current` that forgot to restore would
    /// leave the mask at whatever it probed with.
    #[cfg(unix)]
    #[test]
    fn reading_the_mask_leaves_it_alone() {
        with_mask(0o027, || {
            for _ in 0..100 {
                assert_eq!(super::current(), 0o027);
            }
            assert_eq!(super::probe(), 0o027, "the mask survived being read");
        });
    }

    /// On a host that has no umask the answer is `0`, not a guess.
    #[cfg(not(unix))]
    #[test]
    fn a_host_without_a_umask_masks_nothing() {
        assert_eq!(super::current(), 0);
    }
}
