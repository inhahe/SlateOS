//! Hello world — a minimal userspace program.
//!
//! Prints a greeting and exits.  Used to test process spawning from
//! the init process.

#![no_std]
#![no_main]

/// Declares this binary SlateOS-native: the ELF note the kernel trusts over
/// every Linux signal when it decides which system-call table a program gets
/// (design-decisions.md §33). posix's crt0 carries the same note for every
/// program linked against the libc; this service links no libc, so it carries
/// its own, and `linker.ld` keeps the section and gives it a `PT_NOTE`.
///
/// Layout: `n_namesz` 8, `n_descsz` 4, `n_type` 1 (`NT_SLATEOS_ABI`), the name
/// "SlateOS" with its NUL, then the ABI revision, 1.
#[used]
#[unsafe(link_section = ".note.slateos")]
static SLATEOS_ABI_NOTE: [u32; 6] = [
    8,
    4,
    1,
    u32::from_le_bytes(*b"Slat"),
    u32::from_le_bytes(*b"eOS\0"),
    1,
];

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
