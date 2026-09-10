//! Emits C `_Static_assert`s for every `#[repr(C)]` type a C caller fills in,
//! so that musl's own headers can check our field offsets.
//!
//! # Why this exists
//!
//! On 2026-09-09 `Sigaction` was found to carry the *kernel's* field order
//! under a comment claiming it was glibc's. Both layouts are 152 bytes, so a
//! size test passed; the one field they agree on — `sa_handler` at offset 0 —
//! is the one every test in the tree exercises, so handlers worked and nothing
//! looked wrong for months. See `design-decisions.md` §1010.
//!
//! **No Rust test could have caught it.** Rust builds a `#[repr(C)]` struct by
//! field *name*, so it agrees with itself whichever order it declares; the
//! layout only matters at a boundary with C, and the crate's own tests have
//! none. Worse, the test that existed *certified* the bug: it pinned
//! `offset_of!(Sigaction, sa_flags) == 8` under a comment reading "glibc
//! x86_64".
//!
//! The response to a whole class of defect should not be three hand-written
//! constants, which is all that fixing `Sigaction`, `Termios` and `StackT` by
//! hand amounted to.
//!
//! # The shape, and why it is not a table
//!
//! This module prints C. It asserts nothing itself. `scripts/check-libc-abi.py`
//! runs [`emit_abi_asserts`], takes the C between the markers, and compiles it
//! with `zig cc --target=x86_64-linux-musl`, which is the toolchain every C
//! port in this tree is already built with.
//!
//! So the numbers come from Rust — `size_of` and `offset_of!`, never typed out
//! — and the truth comes from musl. There is no third place holding a copy of
//! either, which is the property that makes this worth building rather than a
//! second list to rot. What is listed here is only *which* types to check and
//! what C calls them, and `check-libc-abi.py` derives that list independently
//! from the exported `extern "C"` signatures and refuses a type that crosses
//! the boundary without an entry.
//!
//! # Adding a type
//!
//! One line. `abi!(out, hdrs, RustType, "c type name", "header.h", field, …)`,
//! with `field as "c_name"` where the C spelling differs. A type that is the
//! *kernel's* wire format rather than the C library's does not belong here at
//! all — see `KERNEL_NCCS` in `crate::ioctl` for the honest example of one, and
//! the translation function beside it.

// `std`, not `alloc`: this module is `#[cfg(test)]` and tests only ever build
// for the host, where the crate has a `std` to borrow from.
use core::fmt::Write as _;
use std::collections::BTreeSet;
use std::string::String;

/// Opens the C block on stdout. `scripts/check-libc-abi.py` cuts on this.
pub(crate) const BEGIN: &str = "===ABI-C-BEGIN===";
/// Closes it.
pub(crate) const END: &str = "===ABI-C-END===";

/// One type's size assertion plus one offset assertion per named field.
///
/// `$cty` is the C type *expression* — `"struct sigaction"`, `"stack_t"` —
/// because not every one of these is a `struct` tag.
macro_rules! abi {
    ($out:expr, $hdrs:expr, $rust:ty, $cty:literal, $hdr:literal
     $(, $f:ident $(as $cn:literal)? )* $(,)?) => {{
        $hdrs.insert($hdr);
        let _ = writeln!(
            $out,
            "_Static_assert(sizeof({}) == {}, \"{} size\");",
            $cty,
            core::mem::size_of::<$rust>(),
            $cty
        );
        $(
            {
                #[allow(unused_mut, unused_assignments)]
                let mut cname: &str = stringify!($f);
                $( cname = $cn; )?
                let _ = writeln!(
                    $out,
                    "_Static_assert(offsetof({}, {}) == {}, \"{}.{}\");",
                    $cty,
                    cname,
                    core::mem::offset_of!($rust, $f),
                    $cty,
                    cname
                );
            }
        )*
    }};
}

