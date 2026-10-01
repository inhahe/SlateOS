//! Hello world — a minimal userspace program.
//!
//! Prints a greeting and exits.  Used to test process spawning from
//! the init process.

#![no_std]
#![no_main]

// Declares this binary SlateOS-native: the ELF note the kernel trusts over
// every Linux signal when it decides which system-call table a program gets
// (design-decisions.md §33). posix's crt0 carries the same note for every
// program linked against the libc; this service links no libc, so it carries
// its own, and `linker.ld` keeps the section and gives it a `PT_NOTE`.
//
// Layout (ELF note, 4-byte aligned): n_namesz 8, n_descsz 4, n_type 1
// (NT_SLATEOS_ABI), the name "SlateOS" with its NUL (8 bytes, already
// aligned), then the descriptor: ABI revision 1, little-endian.
//
// Assembled, as crt0's is, and NOT a `#[used]` static, which is how it was
// first written. On ELF, `#[used]` gives its section SHF_GNU_RETAIN, and LLVM
// tags any object that uses that GNU extension ELFOSABI_GNU. The service then
// reached the kernel labelled a *Linux* program, and a kernel that did not yet
// rank the note above that label ran it on the Linux system-call table, where
// this program's numbers name other calls (its SYS_EXIT, 1, is Linux's
// `write`). On the boot of 2026-09-25 the kernel's container self-test, which
// execs `hello`, sat behind a task that never ended (known-issues.md ->
// B-D-USED-STATIC-TAGGED-THE-SERVICES-GNU). A plain `.pushsection` sets no
// GNU flag, so the header says System V, as the program's system calls do.
// `KEEP` in linker.ld is what keeps the section in the image; nothing here
// needs a retain flag.
core::arch::global_asm!(
    ".pushsection .note.slateos, \"a\", @note",
    ".balign 4",
    ".long 8",
    ".long 4",
    ".long 1",
    ".asciz \"SlateOS\"",
    ".long 1",
    ".popsection",
);

const SYS_EXIT: u64 = 1;
const SYS_CONSOLE_WRITE: u64 = 100;

#[inline(always)]
fn syscall1(nr: u64, arg0: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") nr,
            in("rdi") arg0,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

#[inline(always)]
fn syscall2(nr: u64, arg0: u64, arg1: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") nr,
            in("rdi") arg0,
            in("rsi") arg1,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

fn print(s: &str) {
    syscall2(SYS_CONSOLE_WRITE, s.as_ptr() as u64, s.len() as u64);
}

fn exit(code: i64) -> ! {
    syscall1(SYS_EXIT, code as u64);
    loop {
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    print("Hello from a spawned process!\n");
    exit(0);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    print("!!! PANIC in hello !!!\n");
    exit(-1);
}
