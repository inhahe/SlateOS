//! The driver process's side of the system: its entry point, system calls,
//! and the `slx_*` functions the C half calls (`linuxdrv/emul/slx.h`).
//!
//! One thread runs the whole process (design-decisions 1578 rule 5), so the
//! process's state -- the allocator, its arguments -- is plain data no
//! second thread can reach, held in a cell the type system is told is
//! shared ([`Single`]).

use core::cell::UnsafeCell;

use crate::buddy::{Buddy, MIN_ORDER};
use crate::exit;
use crate::window::Chunks;

// The SlateOS ABI note: a native program (see `services/hello`).
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

// The entry: a native program is given no arguments on its stack (it asks
// for them, `load_args`), so this only aligns the stack for the call.
core::arch::global_asm!(
    ".pushsection .text.start, \"ax\"",
    ".globl _start",
    "_start:",
    "and rsp, -16",
    "call lxkit_start",
    "ud2",
    ".popsection",
);

const SYS_EXIT: u64 = 1;
const SYS_CLOCK_MONOTONIC: u64 = 10;
const SYS_SLEEP: u64 = 11;
const SYS_GETRANDOM: u64 = 90;
const SYS_MMAP: u64 = 20;
const SYS_MUNMAP: u64 = 21;
const SYS_MPROTECT: u64 = 22;
const SYS_CONSOLE_WRITE: u64 = 100;
const SYS_PROCESS_GET_ARGS: u64 = 519;
const MAP_READ: u64 = 1 << 0;
const MAP_WRITE: u64 = 1 << 1;
const MAP_LAZY: u64 = 1 << 6;
/// mprotect's "no access at all".
const PROT_NONE: u64 = 0;

/// kmalloc's region: 256 MiB, mapped lazily, so only what is used costs
/// memory; aligned to its size, so every block is aligned to its own.
const HEAP_ORDER: u32 = 28;
/// The DMA window beside it (design-decisions 1586): as large, reserved
/// with it and untouched until backed, a chunk of 4 MiB at a time, by the
/// device's DMA memory (`SYS_PCI_DEVICE_DMA_ALLOC_AT`).
const WINDOW_ORDER: u32 = HEAP_ORDER;
const CHUNK_ORDER: u32 = 22;
const SYS_PCI_DEVICE_DMA_ALLOC_AT: u64 = 1188;
// The window's chunks are what the chunk table counts.
const _: () = assert!(1usize << (WINDOW_ORDER - CHUNK_ORDER) == crate::window::CHUNKS);

