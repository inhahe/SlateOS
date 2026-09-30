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

/// Like [`abi`], for a kernel struct that is **extensible by size**.
///
/// Some syscalls take the structure's size as an explicit argument —
/// `landlock_create_ruleset(attr, size, flags)`, `perf_event_open` via
/// `attr->size`, `clone3`, `openat2` — precisely so the structure can grow
/// without breaking callers built against an older header. For those, ours
/// being *smaller* than the header is not a defect: it is an older ABI
/// version, and the kernel is required to accept it.
///
/// So the size assertion is `>=` rather than `==`. That still catches the
/// failure that matters — ours being **larger** than the kernel knows about,
/// which would have the kernel read past what it understands — and it still
/// checks every named field at `==`, which is where a real layout error would
/// show.
///
/// **Use this only where the syscall genuinely takes a size.** A `statvfs` that
/// is short is short; only an explicit size argument makes a shorter structure
/// a version rather than a bug.
macro_rules! abi_extensible {
    ($out:expr, $hdrs:expr, $rust:ty, $cty:literal, $hdr:literal
     $(, $f:ident $(as $cn:literal)? )* $(,)?) => {{
        $hdrs.insert($hdr);
        let _ = writeln!(
            $out,
            "_Static_assert(sizeof({}) >= {}, \"{} size (extensible: ours may be an older version)\");",
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

    // --- <ucontext.h>: the context getcontext records and a signal handler
    // receives -------------------------------------------------------------
    abi!(
        out,
        hdrs,
        crate::ucontext::UcontextT,
        "ucontext_t",
        "ucontext.h",
        uc_flags,
        uc_link,
        uc_stack,
        uc_mcontext,
        uc_sigmask,
        fpregs_mem as "__fpregs_mem"
    );
    abi!(
        out,
        hdrs,
        crate::ucontext::McontextT,
        "mcontext_t",
        "ucontext.h",
        gregs,
        fpregs,
        reserved1 as "__reserved1"
    );
    abi!(
        out,
        hdrs,
        crate::ucontext::Fpstate,
        "struct _fpstate",
        "ucontext.h",
        cwd,
        swd,
        ftw,
        fop,
        rip,
        rdp,
        mxcsr,
        mxcr_mask,
        st as "_st",
        xmm as "_xmm",
        padding
    );

    // --- threads: the cleanup record musl's pthread_cleanup_push declares --
    abi!(out, hdrs, crate::pthread::Ptcb, "struct __ptcb", "pthread.h",
         f as "__f", x as "__x", next as "__next");

    // --- search: the table of hsearch_r's own ------------------------------
    abi!(out, hdrs, crate::search::HsearchData, "struct hsearch_data", "search.h",
         tab as "__tab", unused1 as "__unused1", unused2 as "__unused2");
    // ...and the entry hsearch and hsearch_r take and hand back.
    abi!(
        out,
        hdrs,
        crate::search::Entry,
        "ENTRY",
        "search.h",
        key,
        data
    );

    // --- DNS messages: ns_initparse's handle, ns_parserr's record ----------
    abi!(out, hdrs, crate::resolv::NsMsg, "ns_msg", "arpa/nameser.h",
         msg as "_msg", eom as "_eom", id as "_id", flags as "_flags", counts as "_counts",
         sections as "_sections", sect as "_sect", rrnum as "_rrnum", msg_ptr as "_msg_ptr");
    abi!(out, hdrs, crate::resolv::NsRr, "ns_rr", "arpa/nameser.h",
         name as "name", type_ as "type", rr_class as "rr_class", ttl as "ttl",
         rdlength as "rdlength", rdata as "rdata");
    abi!(out, hdrs, crate::resolv::NsFlagData, "struct _ns_flagdata", "arpa/nameser.h",
         mask as "mask", shift as "shift");

    // --- time: ftime's result -------------------------------------------------
    abi!(
        out,
        hdrs,
        crate::time::Timeb,
        "struct timeb",
        "sys/timeb.h",
        time,
        millitm,
        timezone,
        dstflag
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
    // C11's `mtx_t` and `cnd_t` are musl's `pthread_mutex_t` and
    // `pthread_cond_t` under other names (`threads.rs` passes them straight
    // to the pthread functions), so their sizes must agree too.
    abi!(
        out,
        hdrs,
        crate::pthread::PthreadMutexT,
        "mtx_t",
        "threads.h"
    );
    abi!(
        out,
        hdrs,
        crate::pthread::PthreadCondT,
        "cnd_t",
        "threads.h"
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
    // One `cpu_set_t`: pthread.rs's copy, with its `__bits` field, went on
    // 2026-09-27, and its affinity calls take sched.rs's.
    abi!(out, hdrs, crate::sched::CpuSetT, "cpu_set_t", "sched.h");
    // `__opcode` is a bit-field, which `offsetof` cannot name; the size and
    // every field around it pin it.
    abi!(
        out,
        hdrs,
        crate::fenv::FenvT,
        "fenv_t",
        "fenv.h",
        control_word as "__control_word",
        status_word as "__status_word",
        tags as "__tags",
        eip as "__eip",
        cs_selector as "__cs_selector",
        data_offset as "__data_offset",
        data_selector as "__data_selector",
        mxcsr as "__mxcsr",
    );

    // --- what only posix/include defines: types musl's headers lack, which
    // the overlay C is compiled with here lays out as glibc does
    // (design-decisions §1141) -- and `check-libc-overlay.py` holds the
    // overlay's to glibc's own ---------------------------------------------
    abi!(
        out,
        hdrs,
        crate::fenv::FemodeT,
        "femode_t",
        "fenv.h",
        control_word as "__control_word",
        mxcsr as "__mxcsr",
    );
    abi!(
        out,
        hdrs,
        crate::fts::Fts,
        "FTS",
        "fts.h",
        fts_cur,
        fts_child,
        fts_array,
        fts_dev,
        fts_path,
        fts_rfd,
        fts_pathlen,
        fts_nitems,
        fts_compar,
        fts_options
    );
    abi!(
        out,
        hdrs,
        crate::fts::FtsEnt,
        "FTSENT",
        "fts.h",
        fts_cycle,
        fts_parent,
        fts_link,
        fts_number,
        fts_pointer,
        fts_accpath,
        fts_path,
        fts_errno,
        fts_symfd,
        fts_pathlen,
        fts_namelen,
        fts_ino,
        fts_dev,
        fts_nlink,
        fts_level,
        fts_info,
        fts_flags,
        fts_instr,
        fts_statp,
        fts_name
    );
    abi!(
        out,
        hdrs,
        crate::malloc::Mallinfo,
        "struct mallinfo",
        "malloc.h",
        arena,
        ordblks,
        smblks,
        hblks,
        hblkhd,
        usmblks,
        fsmblks,
        uordblks,
        fordblks,
        keepcost
    );
    abi!(
        out,
        hdrs,
        crate::malloc::Mallinfo2,
        "struct mallinfo2",
        "malloc.h",
        arena,
        ordblks,
        smblks,
        hblks,
        hblkhd,
        usmblks,
        fsmblks,
        uordblks,
        fordblks,
        keepcost
    );
    abi!(
        out,
        hdrs,
        crate::legacy::Sigstack,
        "struct sigstack",
        "signal.h",
        ss_sp,
        ss_onstack
    );
    abi!(
        out,
        hdrs,
        crate::prng::RandomData,
        "struct random_data",
        "stdlib.h",
        fptr,
        rptr,
        state,
        rand_type,
        rand_deg,
        rand_sep,
        end_ptr
    );
    abi!(
        out,
        hdrs,
        crate::prng::Drand48Data,
        "struct drand48_data",
        "stdlib.h",
        x as "__x",
        old_x as "__old_x",
        c as "__c",
        init as "__init",
        a as "__a",
    );
    abi!(
        out,
        hdrs,
        crate::dlfcn::DlSerpath,
        "Dl_serpath",
        "dlfcn.h",
        dls_name,
        dls_flags
    );
    abi!(
        out,
        hdrs,
        crate::dlfcn::DlSerinfo,
        "Dl_serinfo",
        "dlfcn.h",
        dls_size,
        dls_cnt,
        dls_serpath
    );
    abi!(
        out,
        hdrs,
        crate::dlfcn::DlFindObject,
        "struct dl_find_object",
        "dlfcn.h",
        dlfo_flags,
        dlfo_map_start,
        dlfo_map_end,
        dlfo_link_map,
        dlfo_eh_frame,
        reserved as "__dflo_reserved",
    );

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
    // `struct mmsghdr` is in <sys/socket.h> only under _GNU_SOURCE, which the
    // fixture defines. Its second field is `unsigned int`, not `size_t`: the
    // obvious guess is wrong and would move nothing on x86-64 while breaking
    // the moment anything reads the array with a different stride.
    abi!(
        out,
        hdrs,
        crate::socket::Mmsghdr,
        "struct mmsghdr",
        "sys/socket.h",
        msg_hdr,
        msg_len
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
        crate::socket::InAddr,
        "struct in_addr",
        "netinet/in.h",
        s_addr
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
        crate::sysv_sem::SemidDs,
        "struct semid_ds",
        "sys/sem.h",
        sem_perm,
        sem_otime,
        sem_ctime,
        sem_nsems
    );
    abi!(
        out,
        hdrs,
        crate::sysv_sem::Seminfo,
        "struct seminfo",
        "sys/sem.h",
        semmap,
        semmni,
        semmns,
        semmnu,
        semmsl,
        semopm,
        semume,
        semusz,
        semvmx,
        semaem
    );
    abi!(
        out,
        hdrs,
        crate::mntent::Mntent,
        "struct mntent",
        "mntent.h",
        mnt_fsname,
        mnt_dir,
        mnt_type,
        mnt_opts,
        mnt_freq,
        mnt_passno
    );
    abi!(
        out,
        hdrs,
        crate::stdio::CookieIoFunctions,
        "cookie_io_functions_t",
        "stdio.h",
        read,
        write,
        seek,
        close
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
    abi!(out, hdrs, crate::wchar::MbstateT, "mbstate_t", "wchar.h");
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
        crate::dlfcn::LinkMap,
        "struct link_map",
        "link.h",
        l_addr,
        l_name,
        l_ld,
        l_next,
        l_prev
    );
    abi!(
        out,
        hdrs,
        crate::dlfcn::DlPhdrInfo,
        "struct dl_phdr_info",
        "link.h",
        dlpi_addr,
        dlpi_name,
        dlpi_phdr,
        dlpi_phnum,
        dlpi_adds,
        dlpi_subs,
        dlpi_tls_modid,
        dlpi_tls_data
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
        crate::inet::EtherAddr,
        "struct ether_addr",
        "net/ethernet.h",
        ether_addr_octet
    );
    abi!(
        out,
        hdrs,
        crate::netdb::Servent,
        "struct servent",
        "netdb.h",
        s_name,
        s_aliases,
        s_port,
        s_proto
    );
    abi!(
        out,
        hdrs,
        crate::netdb::Protoent,
        "struct protoent",
        "netdb.h",
        p_name,
        p_aliases,
        p_proto
    );
    abi!(
        out,
        hdrs,
        crate::netdb::Netent,
        "struct netent",
        "netdb.h",
        n_name,
        n_aliases,
        n_addrtype,
        n_net
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
        crate::socket::SockaddrLl,
        "struct sockaddr_ll",
        "netpacket/packet.h",
        sll_family,
        sll_protocol,
        sll_ifindex,
        sll_hatype,
        sll_pkttype,
        sll_halen,
        sll_addr
    );
    abi!(
        out,
        hdrs,
        crate::socket::RtnlLinkStats,
        "struct rtnl_link_stats",
        "linux/if_link.h",
        rx_packets,
        tx_packets,
        rx_bytes,
        tx_bytes,
        rx_errors,
        tx_errors,
        rx_dropped,
        tx_dropped,
        multicast,
        collisions,
        rx_length_errors,
        rx_over_errors,
        rx_crc_errors,
        rx_frame_errors,
        rx_fifo_errors,
        rx_missed_errors,
        tx_aborted_errors,
        tx_carrier_errors,
        tx_fifo_errors,
        tx_heartbeat_errors,
        tx_window_errors,
        rx_compressed,
        tx_compressed,
        rx_nohandler
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

    // The System V IPC trio. These were size-only while ours flattened
    // `ipc_perm` into `msg_perm_uid` and friends -- names that correspond to
    // nothing in C. They nest it now, so every field can be checked, which is
    // the difference between "the bytes add up" and "the fields are where the
    // caller will look for them".
    abi!(
        out,
        hdrs,
        crate::linux_ipc::IpcPerm,
        "struct ipc_perm",
        "sys/ipc.h",
        __ipc_perm_key,
        uid,
        gid,
        cuid,
        cgid,
        mode,
        __ipc_perm_seq
    );
    abi!(
        out,
        hdrs,
        crate::sysv_msg::MsqidDs,
        "struct msqid_ds",
        "sys/msg.h",
        msg_perm,
        msg_stime,
        msg_rtime,
        msg_ctime,
        msg_cbytes,
        msg_qnum,
        msg_qbytes,
        msg_lspid,
        msg_lrpid
    );
    abi!(
        out,
        hdrs,
        crate::sysv_msg::Msginfo,
        "struct msginfo",
        "sys/msg.h",
        msgpool,
        msgmap,
        msgmax,
        msgmnb,
        msgmni,
        msgssz,
        msgtql,
        msgseg
    );
    abi!(
        out,
        hdrs,
        crate::sysv_shm::ShmidDs,
        "struct shmid_ds",
        "sys/shm.h",
        shm_perm,
        shm_segsz,
        shm_atime,
        shm_dtime,
        shm_ctime,
        shm_cpid,
        shm_lpid,
        shm_nattch
    );

    // Size only: ours embeds a `struct timeval` as two flattened fields, so the
    // names do not correspond even though the bytes may.
    abi!(
        out,
        hdrs,
        crate::sys_timex::Timex,
        "struct timex",
        "sys/timex.h"
    );

    // --- the kernel-ABI structs, checked against the kernel's own uapi -------
    //
    // These do not cross a *C library* boundary; they cross the **syscall**
    // boundary. That does not put them outside this gate — it changes which
    // header is the oracle, and zig ships the Linux uapi headers alongside
    // musl's, so the same mechanism checks both. The rule the gate follows is
    // "compare against the header that defines the boundary this type
    // crosses", and for `struct open_how` that is `<linux/openat2.h>`.
    //
    // This was nearly got wrong by assumption: the twelve below were about to
    // be written off as "kernel formats, out of remit" on the strength of
    // musl not declaring them. Probing first showed it declares eleven and the
    // uapi supplies the rest.
    abi!(
        out,
        hdrs,
        crate::sys_capability::CapUserHeader,
        "struct __user_cap_header_struct",
        "linux/capability.h",
        version,
        pid
    );
    abi!(
        out,
        hdrs,
        crate::sys_capability::CapUserData,
        "struct __user_cap_data_struct",
        "linux/capability.h",
        effective,
        permitted,
        inheritable
    );
    // 16 against the header's 24: the header has grown a `scoped` field that
    // this kernel's ABI version does not have. `landlock_create_ruleset` takes
    // the size, so a 16-byte attr is a valid older request.
    abi_extensible!(
        out,
        hdrs,
        crate::linux_landlock::LandlockRulesetAttr,
        "struct landlock_ruleset_attr",
        "linux/landlock.h",
        handled_access_fs,
        handled_access_net
    );
    abi!(
        out,
        hdrs,
        crate::linux_aio_abi::IoEvent,
        "struct io_event",
        "linux/aio_abi.h",
        data,
        obj,
        res,
        res2
    );
    abi_extensible!(
        out,
        hdrs,
        crate::file::OpenHow,
        "struct open_how",
        "linux/openat2.h",
        flags,
        mode,
        resolve
    );
    abi!(
        out,
        hdrs,
        crate::file::FileHandle,
        "struct file_handle",
        "fcntl.h",
        handle_bytes,
        handle_type
    );
    abi!(
        out,
        hdrs,
        crate::file::StatxTimestamp,
        "struct statx_timestamp",
        "sys/stat.h",
        tv_sec,
        tv_nsec
    );

    // Size only: each of these has a union or a bitfield where ours has a
    // flattened field, so the *names* cannot correspond even where the bytes
    // do -- `perf_event_attr`'s `sample_period`/`sample_freq` union is ours
    // as `sample_period_or_freq`, and `io_uring_params` nests two `*_off`
    // structs. The size is the part a caller depends on.
    // 112 against the header's 144. `perf_event_open` reads `attr->size` and
    // accepts every historical value of it (PERF_ATTR_SIZE_VER0 was 64), so a
    // smaller structure is a version rather than a defect.
    abi_extensible!(
        out,
        hdrs,
        crate::linux_perf_event::PerfEventAttr,
        "struct perf_event_attr",
        "linux/perf_event.h"
    );
    abi!(
        out,
        hdrs,
        crate::linux_aio_abi::Iocb,
        "struct iocb",
        "linux/aio_abi.h"
    );
    abi!(
        out,
        hdrs,
        crate::linux_io_uring::IoUringParams,
        "struct io_uring_params",
        "linux/io_uring.h"
    );
    abi_extensible!(
        out,
        hdrs,
        crate::process::CloneArgs,
        "struct clone_args",
        "linux/sched.h"
    );
    abi!(out, hdrs, crate::file::Statx, "struct statx", "sys/stat.h");

    // `va_list` is `struct __va_list_tag[1]` and the tag type is incomplete in
    // clang's headers, so only the size can be asked for -- which is the whole
    // of what matters here: `VaList` is what `va_trampoline!` builds on the
    // stack for a C variadic callee to walk.
    abi!(out, hdrs, crate::printf::VaList, "va_list", "stdarg.h");

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
