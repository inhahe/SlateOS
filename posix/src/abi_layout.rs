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

    // --- the types real ports touch ------------------------------------------
    abi!(
        out,
        hdrs,
        crate::epoll::EpollEvent,
        "struct epoll_event",
        "sys/epoll.h",
        events,
        data
    );
    abi!(
        out,
        hdrs,
        crate::linux_time::Itimerval,
        "struct itimerval",
        "sys/time.h",
        it_interval,
        it_value
    );
    abi!(
        out,
        hdrs,
        crate::pwd::Passwd,
        "struct passwd",
        "pwd.h",
        pw_name,
        pw_passwd,
        pw_uid,
        pw_gid,
        pw_gecos,
        pw_dir,
        pw_shell
    );
    abi!(
        out,
        hdrs,
        crate::pwd::Group,
        "struct group",
        "grp.h",
        gr_name,
        gr_passwd,
        gr_gid,
        gr_mem
    );
    abi!(
        out,
        hdrs,
        crate::resource::Rusage,
        "struct rusage",
        "sys/resource.h",
        ru_utime,
        ru_stime,
        ru_maxrss,
        ru_ixrss,
        ru_idrss,
        ru_isrss,
        ru_minflt,
        ru_majflt,
        ru_nswap,
        ru_inblock,
        ru_oublock,
        ru_msgsnd,
        ru_msgrcv,
        ru_nsignals,
        ru_nvcsw,
        ru_nivcsw
    );
    abi!(
        out,
        hdrs,
        crate::shadow::Spwd,
        "struct spwd",
        "shadow.h",
        sp_namp,
        sp_pwdp,
        sp_lstchg,
        sp_min,
        sp_max,
        sp_warn,
        sp_inact,
        sp_expire,
        sp_flag
    );
    abi!(
        out,
        hdrs,
        crate::socket::Msghdr,
        "struct msghdr",
        "sys/socket.h",
        msg_name,
        msg_namelen,
        msg_iov,
        msg_iovlen,
        msg_control,
        msg_controllen,
        msg_flags
    );
    abi!(
        out,
        hdrs,
        crate::socket::SockaddrIn,
        "struct sockaddr_in",
        "netinet/in.h",
        sin_family,
        sin_port,
        sin_addr,
        sin_zero
    );
    abi!(
        out,
        hdrs,
        crate::socket::Sockaddr,
        "struct sockaddr",
        "sys/socket.h",
        sa_family,
        sa_data
    );
    abi!(
        out,
        hdrs,
        crate::socket::Addrinfo,
        "struct addrinfo",
        "netdb.h",
        ai_flags,
        ai_family,
        ai_socktype,
        ai_protocol,
        ai_addrlen,
        ai_addr,
        ai_canonname,
        ai_next
    );
    abi!(
        out,
        hdrs,
        crate::socket::IfNameindex,
        "struct if_nameindex",
        "net/if.h",
        if_index,
        if_name
    );
    abi!(
        out,
        hdrs,
        crate::statvfs::Statvfs,
        "struct statvfs",
        "sys/statvfs.h",
        f_bsize,
        f_frsize,
        f_blocks,
        f_bfree,
        f_bavail,
        f_files,
        f_ffree,
        f_favail,
        f_fsid,
        f_flag,
        f_namemax
    );
    abi!(
        out,
        hdrs,
        crate::sysv_sem::Sembuf,
        "struct sembuf",
        "sys/sem.h",
        sem_num,
        sem_op,
        sem_flg
    );
    abi!(
        out,
        hdrs,
        crate::unistd::Mntent,
        "struct mntent",
        "mntent.h",
        mnt_fsname,
        mnt_dir,
        mnt_type,
        mnt_opts,
        mnt_freq,
        mnt_passno
    );

    // --- more by-value types, and the rest of the ordinary libc surface -----
    //
    // The opaque ones carry no field list on purpose: musl declares them as
    // unions of anonymous arrays, so size and alignment are the whole contract
    // and naming an internal field would be asserting something musl does not
    // promise.
    abi!(out, hdrs, crate::poll::FdSet, "fd_set", "sys/select.h");
    abi!(out, hdrs, crate::signal::SiginfoT, "siginfo_t", "signal.h");
    abi!(out, hdrs, crate::glob::GlobT, "glob_t", "glob.h");
    abi!(
        out,
        hdrs,
        crate::wordexp::WordexpT,
        "wordexp_t",
        "wordexp.h"
    );
    abi!(out, hdrs, crate::uchar::MbstateT, "mbstate_t", "wchar.h");
    abi!(
        out,
        hdrs,
        crate::spawn::PosixSpawnattrT,
        "posix_spawnattr_t",
        "spawn.h"
    );
    abi!(
        out,
        hdrs,
        crate::spawn::PosixSpawnFileActionsT,
        "posix_spawn_file_actions_t",
        "spawn.h"
    );

    abi!(
        out,
        hdrs,
        crate::aio::Aiocb,
        "struct aiocb",
        "aio.h",
        aio_fildes,
        aio_offset,
        aio_buf,
        aio_nbytes,
        aio_reqprio,
        aio_sigevent,
        aio_lio_opcode
    );
    abi!(
        out,
        hdrs,
        crate::dlfcn::DlInfo,
        "Dl_info",
        "dlfcn.h",
        dli_fname,
        dli_fbase,
        dli_sname,
        dli_saddr
    );
    abi!(
        out,
        hdrs,
        crate::getopt::Option,
        "struct option",
        "getopt.h",
        name,
        has_arg,
        flag,
        val
    );
    abi!(
        out,
        hdrs,
        crate::mqueue::MqAttr,
        "struct mq_attr",
        "mqueue.h",
        mq_flags,
        mq_maxmsg,
        mq_msgsize,
        mq_curmsgs
    );
    abi!(
        out,
        hdrs,
        crate::socket::Hostent,
        "struct hostent",
        "netdb.h",
        h_name,
        h_aliases,
        h_addrtype,
        h_length,
        h_addr_list
    );
    abi!(
        out,
        hdrs,
        crate::socket::Ifaddrs,
        "struct ifaddrs",
        "ifaddrs.h",
        ifa_next,
        ifa_name,
        ifa_flags,
        ifa_addr,
        ifa_netmask,
        ifa_broadaddr,
        ifa_data
    );
    abi!(
        out,
        hdrs,
        crate::statvfs::Statfs,
        "struct statfs",
        "sys/vfs.h",
        f_type,
        f_bsize,
        f_blocks,
        f_bfree,
        f_bavail,
        f_files,
        f_ffree,
        f_fsid,
        f_namelen,
        f_frsize,
        f_flags
    );
    abi!(
        out,
        hdrs,
        crate::time::Sigevent,
        "struct sigevent",
        "signal.h",
        sigev_value,
        sigev_signo,
        sigev_notify
    );
    abi!(
        out,
        hdrs,
        crate::unistd::Sysinfo,
        "struct sysinfo",
        "sys/sysinfo.h",
        uptime,
        loads,
        totalram,
        freeram,
        sharedram,
        bufferram,
        totalswap,
        freeswap,
        procs,
        totalhigh,
        freehigh,
        mem_unit
    );
    abi!(
        out,
        hdrs,
        crate::utmpx::Utmpx,
        "struct utmpx",
        "utmpx.h",
        ut_type,
        ut_pid,
        ut_line,
        ut_id,
        ut_user,
        ut_host,
        ut_exit,
        ut_session,
        ut_tv,
        ut_addr_v6
    );

    // Size only: ours flattens a nested C struct, so the field *names* do not
    // correspond even where the bytes do. `msqid_ds` and `shmid_ds` open with a
    // `struct ipc_perm`, and `timex` embeds a `struct timeval`. Checking the
    // size is still worth doing -- these are filled in for a caller -- and the
    // flattening is worth revisiting separately.
    abi!(
        out,
        hdrs,
        crate::sysv_msg::MsqidDs,
        "struct msqid_ds",
        "sys/msg.h"
    );
    abi!(
        out,
        hdrs,
        crate::sysv_shm::ShmidDs,
        "struct shmid_ds",
        "sys/shm.h"
    );
    abi!(
        out,
        hdrs,
        crate::sys_timex::Timex,
        "struct timex",
        "sys/timex.h"
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