#[inline]
fn syscall3(nr: u64, a0: u64, a1: u64, a2: u64) -> i64 {
    let ret: i64;
    // SAFETY: the SlateOS system-call convention: number in rax, arguments
    // in rdi, rsi, rdx; result in rax; rcx and r11 clobbered.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr.cast_signed() => ret,
            in("rdi") a0,
            in("rsi") a1,
            in("rdx") a2,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// A four-argument system call (the fourth in r10), for the device calls
/// (`pci`).
#[inline]
pub(crate) fn syscall4(nr: u64, a0: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: the SlateOS system-call convention: number in rax, arguments
    // in rdi, rsi, rdx, r10; result in rax; rcx and r11 clobbered.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr.cast_signed() => ret,
            in("rdi") a0,
            in("rsi") a1,
            in("rdx") a2,
            in("r10") a3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Process-wide state one thread uses: see the module's documentation.
pub(crate) struct Single<T>(UnsafeCell<T>);

// SAFETY: a driver process has one thread; nothing else reaches the cell.
unsafe impl<T> Sync for Single<T> {}

impl<T> Single<T> {
    pub(crate) const fn new(value: T) -> Self {
        Single(UnsafeCell::new(value))
    }

    /// A pointer to the value, for state a task switch crosses: no
    /// reference made from it may be held across one.
    pub(crate) const fn as_ptr(&self) -> *mut T {
        self.0.get()
    }

    /// The value, to change.
    ///
    /// # Safety
    ///
    /// No other reference to it is live: the process's one thread does not
    /// re-enter the function that holds it.
    // A `&mut` from `&self` is this cell's purpose: one thread, which the
    // module's documentation says, and the caller promises not to re-enter.
    #[allow(clippy::mut_from_ref)]
    pub(crate) unsafe fn get(&self) -> &mut T {
        // SAFETY: the caller's.
        unsafe { &mut *self.0.get() }
    }
}

/// The process's memory: kmalloc's heap, the window reserved beside it, and
/// once a device's DMA is set up ([`slx_dma_open`]) the window in use.
struct Memory {
    heap: Buddy,
    window_base: usize,
    dma: Option<Dma>,
}

/// The DMA window in use: its allocator, given each chunk once backed; the
/// chunks backed and where the device sees them; the device whose memory
/// backs them; and how many address bits it has.
struct Dma {
    buddy: Buddy,
    chunks: Chunks,
    handle: i64,
    bits: u64,
}

impl Dma {
    /// A block of `size` bytes from the window, the window grown by a chunk
    /// when it has none: `None` past a chunk's size, or when the device
    /// gives no more.
    fn alloc(&mut self, size: usize) -> Option<usize> {
        loop {
            if let Some(at) = self.buddy.alloc(size) {
                return Some(at);
            }
            if size > self.chunks.chunk_bytes() {
                return None;
            }
            self.grow()?;
        }
    }

    /// The next chunk backed by the device's memory and given to the
    /// allocator.
    fn grow(&mut self) -> Option<()> {
        let at = self.chunks.next()?;
        let bytes = self.chunks.chunk_bytes();
        let device_addr = syscall4(
            SYS_PCI_DEVICE_DMA_ALLOC_AT,
            self.handle.cast_unsigned(),
            at as u64,
            bytes as u64,
            self.bits,
        );
        let Ok(device_addr) = u64::try_from(device_addr) else {
            log(b"[lx] the DMA window could not grow: the device gave no more memory\n");
            return None;
        };
        self.chunks.record(device_addr);
        // SAFETY: the chunk was just backed for this alone, inside the window.
        unsafe { self.buddy.give(at, CHUNK_ORDER) }.then_some(())
    }

    fn holds(&self, at: usize) -> bool {
        let (base, bytes) = self.buddy.span();
        at.checked_sub(base).is_some_and(|offset| offset < bytes)
    }
}

static MEMORY: Single<Option<Memory>> = Single(UnsafeCell::new(None));

/// The arguments: how many, and an array of pointers to each, NUL-terminated
/// ([`load_args`]).
static ARGS: Single<(usize, *const *const u8)> = Single(UnsafeCell::new((0, core::ptr::null())));

/// Map `bytes` of lazily backed memory, read-write: its address, page
/// aligned, or `None`.
pub(crate) fn map_lazy(bytes: usize) -> Option<usize> {
    let size = u64::try_from(bytes).ok()?;
    let at = syscall3(SYS_MMAP, 0, size, MAP_READ | MAP_WRITE | MAP_LAZY);
    usize::try_from(at).ok().filter(|&a| a != 0)
}

/// Make `bytes` at `at` -- page-aligned, mapped -- untouchable, so that any
/// access faults; whether it took.
pub(crate) fn forbid(at: usize, bytes: usize) -> bool {
    syscall3(SYS_MPROTECT, at as u64, bytes as u64, PROT_NONE) == 0
}

/// Unmap `bytes` at `at`, which [`map_lazy`] mapped; whether it took.
pub(crate) fn unmap(at: usize, bytes: usize) -> bool {
    syscall3(SYS_MUNMAP, at as u64, bytes as u64, 0) == 0
}

/// kmalloc's region, its order bytes, and the DMA window reserved beside
/// it: three times the region mapped, so that a base aligned to its size
/// with the window after it fits inside.
fn heap_init() -> Option<Memory> {
    let size = 1usize << HEAP_ORDER;
    let mapped = map_lazy(size.checked_mul(3)?)?;
    let base = mapped.checked_next_multiple_of(size)?;
    let orders = map_lazy(size >> MIN_ORDER)?;
    // SAFETY: both ranges were just mapped for this alone, and read 0.
    let heap = unsafe { Buddy::new(base, HEAP_ORDER, orders as *mut u8) }.ok()?;
    Some(Memory {
        heap,
        window_base: base.checked_add(size)?,
        dma: None,
    })
}

unsafe extern "C" {
    /// The C half: set up what must be ready before the loop first turns;
    /// `exit::OK`, or what the process exits with.
    fn lx_emul_init() -> i32;
    /// The C half: run the driver's initcalls; what the process exits with.
    fn lx_emul_main() -> i32;
}

/// The first task: the C half's `lx_emul_main`. A failure ends the process
/// at once, whatever tasks the initcalls before it started; success leaves
/// those to run, and the process ends when they do.
unsafe extern "C" fn main_task(_: *mut core::ffi::c_void) {
    // SAFETY: the C half's entry, called once.
    let code = unsafe { lx_emul_main() };
    if code != exit::OK {
        exit_with(code);
    }
}

/// The process's arguments, asked of the kernel once, kept in kmalloc's
/// memory with an array of pointers to each ([`crate::args`]). None when
/// the kernel has none, or memory runs out: the process then has no
/// arguments, which a driver must cope with anyway.
fn load_args() {
    let Ok(needed) = usize::try_from(syscall3(SYS_PROCESS_GET_ARGS, 0, 0, 0)) else {
        return;
    };
    if needed < crate::args::HEADER {
        return;
    }
    let buf = slx_alloc(needed);
    if buf.is_null() {
        return;
    }
    let got = syscall3(SYS_PROCESS_GET_ARGS, buf as u64, needed as u64, 0);
    if usize::try_from(got) != Ok(needed) {
        return;
    }
    // SAFETY: `needed` bytes at `buf`, kmalloc's and just filled.
    let packed = unsafe { core::slice::from_raw_parts(buf, needed) };
    let count = crate::args::arguments(packed).count();
    let Some(bytes) = count.checked_add(1).and_then(|n| n.checked_mul(8)) else {
        return;
    };
    // Aligned for pointers: slx_alloc's blocks are at least 16-byte aligned.
    #[allow(clippy::cast_ptr_alignment)]
    let pointers = slx_alloc(bytes).cast::<*const u8>();
    if pointers.is_null() {
        return;
    }
    for (i, arg) in crate::args::arguments(packed).enumerate() {
        // SAFETY: i < count, inside the array; each argument is followed by
        // its terminator in `packed`, which is never freed.
        unsafe { pointers.add(i).write(arg.as_ptr()) };
    }
    // SAFETY: the array's last slot.
    unsafe { pointers.add(count).write(core::ptr::null()) };
    // SAFETY: the one thread, at entry.
    unsafe { *ARGS.get() = (count, pointers.cast_const()) };
}

/// Where `_start` goes: memory, the arguments, then the C half's driver.
///
/// # Safety
///
/// Called once, by `_start`.
#[unsafe(no_mangle)]
unsafe extern "C" fn lxkit_start() -> ! {
    let Some(memory) = heap_init() else {
        log(b"[lx] the driver process's memory could not be mapped\n");
        exit_with(exit::NO_MEMORY);
    };
    // SAFETY: the one thread, before any other use of the heap.
    unsafe { *MEMORY.get() = Some(memory) };
    load_args();
    // SAFETY: the C half's set-up, once, before any task or turn.
    let ready = unsafe { lx_emul_init() };
    if ready != exit::OK {
        exit_with(ready);
    }
    // The C half's main runs as the first task, so that it can wait as any
    // task does; the process ends once nothing will ever run again.
    if crate::sched::slx_task_new(main_task, core::ptr::null_mut(), core::ptr::null_mut()) < 0 {
        log(b"[lx] the first task could not be made\n");
        exit_with(exit::NO_MEMORY);
    }
    // Done when every task finished; a task left waiting for what will never
    // come -- the first, or one an initcall started -- is a deadlock. Unless
    // a device with a handler requested may still wake it: the process then
    // waits for its device for ever, as a driver does (design-decisions
    // 1585), and is never done by itself.
    if crate::sched::run() {
        exit_with(exit::OK);
    }
    log(b"[lx] every task waits on something that will never happen\n");
    exit_with(exit::DEADLOCK)
}

pub(crate) fn log(text: &[u8]) {
    syscall3(
        SYS_CONSOLE_WRITE,
        text.as_ptr() as u64,
        text.len() as u64,
        0,
    );
}

pub(crate) fn exit_with(code: i32) -> ! {
    syscall3(SYS_EXIT, u64::from(code.cast_unsigned()), 0, 0);
    loop {
        core::hint::spin_loop();
    }
}

/// `slx_log`: write `len` bytes at `text` to the console: in as many calls
/// as the kernel needs, since one takes at most a few KiB and says how many.
/// A call that takes none ends it, what is left unsaid.
///
/// # Safety
///
/// `text` points at `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn slx_log(text: *const u8, len: usize) {
    let mut done = 0usize;
    while done < len {
        let took = syscall3(
            SYS_CONSOLE_WRITE,
            text.wrapping_add(done) as u64,
            len.saturating_sub(done) as u64,
            0,
        );
        match usize::try_from(took) {
            Ok(n) if n > 0 => done = done.saturating_add(n),
            _ => return,
        }
    }
}

/// `slx_alloc`: `size` bytes aligned to their own size rounded up to a
/// power of two, or NULL. Once a device's DMA is set up, from the DMA
/// window while it can hold the block -- so that what Linux code hands its
/// device is memory the device reaches -- and from the heap past that.
#[unsafe(no_mangle)]
pub extern "C" fn slx_alloc(size: usize) -> *mut u8 {
    // SAFETY: the one thread; slx_alloc and slx_free do not re-enter.
    let memory = unsafe { MEMORY.get() };
    memory
        .as_mut()
        .and_then(|m| {
            m.dma
                .as_mut()
                .and_then(|d| d.alloc(size))
                .or_else(|| m.heap.alloc(size))
        })
        .map_or(core::ptr::null_mut(), |at| at as *mut u8)
}

/// `slx_dma_alloc`: [`slx_alloc`] from the DMA window alone -- memory the
/// device reaches, a chunk (4 MiB) at most -- or NULL.
#[unsafe(no_mangle)]
pub extern "C" fn slx_dma_alloc(size: usize) -> *mut u8 {
    // SAFETY: as in slx_alloc.
    let memory = unsafe { MEMORY.get() };
    memory
        .as_mut()
        .and_then(|m| m.dma.as_mut())
        .and_then(|d| d.alloc(size))
        .map_or(core::ptr::null_mut(), |at| at as *mut u8)
}

/// `slx_dma_open`: the DMA window set up for the device `handle` names,
/// which has `bits` address bits (64 for any; the kernel's rule): from now
/// on [`slx_alloc`] serves from it. One window per process, the first
/// device's: 0 when it is set up, now or already; an error when it cannot
/// be.
#[unsafe(no_mangle)]
pub extern "C" fn slx_dma_open(handle: i64, bits: u64) -> i64 {
    // SAFETY: as in slx_alloc.
    let memory = unsafe { MEMORY.get() };
    let Some(m) = memory.as_mut() else {
        return -1;
    };
    if m.dma.is_some() {
        return 0;
    }
    let Some(orders) = map_lazy((1usize << WINDOW_ORDER) >> MIN_ORDER) else {
        return -1;
    };
    // SAFETY: the order bytes were just mapped for this alone, and read 0;
    // the window is the process's, reserved with the heap and untouched.
    let Ok(buddy) = (unsafe { Buddy::new_empty(m.window_base, WINDOW_ORDER, orders as *mut u8) })
    else {
        return -1;
    };
    m.dma = Some(Dma {
        buddy,
        chunks: Chunks::new(m.window_base, CHUNK_ORDER),
        handle,
        bits,
    });
    0
}

/// `slx_dma_bits`: the window's device has `bits` address bits from now on:
/// the chunks backed after this are within them.
#[unsafe(no_mangle)]
pub extern "C" fn slx_dma_bits(bits: u64) {
    // SAFETY: as in slx_alloc.
    let memory = unsafe { MEMORY.get() };
    if let Some(d) = memory.as_mut().and_then(|m| m.dma.as_mut()) {
        d.bits = bits;
    }
}

/// `slx_dma_address`: where the window's device sees the `len` bytes at
/// `ptr`, or `u64::MAX` when it does not -- memory outside the window, or a
/// range crossing two of its chunks.
#[unsafe(no_mangle)]
pub extern "C" fn slx_dma_address(ptr: *const u8, len: usize) -> u64 {
    // SAFETY: as in slx_alloc.
    let memory = unsafe { MEMORY.get() };
    memory
        .as_ref()
        .and_then(|m| m.dma.as_ref())
        .and_then(|d| d.chunks.device_address(ptr as usize, len))
        .unwrap_or(u64::MAX)
}

/// `slx_alloc_size`: the bytes `slx_alloc(size)` hands out -- `size` rounded
/// up as the heap rounds it -- or `size` itself where the heap could never
/// hold that much, which `slx_alloc` then refuses.
#[unsafe(no_mangle)]
pub extern "C" fn slx_alloc_size(size: usize) -> usize {
    // SAFETY: as in slx_alloc.
    let memory = unsafe { MEMORY.get() };
    memory
        .as_ref()
        .and_then(|m| m.heap.order_for(size))
        .and_then(|k| 1usize.checked_shl(k))
        .unwrap_or(size)
}

/// `slx_block_size`: the bytes the block `slx_alloc` gave at `ptr` holds, or
/// 0 for anything it did not give.
#[unsafe(no_mangle)]
pub extern "C" fn slx_block_size(ptr: *const u8) -> usize {
    // SAFETY: as in slx_alloc.
    let memory = unsafe { MEMORY.get() };
    memory
        .as_ref()
        .and_then(|m| match m.dma.as_ref().filter(|d| d.holds(ptr as usize)) {
            Some(d) => d.buddy.block_size(ptr as usize),
            None => m.heap.block_size(ptr as usize),
        })
        .unwrap_or(0)
}

/// `slx_heap`: the memory Linux's direct map covers -- kmalloc's heap and
/// the DMA window beside it -- over which the C half lays its page array:
/// where it begins and how many bytes it spans; both 0 when there is none.
///
/// # Safety
///
/// `base` and `bytes` point at writable words.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn slx_heap(base: *mut usize, bytes: *mut usize) {
    // SAFETY: as in slx_alloc.
    let memory = unsafe { MEMORY.get() };
    let (at, size) = memory.as_ref().map_or((0, 0), |m| {
        let (at, heap) = m.heap.span();
        (at, heap.saturating_mul(2))
    });
    // SAFETY: the caller's.
    unsafe {
        base.write(at);
        bytes.write(size);
    }
}

