//! Ticker service — a long-running background process for testing
//! the service manager.
//!
//! Lifecycle:
//! 1. Print startup message.
//! 2. Signal readiness via `SYS_NOTIFY_READY`.
//! 3. Enter a loop: sleep 5 seconds, print a tick message, repeat.
//!
//! This exercises:
//! - Service auto-start from `/etc/services`
//! - Readiness notification (init detects the `ready` flag)
//! - Long-running service monitoring (init polls with `try_wait`)
//! - Graceful shutdown via `svc stop` (kernel sends PROCESS_KILL)

#![no_std]
#![no_main]

// ---------------------------------------------------------------------------
// Syscall numbers
// ---------------------------------------------------------------------------

const SYS_EXIT: u64 = 1;
const SYS_CLOCK_MONOTONIC: u64 = 10;
const SYS_SLEEP: u64 = 11;
const SYS_CONSOLE_WRITE: u64 = 100;
const SYS_NOTIFY_READY: u64 = 508;

// ---------------------------------------------------------------------------
// Syscall wrappers
// ---------------------------------------------------------------------------

#[inline(always)]
fn syscall0(nr: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") nr,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

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
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)); }
    }
}

fn sleep_ns(ns: u64) {
    syscall1(SYS_SLEEP, ns);
}

fn clock_monotonic() -> i64 {
    syscall0(SYS_CLOCK_MONOTONIC)
}

/// Format a u64 as decimal into `buf`, returning the slice of digits.
fn format_u64(value: u64, buf: &mut [u8; 20]) -> &[u8] {
    if value == 0 {
        buf[19] = b'0';
        return &buf[19..];
    }

    let mut pos = 20;
    let mut v = value;
    while v > 0 && pos > 0 {
        pos -= 1;
        buf[pos] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    &buf[pos..]
}

fn print_u64(v: u64) {
    let mut buf = [0u8; 20];
    let s = format_u64(v, &mut buf);
    syscall2(SYS_CONSOLE_WRITE, s.as_ptr() as u64, s.len() as u64);
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Tick interval: 5 seconds in nanoseconds.
const TICK_INTERVAL_NS: u64 = 5_000_000_000;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    print("[ticker] Starting up...\n");

    // Signal that we're ready to accept work.
    syscall0(SYS_NOTIFY_READY);
    print("[ticker] Ready.\n");

    // Main service loop: sleep and print periodic ticks.
    let mut tick: u64 = 0;
    loop {
        sleep_ns(TICK_INTERVAL_NS);
        tick += 1;

        let uptime_ns = clock_monotonic() as u64;
        let uptime_s = uptime_ns / 1_000_000_000;

        print("[ticker] tick #");
        print_u64(tick);
        print(" at ");
        print_u64(uptime_s);
        print("s uptime\n");
    }
}

// ---------------------------------------------------------------------------
// Panic handler
// ---------------------------------------------------------------------------

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    print("!!! PANIC in ticker !!!\n");
    exit(-1);
}