/// The C translation unit to compile against musl.
///
/// Public to the crate rather than private to the test so that the test is a
/// two-line caller — the interesting thing here is the table, and a table
/// buried inside `#[test]` is one nobody edits.
pub(crate) fn abi_asserts() -> String {
    let mut hdrs: BTreeSet<&'static str> = BTreeSet::new();
    let mut body = String::new();
    let out = &mut body;

    // --- signals: the family §1010 was found in ----------------------------
    abi!(
        out,
        hdrs,
        crate::signal::Sigaction,
        "struct sigaction",
        "signal.h",
        sa_handler,
        sa_mask,
        sa_flags,
        sa_restorer
    );
    abi!(
        out,
        hdrs,
        crate::signal::StackT,
        "stack_t",
        "signal.h",
        ss_sp,
        ss_flags,
        ss_size
    );

    // --- terminals ---------------------------------------------------------
    abi!(out, hdrs, crate::ioctl::Termios, "struct termios", "termios.h",
         c_iflag, c_oflag, c_cflag, c_lflag, c_line, c_cc,
         c_ispeed as "__c_ispeed", c_ospeed as "__c_ospeed");
    abi!(
        out,
        hdrs,
        crate::ioctl::Winsize,
        "struct winsize",
        "sys/ioctl.h",
        ws_row,
        ws_col,
        ws_xpixel,
        ws_ypixel
    );

    // --- time --------------------------------------------------------------
    abi!(
        out,
        hdrs,
        crate::stat::Timespec,
        "struct timespec",
        "time.h",
        tv_sec,
        tv_nsec
    );
    abi!(
        out,
        hdrs,
        crate::file::Timeval,
        "struct timeval",
        "sys/time.h",
        tv_sec,
        tv_usec
    );
    abi!(
        out,
        hdrs,
        crate::epoll::Itimerspec,
        "struct itimerspec",
        "time.h",
        it_interval,
        it_value
    );
    abi!(
        out,
        hdrs,
        crate::time::Tm,
        "struct tm",
        "time.h",
        tm_sec,
        tm_min,
        tm_hour,
        tm_mday,
        tm_mon,
        tm_year,
        tm_wday,
        tm_yday,
        tm_isdst,
        tm_gmtoff,
        tm_zone
    );

    // --- files and directories ---------------------------------------------
    abi!(
        out,
        hdrs,
        crate::stat::Stat,
        "struct stat",
        "sys/stat.h",
        st_dev,
        st_ino,
        st_nlink,
        st_mode,
        st_uid,
        st_gid,
        st_rdev,
        st_size,
        st_blksize,
        st_blocks,
        st_atim,
        st_mtim,
        st_ctim
    );
    abi!(
        out,
        hdrs,
        crate::dirent::Dirent,
        "struct dirent",
        "dirent.h",
        d_ino,
        d_off,
        d_reclen,
        d_type,
        d_name
    );
    abi!(
        out,
        hdrs,
        crate::file::Iovec,
        "struct iovec",
        "sys/uio.h",
        iov_base,
        iov_len
    );
    abi!(
        out,
        hdrs,
        crate::fcntl_ops::Flock,
        "struct flock",
        "fcntl.h",
        l_type,
        l_whence,
        l_start,
        l_len,
        l_pid
    );

    // --- polling and events -------------------------------------------------
    abi!(
        out,
        hdrs,
        crate::poll::Pollfd,
        "struct pollfd",
        "poll.h",
        fd,
        events,
        revents
    );

    // --- process limits and accounting --------------------------------------
    abi!(
        out,
        hdrs,
        crate::resource::Rlimit,
        "struct rlimit",
        "sys/resource.h",
        rlim_cur,
        rlim_max
    );
    abi!(
        out,
        hdrs,
        crate::utsname::Utsname,
        "struct utsname",
        "sys/utsname.h",
        // musl spells it `domainname` under _GNU_SOURCE (glibc keeps
        // `__domainname` and #defines the short name), and the generated
        // source defines _GNU_SOURCE, so no override is needed. Found by
        // this gate on its first run, which is the point of it.
        sysname,
        nodename,
        release,
        version,
        machine,
        domainname
    );

    // --- types a C program declares BY VALUE --------------------------------
    //
    // The highest stakes in this file, and the reason they came first when the
    // ratchet started shrinking. A C program writes `pthread_mutex_t m;` on its
    // own stack and hands us `&m`; if our idea of the type is *smaller* than
    // musl's, every write we make past our idea lands in the caller's frame.
    // No field names, deliberately -- musl declares these as unions of
    // anonymous arrays and the internals are nobody's business. Size and
    // alignment are the whole contract.
    abi!(out, hdrs, crate::signal::SigsetT, "sigset_t", "signal.h");
    abi!(
        out,
        hdrs,
        crate::pthread::PthreadMutexT,
        "pthread_mutex_t",
        "pthread.h"
    );
    abi!(
        out,
        hdrs,
        crate::pthread::PthreadCondT,
        "pthread_cond_t",
        "pthread.h"
    );
    abi!(
        out,
        hdrs,
        crate::pthread::PthreadRwlockT,
        "pthread_rwlock_t",
        "pthread.h"
    );
    abi!(
        out,
        hdrs,
        crate::pthread::PthreadOnceT,
        "pthread_once_t",
        "pthread.h"
    );
    abi!(
        out,
        hdrs,
        crate::pthread::PthreadBarrierT,
        "pthread_barrier_t",
        "pthread.h"
    );
    abi!(out, hdrs, crate::semaphore::SemT, "sem_t", "semaphore.h");
    // Two independent `CpuSetT` definitions exist -- `pthread::CpuSetT` with a
    // `__bits` field and `sched::CpuSetT` with a `bits` field. Both are
    // checked, because a duplicate type is exactly the thing that drifts.
    abi!(out, hdrs, crate::pthread::CpuSetT, "cpu_set_t", "sched.h");
    abi!(out, hdrs, crate::sched::CpuSetT, "cpu_set_t", "sched.h");

    // --- regex: `regex_t` is declared by value too ---------------------------
    abi!(out, hdrs, crate::regex::RegexT, "regex_t", "regex.h");
    abi!(
        out,
        hdrs,
        crate::regex::RegMatch,
        "regmatch_t",
        "regex.h",
        rm_so,
        rm_eo
    );

    // --- small odds and ends -------------------------------------------------
    abi!(
        out,
        hdrs,
        crate::sched::SchedParam,
        "struct sched_param",
        "sched.h",
        sched_priority
    );
    abi!(
        out,
        hdrs,
        crate::sys_times::Tms,
        "struct tms",
        "sys/times.h",
        tms_utime,
        tms_stime,
        tms_cutime,
        tms_cstime
    );
    abi!(
        out,
        hdrs,
        crate::utime::Utimbuf,
        "struct utimbuf",
        "utime.h",
        actime,
        modtime
    );

    let mut src = String::new();
    let _ = writeln!(
        src,
        "/* Generated by posix::abi_layout::abi_asserts. Do not edit. */\n\
         #define _GNU_SOURCE 1"
    );
    for h in &hdrs {
        let _ = writeln!(src, "#include <{h}>");
    }
    let _ = writeln!(src, "#include <stddef.h>\n");
    src.push_str(&body);
    src
}

#[cfg(test)]
mod tests {
    use super::{BEGIN, END, abi_asserts};

    /// Print the C. **This test asserts almost nothing** — the checking
    /// happens when `scripts/check-libc-abi.py` compiles what it prints
    /// against musl's headers, which is the only place the truth lives.
    ///
    /// Run it by hand with:
    ///
    /// ```text
    /// cargo test -p posix --target x86_64-pc-windows-gnu --lib \
    ///     abi_layout::tests::emit_abi_asserts -- --nocapture --exact
    /// ```
    #[test]
    fn emit_abi_asserts() {
        let src = abi_asserts();
        // A guard against the emitter silently producing nothing, which would
        // make the gate pass by having no work to do -- the failure mode a
        // generated check is most likely to have.
        assert!(
            src.matches("_Static_assert").count() >= 40,
            "suspiciously few assertions emitted: the table is probably broken"
        );
        std::println!("{BEGIN}");
        std::print!("{src}");
        std::println!("{END}");
    }
}