/// `slx_map_lazy`: `bytes` of memory reading 0, read-write, backed only
/// where it is touched; NULL when it cannot be mapped.
#[unsafe(no_mangle)]
pub extern "C" fn slx_map_lazy(bytes: usize) -> *mut u8 {
    map_lazy(bytes).map_or(core::ptr::null_mut(), |at| at as *mut u8)
}

/// `slx_free`: give back what `slx_alloc` gave; null is ignored, and anything
/// else not given is refused and said.
#[unsafe(no_mangle)]
pub extern "C" fn slx_free(ptr: *const u8) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: as in slx_alloc.
    let memory = unsafe { MEMORY.get() };
    let freed =
        memory
            .as_mut()
            .is_some_and(|m| match m.dma.as_mut().filter(|d| d.holds(ptr as usize)) {
                Some(d) => d.buddy.free(ptr as usize),
                None => m.heap.free(ptr as usize),
            });
    if !freed {
        log(b"[lx] BUG: kfree of memory kmalloc did not give\n");
    }
}

/// `slx_random`: fill `len` bytes at `buf` from the system's CSPRNG. Linux
/// code that asks for randomness cannot go on without it, so a system that
/// gives none stops the driver.
///
/// # Safety
///
/// `buf` points at `len` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn slx_random(buf: *mut u8, len: usize) {
    let mut done = 0usize;
    while done < len {
        // A short read is not an error: the kernel caps one call.
        let got = syscall3(
            SYS_GETRANDOM,
            buf.wrapping_add(done) as u64,
            len.saturating_sub(done) as u64,
            0,
        );
        match usize::try_from(got) {
            Ok(n) if n > 0 => done = done.saturating_add(n),
            _ => {
                log(b"[lx] the system gave no random bytes\n");
                exit_with(exit::NO_RANDOM);
            }
        }
    }
}

