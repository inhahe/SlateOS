//! `<mcheck.h>` — glibc's heap consistency checking (`mcheck`,
//! `mcheck_pedantic`, `mcheck_check_all`, `mprobe`) and allocation tracing
//! (`mtrace`, `muntrace`), answered as glibc's libc answers them: turned off.
//!
//! Since glibc 2.34 the working versions are in `libc_malloc_debug.so`,
//! which a program gets by preloading it (`LD_PRELOAD`); `libc` itself has
//! only what answers without it.  A statically linked program cannot preload
//! anything, and every program here is statically linked, so what one gets is
//! `libc`'s: `mcheck` and `mcheck_pedantic` refuse (-1), `mprobe` says
//! checking is not on (`MCHECK_DISABLED`), and the other three do nothing --
//! `MALLOC_TRACE` asks for no file.  `errno` is untouched throughout.  glibc
//! 2.39's answers, statically linked, are `mcheck_oracle.txt`
//! (`posix/tools/oracle/mcheck_harness.py`), which the tests replay.

/// `enum mcheck_status`: consistency checking is not on.
pub const MCHECK_DISABLED: i32 = -1;
/// The block is fine.
pub const MCHECK_OK: i32 = 0;
/// The block was freed twice.
pub const MCHECK_FREE: i32 = 1;
/// The memory before the block was written over.
pub const MCHECK_HEAD: i32 = 2;
/// The memory after the block was written over.
pub const MCHECK_TAIL: i32 = 3;

/// Turn on consistency checks of every block, calling `abortfunc` -- or
/// printing and aborting, for NULL -- when one fails: -1, since there are
/// none to turn on.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mcheck(_abortfunc: Option<extern "C" fn(i32)>) -> i32 {
    -1
}

/// [`mcheck`], with every block checked on every call into the allocator:
/// -1.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mcheck_pedantic(_abortfunc: Option<extern "C" fn(i32)>) -> i32 {
    -1
}

/// Check every block now: there are no checks to run.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mcheck_check_all() {}

/// The state of the block at `ptr`: [`MCHECK_DISABLED`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mprobe(_ptr: *mut u8) -> i32 {
    MCHECK_DISABLED
}

/// Start tracing allocations to the file `MALLOC_TRACE` names: nothing is
/// traced.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mtrace() {}

/// Stop tracing: there is none.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn muntrace() {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    /// glibc 2.39's answers, statically linked.
    const ORACLE: &str = include_str!("mcheck_oracle.txt");

    extern "C" fn abortfunc(_status: i32) {}

    fn kept() -> &'static str {
        if crate::errno::get_errno() == 12345 {
            "kept"
        } else {
            "changed"
        }
    }

    /// Every answer, and every constant, is glibc's; and `MALLOC_TRACE`
    /// makes no file (none is opened: nothing here reads the variable).
    #[test]
    fn every_answer_is_glibcs() {
        let mut ours: Vec<String> = Vec::new();
        crate::errno::set_errno(12345);
        let rc = mcheck(None);
        ours.push(format!("mcheck(NULL) = {rc} errno={}", kept()));
        crate::errno::set_errno(12345);
        let rc = mcheck(Some(abortfunc));
        ours.push(format!("mcheck(abortfunc) = {rc} errno={}", kept()));
        crate::errno::set_errno(12345);
        let rc = mcheck_pedantic(None);
        ours.push(format!("mcheck_pedantic(NULL) = {rc} errno={}", kept()));
        let p = crate::malloc::malloc(10);
        crate::errno::set_errno(12345);
        let status = mprobe(p);
        ours.push(format!("mprobe(block) = {status} errno={}", kept()));
        // SAFETY: the block just allocated.
        unsafe { crate::malloc::free(p) };
        crate::errno::set_errno(12345);
        mcheck_check_all();
        ours.push(format!("mcheck_check_all() errno={}", kept()));
        crate::errno::set_errno(12345);
        mtrace();
        ours.push(format!("mtrace() errno={}", kept()));
        crate::errno::set_errno(12345);
        muntrace();
        ours.push(format!("muntrace() errno={}", kept()));
        ours.push(format!(
            "MCHECK_DISABLED={MCHECK_DISABLED} MCHECK_OK={MCHECK_OK} MCHECK_FREE={MCHECK_FREE} \
             MCHECK_HEAD={MCHECK_HEAD} MCHECK_TAIL={MCHECK_TAIL}"
        ));
        ours.push("MALLOC_TRACE file not made".into());
        let glibc: Vec<&str> = ORACLE.lines().filter(|l| !l.starts_with('#')).collect();
        assert_eq!(glibc, ours);
    }
}
