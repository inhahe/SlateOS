//! Hello world — a minimal userspace program.
//!
//! Prints a greeting and exits.  Used to test process spawning from
//! the init process.

#![no_std]
#![no_main]

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
    loop { unsafe { core::arch::asm!("hlt", options(nomem, nostack)); } }
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