/// Nanoseconds of the monotonic clock. The tasks' deadlines and Linux's
/// `jiffies` are kept by it, so a system that gives none ends the process.
pub(crate) fn now_ns() -> u64 {
    if let Ok(ns) = u64::try_from(syscall3(SYS_CLOCK_MONOTONIC, 0, 0, 0)) {
        return ns;
    }
    log(b"[lx] the system gave no clock\n");
    exit_with(exit::NO_CLOCK)
}

/// Sleep the process's thread for `ns` nanoseconds: no task may run. Its
/// result is not needed: a sleep cut short or refused costs only a turn of
/// the loop, which reads the clock again.
pub(crate) fn sleep_ns(ns: u64) {
    syscall3(SYS_SLEEP, ns, 0, 0);
}

/// `slx_now_ns`: nanoseconds of the monotonic clock.
#[unsafe(no_mangle)]
pub extern "C" fn slx_now_ns() -> u64 {
    now_ns()
}

/// `slx_argc`: how many arguments the process was given.
#[unsafe(no_mangle)]
pub extern "C" fn slx_argc() -> i32 {
    // SAFETY: written once at entry, only read after.
    i32::try_from(unsafe { ARGS.get() }.0).unwrap_or(i32::MAX)
}

/// `slx_argv`: the `n`-th argument, NUL-terminated; null past the last.
#[unsafe(no_mangle)]
pub extern "C" fn slx_argv(n: i32) -> *const u8 {
    // SAFETY: as in slx_argc.
    let (count, pointers) = *unsafe { ARGS.get() };
    match usize::try_from(n) {
        // SAFETY: n < count indexes the kernel's argument array.
        Ok(i) if i < count => unsafe { pointers.add(i).read() },
        _ => core::ptr::null(),
    }
}

/// `slx_exit`: end the process with `code`.
#[unsafe(no_mangle)]
pub extern "C" fn slx_exit(code: i32) -> ! {
    exit_with(code)
}

/// `slx_stop`: Linux code reached `what` (NUL-terminated), which the layer
/// does not implement, called from `caller`: said -- the caller's address,
/// for the driver's symbol table, which its file keeps -- and the process
/// ends with [`exit::STOP`].
///
/// # Safety
///
/// `what` is a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn slx_stop(what: *const u8, caller: usize) -> ! {
    log(b"[lx] stop: Linux code reached ");
    // SAFETY: the caller's: a generated stop passes its own name.
    let name = unsafe { core::ffi::CStr::from_ptr(what.cast()) };
    log(name.to_bytes());
    log(b", which the layer does not implement, called from 0x");
    let mut hex = [0u8; 16];
    for (i, digit) in hex.iter_mut().enumerate() {
        let nibble = (caller >> (60usize.saturating_sub(i.saturating_mul(4)))) & 0xF;
        *digit = b"0123456789abcdef".get(nibble).copied().unwrap_or(b'?');
    }
    log(&hex);
    log(b"\n");
    exit_with(exit::STOP)
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    log(b"[lx] the layer's Rust half panicked\n");
    exit_with(exit::PANIC)
}
