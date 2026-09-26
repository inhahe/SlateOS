// Entry counts here are bounded by the list's capacity, itself bounded by
// what `malloc` could give; every product is checked before it is used.
#![allow(clippy::arithmetic_side_effects)]

//! The handlers `exit` and `quick_exit` run, and the destructors of a
//! thread's `thread_local` objects.
//!
//! # The exit lists
//!
//! glibc's `__exit_funcs` and `__quick_exit_funcs` (stdlib/cxa_atexit.c,
//! stdlib/exit.c): one list per kind of exit, holding every way a handler can
//! be registered.
//!
//! | registered by | called as |
//! |---|---|
//! | `atexit(f)`, `at_quick_exit(f)` | `f()` |
//! | `on_exit(f, arg)` | `f(status, arg)` |
//! | `__cxa_atexit(f, arg, dso)` | `f(arg)` -- a C++ static destructor and its object |
//!
//! [`run`] takes handlers off the end of a list and calls each with the lock
//! released, so they run in reverse order of registration with the kinds
//! interleaved, and a handler that registers another has it run next.
//! [`finalize`] is `__cxa_finalize`: it runs one module's C++ entries, or
//! every termination function, marking each done first so nothing runs twice.
//! The first 32 entries are in place, so registering during startup allocates
//! nothing; past them the list grows through `malloc`.
//!
//! Until 2026-09-26 these were three fixed tables of 32: `__cxa_atexit`
//! registered its destructor as a plain `atexit` handler and dropped the
//! object, so every C++ static destructor ran on a garbage `this` -- CMake,
//! the first large C++ program to finish on SlateOS, crashed in its first
//! one -- the 33rd registration of any kind failed, and `on_exit` handlers
//! were stored and never called (`B-D-CXA-ATEXIT-DROPPED-THE-OBJECT`).
//!
//! # `thread_local` destructors
//!
//! glibc's `__cxa_thread_atexit_impl` and `__call_tls_dtors`
//! (stdlib/cxa_thread_atexit_impl.c): each thread keeps its own list, newest
//! first, in [`crate::perthread::PerThread::tls_dtors`].  [`run_thread_dtors`]
//! empties it when the thread ends (`pthread_exit`, which a returning start
//! routine reaches too) and, for the thread that calls it, at the start of
//! `exit` -- before the exit list, as glibc does.  Until 2026-09-26 the
//! registration was accepted and the destructor never run.
//!
//! # What is not glibc's
//!
//! glibc's `atexit` and `at_quick_exit` are inline wrappers linked into each
//! module, which pass the caller's `__dso_handle`; ours are ordinary functions
//! and cannot know it.  So `__cxa_finalize(dso)` for one module runs only its
//! `__cxa_atexit` entries, never an `atexit` handler that module registered.
//! Nothing here unloads modules -- `dlclose` is a stub -- so the difference
//! cannot yet be seen.

use crate::perprocess::{PoolLock, lock_pool, process_global};

/// `atexit`'s and `at_quick_exit`'s handler.
pub(crate) type AtexitFn = extern "C" fn();
/// `on_exit`'s handler: the exit status and its argument.
pub(crate) type OnExitFn = extern "C" fn(i32, *mut u8);
/// `__cxa_atexit`'s and `__cxa_thread_atexit_impl`'s handler: a destructor
/// and its object.
pub(crate) type CxaFn = extern "C" fn(*mut u8);

/// One registered handler.
///
/// Neither `Debug` nor `PartialEq`: comparing function pointers is not
/// meaningful (one function can have several addresses), and nothing here
/// needs to.
#[derive(Clone, Copy)]
pub(crate) enum Handler {
    /// Run by `__cxa_finalize`, taken off the list, or never filled: skipped.
    Done,
    /// `f()`.
    At(AtexitFn),
    /// `f(status, arg)`.
    On(OnExitFn, *mut u8),
    /// `f(arg)`, and the module (`__dso_handle`) it belongs to.
    Cxa(CxaFn, *mut u8, *mut u8),
}

/// Entries kept in place, before the list needs `malloc` -- glibc's
/// `struct exit_function_list` holds 32 as well.
const INLINE: usize = 32;

/// One list: `INLINE` entries in place, the rest in a `malloc` block.
pub(crate) struct List {
    inline: [Handler; INLINE],
    /// Entries `INLINE..INLINE + heap_cap`, or NULL.  Never freed: a list
    /// lives as long as its process.
    heap: *mut Handler,
    heap_cap: usize,
    len: usize,
    /// Advanced by every registration, so a pass that called out can tell a
    /// handler registered another -- glibc's `__new_exitfn_called`.
    generation: u64,
}

impl List {
    /// No handlers.  A `const fn`, as `process_global!` needs.
    pub(crate) const fn new() -> Self {
        Self {
            inline: [Handler::Done; INLINE],
            heap: core::ptr::null_mut(),
            heap_cap: 0,
            len: 0,
            generation: 0,
        }
    }

    fn slot(&mut self, i: usize) -> Option<&mut Handler> {
        if i < INLINE {
            return self.inline.get_mut(i);
        }
        let j = i - INLINE;
        // SAFETY: `heap` holds `heap_cap` initialised entries, and the list
        // is borrowed mutably, so the reference is unique.
        (j < self.heap_cap).then(|| unsafe { &mut *self.heap.add(j) })
    }

    /// Append `h`; `Err` when more room cannot be had.
    ///
    /// Entries `__cxa_finalize` already ran are dropped off the end first,
    /// as glibc's `__new_exitfn` reuses them, so a module loaded and
    /// finalised over and over does not grow the list.
    fn push(&mut self, h: Handler) -> Result<(), ()> {
        while let Some(top) = self.len.checked_sub(1) {
            if !matches!(self.slot(top), Some(Handler::Done)) {
                break;
            }
            self.len = top;
        }
        if self.len >= INLINE && self.len - INLINE == self.heap_cap {
            let cap = self.heap_cap.checked_mul(2).ok_or(())?.max(INLINE);
            let bytes = cap.checked_mul(size_of::<Handler>()).ok_or(())?;
            // SAFETY: `heap` is NULL or this list's own block; `realloc(NULL,
            // n)` is `malloc(n)`.  The list's lock is held across the call,
            // which is safe because the allocator registers no exit handler
            // and so never re-enters this module.
            let p =
                unsafe { crate::malloc::realloc(self.heap.cast::<u8>(), bytes) }.cast::<Handler>();
            if p.is_null() {
                return Err(());
            }
            for j in self.heap_cap..cap {
                // SAFETY: the block holds `cap` entries; these are the new
                // ones, not yet initialised.
                unsafe { p.add(j).write(Handler::Done) };
            }
            self.heap = p;
            self.heap_cap = cap;
        }
        let at = self.len;
        *self.slot(at).ok_or(())? = h;
        self.len += 1;
        self.generation = self.generation.wrapping_add(1);
        Ok(())
    }

    /// Take the newest handler off the end, leaving `Done` in its slot.
    fn pop(&mut self) -> Option<Handler> {
        let at = self.len.checked_sub(1)?;
        let h = core::mem::replace(self.slot(at)?, Handler::Done);
        self.len = at;
        Some(h)
    }
}

/// Which list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Which {
    /// `exit`'s.
    Exit,
    /// `quick_exit`'s.
    QuickExit,
}

process_global! {
    /// The handlers `exit` runs.  Per-thread on the host, for test
    /// isolation; the process's on the target.
    fn exit_list() -> List = List::new();

    /// The handlers `quick_exit` runs.
    fn quick_exit_list() -> List = List::new();

    /// Serialises every use of both lists.  Never held across a handler.
    fn lists_lock() -> PoolLock = PoolLock::new();
}

/// Run `f` on `which`'s list with the lock held.
fn with_list<R>(which: Which, f: impl FnOnce(&mut List) -> R) -> R {
    // SAFETY: `lists_lock()` is this context's lock, valid as long as the
    // lists it guards.
    let _guard = unsafe { lock_pool(lists_lock()) };
    let list = match which {
        Which::Exit => exit_list(),
        Which::QuickExit => quick_exit_list(),
    };
    // SAFETY: the lock is held, so this is the only reference to the list,
    // which is this context's and lives as long as it does.
    f(unsafe { &mut *list })
}

/// Register `h` on `which`'s list: 0, or -1 with `errno` `ENOMEM` when the
/// list cannot grow -- glibc's answer, `__new_exitfn` failing.
pub(crate) fn register(which: Which, h: Handler) -> i32 {
    match with_list(which, |l| l.push(h)) {
        Ok(()) => 0,
        Err(()) => {
            crate::errno::set_errno(crate::errno::ENOMEM);
            -1
        }
    }
}

/// `__run_exit_handlers`' loop: run `which`'s handlers newest first until
/// the list is empty, each called with the lock released and gone from the
/// list before it runs -- so a handler registered meanwhile runs next, and a
/// nested `exit` finishes the list rather than repeating it.
pub(crate) fn run(which: Which, status: i32) {
    while let Some(h) = with_list(which, List::pop) {
        call(h, status);
    }
}

fn call(h: Handler, status: i32) {
    match h {
        Handler::Done => {}
        Handler::At(f) => f(),
        Handler::On(f, arg) => f(status, arg),
        Handler::Cxa(f, arg, _) => f(arg),
    }
}

/// `__cxa_finalize(dso)`: run, newest first, every `__cxa_atexit` entry of
/// module `dso`, marking each done before it runs.  For a NULL `dso`, every
/// termination function -- the Itanium ABI's "call all of them", which in
/// glibc covers `atexit` handlers as well, since its `atexit` registers
/// through `__cxa_atexit` -- and the `at_quick_exit` handlers are dropped
/// uncalled, as glibc drops those of a module it finalises.  `on_exit`
/// handlers are never this call's.
///
/// When a destructor registers another, or the list shrinks under a
/// concurrent `exit`, the scan restarts from the end, as glibc's does; the
/// entries already run are `Done` by then and are not run again.
pub(crate) fn finalize(dso: *mut u8) {
    'restart: loop {
        let (mut i, seen) = with_list(Which::Exit, |l| (l.len, l.generation));
        while i > 0 {
            i -= 1;
            let next = with_list(Which::Exit, |l| {
                if l.generation != seen || i >= l.len {
                    return Err(());
                }
                let entry = l.slot(i).ok_or(())?;
                let ours = match *entry {
                    Handler::Cxa(_, _, d) => dso.is_null() || d == dso,
                    Handler::At(_) => dso.is_null(),
                    Handler::On(..) | Handler::Done => false,
                };
                Ok(ours.then(|| core::mem::replace(entry, Handler::Done)))
            });
            match next {
                Err(()) => continue 'restart,
                Ok(Some(h)) => {
                    call(h, 0);
                    // Asked at once, as glibc asks: waiting for the next
                    // entry would miss a registration made by the last one.
                    if with_list(Which::Exit, |l| l.generation) != seen {
                        continue 'restart;
                    }
                }
                Ok(None) => {}
            }
        }
        break;
    }
    if dso.is_null() {
        with_list(Which::QuickExit, |l| while l.pop().is_some() {});
    }
}

// ---------------------------------------------------------------------------
// thread_local destructors
// ---------------------------------------------------------------------------

/// One `thread_local` object's destructor: a node of the calling thread's
/// list, newest first, allocated by `malloc`.
struct ThreadDtor {
    func: CxaFn,
    obj: *mut u8,
    next: *mut ThreadDtor,
}

/// `__cxa_thread_atexit_impl(func, obj, _)`: run `func(obj)` when the calling
/// thread ends.  `Err` when the node cannot be allocated.
pub(crate) fn register_thread_dtor(func: CxaFn, obj: *mut u8) -> Result<(), ()> {
    let node = crate::malloc::malloc(size_of::<ThreadDtor>()).cast::<ThreadDtor>();
    if node.is_null() {
        return Err(());
    }
    let pt = crate::perthread::current();
    // SAFETY: `pt` is the calling thread's block, which only this thread
    // touches; `node` is a fresh allocation of a `ThreadDtor`, and `malloc`
    // returns memory aligned for any object.
    unsafe {
        node.write(ThreadDtor {
            func,
            obj,
            next: (*pt).tls_dtors.cast::<ThreadDtor>(),
        });
        (*pt).tls_dtors = node.cast::<u8>();
    }
    Ok(())
}

/// `__call_tls_dtors`: run the calling thread's `thread_local` destructors,
/// newest first, until none is left -- including any a destructor registers.
/// Each node is unlinked and freed before its destructor runs, so a
/// destructor that ends the thread (`pthread_exit` runs this again) or the
/// process leaves nothing to run twice.
pub(crate) fn run_thread_dtors() {
    loop {
        let pt = crate::perthread::current();
        // SAFETY: `pt` is the calling thread's block; `tls_dtors` is NULL or
        // the newest node `register_thread_dtor` linked, which nothing else
        // frees.
        let ThreadDtor { func, obj, .. } = unsafe {
            let head = (*pt).tls_dtors.cast::<ThreadDtor>();
            if head.is_null() {
                return;
            }
            let node = head.read();
            (*pt).tls_dtors = node.next.cast::<u8>();
            crate::malloc::free(head.cast::<u8>());
            node
        };
        func(obj);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use core::cell::RefCell;
    use std::vec::Vec;

    std::thread_local! {
        /// What ran, in order: this thread's, as the lists are.
        static LOG: RefCell<Vec<(char, usize, i32)>> = const { RefCell::new(Vec::new()) };
    }

    fn log(tag: char, v: usize, status: i32) {
        LOG.with(|l| l.borrow_mut().push((tag, v, status)));
    }

    fn take_log() -> Vec<(char, usize, i32)> {
        LOG.with(|l| core::mem::take(&mut *l.borrow_mut()))
    }

    fn cxa(v: usize, dso: usize) -> Handler {
        Handler::Cxa(dtor, v as *mut u8, dso as *mut u8)
    }

    extern "C" fn at_one() {
        log('a', 1, 0);
    }
    extern "C" fn at_two() {
        log('a', 2, 0);
    }
    extern "C" fn on(status: i32, arg: *mut u8) {
        log('o', arg.addr(), status);
    }
    extern "C" fn dtor(obj: *mut u8) {
        log('c', obj.addr(), 0);
    }
    extern "C" fn registers_another() {
        log('r', 0, 0);
        assert_eq!(register(Which::Exit, Handler::At(at_two)), 0);
    }
    extern "C" fn dtor_registers_another(obj: *mut u8) {
        log('d', obj.addr(), 0);
        assert_eq!(register(Which::Exit, cxa(77, 0x5000)), 0);
    }
    extern "C" fn thread_dtor(obj: *mut u8) {
        log('t', obj.addr(), 0);
    }
    extern "C" fn thread_dtor_registers_another(obj: *mut u8) {
        log('u', obj.addr(), 0);
        register_thread_dtor(thread_dtor, 99 as *mut u8).unwrap();
    }

    #[test]
    fn a_destructor_gets_its_object() {
        // THE REGRESSION PIN: the object was dropped, so a C++ static
        // destructor ran on whatever `rdi` held -- NULL, for CMake.
        assert_eq!(register(Which::Exit, cxa(0xC0DE, 0)), 0);
        run(Which::Exit, 0);
        assert_eq!(take_log(), vec![('c', 0xC0DE, 0)]);
    }

    #[test]
    fn every_kind_runs_newest_first_with_its_arguments() {
        let h = [
            Handler::At(at_one),
            Handler::On(on, 5 as *mut u8),
            cxa(9, 0),
            Handler::At(at_two),
        ];
        for x in h {
            assert_eq!(register(Which::Exit, x), 0);
        }
        run(Which::Exit, 3);
        assert_eq!(
            take_log(),
            vec![('a', 2, 0), ('c', 9, 0), ('o', 5, 3), ('a', 1, 0)],
            "on_exit's handler is called at last, with the status"
        );
        run(Which::Exit, 3);
        assert!(take_log().is_empty(), "each runs once");
    }

    #[test]
    fn the_list_is_not_limited_to_32() {
        for i in 0..1000 {
            assert_eq!(register(Which::Exit, cxa(i, 0)), 0, "entry {i}");
        }
        run(Which::Exit, 0);
        let ran: Vec<usize> = take_log().into_iter().map(|(_, v, _)| v).collect();
        assert_eq!(ran, (0..1000).rev().collect::<Vec<_>>());
    }

    #[test]
    fn a_handler_registered_during_exit_runs_next() {
        assert_eq!(register(Which::Exit, Handler::At(at_one)), 0);
        assert_eq!(register(Which::Exit, Handler::At(registers_another)), 0);
        run(Which::Exit, 0);
        assert_eq!(take_log(), vec![('r', 0, 0), ('a', 2, 0), ('a', 1, 0)]);
    }

    #[test]
    fn finalize_runs_one_modules_destructors_once() {
        let h = [
            cxa(1, 0x1000),
            Handler::At(at_one),
            cxa(2, 0x2000),
            cxa(3, 0x1000),
        ];
        for x in h {
            assert_eq!(register(Which::Exit, x), 0);
        }
        finalize(0x1000 as *mut u8);
        assert_eq!(take_log(), vec![('c', 3, 0), ('c', 1, 0)]);
        finalize(0x1000 as *mut u8);
        assert!(take_log().is_empty(), "nothing twice");
        // exit runs what is left: the other module's and the atexit one.
        run(Which::Exit, 0);
        assert_eq!(take_log(), vec![('c', 2, 0), ('a', 1, 0)]);
    }

    #[test]
    fn finalize_null_runs_every_termination_function_but_not_on_exits() {
        let h = [
            cxa(1, 0x1000),
            Handler::At(at_one),
            Handler::On(on, 4 as *mut u8),
            cxa(2, 0x2000),
        ];
        for x in h {
            assert_eq!(register(Which::Exit, x), 0);
        }
        assert_eq!(register(Which::QuickExit, Handler::At(at_two)), 0);
        finalize(core::ptr::null_mut());
        assert_eq!(take_log(), vec![('c', 2, 0), ('a', 1, 0), ('c', 1, 0)]);
        run(Which::Exit, 6);
        assert_eq!(take_log(), vec![('o', 4, 6)], "on_exit's is exit's alone");
        run(Which::QuickExit, 0);
        assert!(take_log().is_empty(), "quick_exit's were dropped, uncalled");
    }

    #[test]
    fn finalize_restarts_when_a_destructor_registers_another() {
        let h = Handler::Cxa(dtor_registers_another, 1 as *mut u8, 0x5000 as *mut u8);
        assert_eq!(register(Which::Exit, h), 0);
        finalize(0x5000 as *mut u8);
        assert_eq!(take_log(), vec![('d', 1, 0), ('c', 77, 0)]);
    }

    #[test]
    fn finalised_entries_on_top_are_reused() {
        assert_eq!(register(Which::Exit, Handler::At(at_one)), 0);
        for round in 0..100 {
            for i in 0..INLINE {
                assert_eq!(register(Which::Exit, cxa(i, 0x3000)), 0);
            }
            finalize(0x3000 as *mut u8);
            assert_eq!(take_log().len(), INLINE, "round {round}");
        }
        let (len, heap_cap) = with_list(Which::Exit, |l| (l.len, l.heap_cap));
        assert!(
            len <= INLINE + 1 && heap_cap <= INLINE,
            "len {len}, heap {heap_cap}: finalised entries piled up"
        );
        run(Which::Exit, 0);
        assert_eq!(take_log(), vec![('a', 1, 0)]);
    }

    #[test]
    fn quick_exit_has_its_own_list() {
        assert_eq!(register(Which::QuickExit, Handler::At(at_one)), 0);
        assert_eq!(register(Which::Exit, Handler::At(at_two)), 0);
        run(Which::QuickExit, 0);
        assert_eq!(take_log(), vec![('a', 1, 0)]);
        run(Which::Exit, 0);
        assert_eq!(take_log(), vec![('a', 2, 0)]);
    }

    #[test]
    fn thread_destructors_run_newest_first_with_their_objects() {
        let before = crate::malloc::live_allocations::count();
        register_thread_dtor(thread_dtor, 1 as *mut u8).unwrap();
        register_thread_dtor(thread_dtor, 2 as *mut u8).unwrap();
        register_thread_dtor(thread_dtor_registers_another, 3 as *mut u8).unwrap();
        run_thread_dtors();
        assert_eq!(
            take_log(),
            vec![('u', 3, 0), ('t', 99, 0), ('t', 2, 0), ('t', 1, 0)],
            "one registered while running runs next"
        );
        assert_eq!(
            crate::malloc::live_allocations::count(),
            before,
            "every node is freed"
        );
        run_thread_dtors();
        assert!(take_log().is_empty(), "each runs once");
    }

    #[test]
    fn thread_destructors_are_the_registering_threads() {
        register_thread_dtor(thread_dtor, 5 as *mut u8).unwrap();
        let other = std::thread::spawn(|| {
            run_thread_dtors();
            take_log()
        })
        .join()
        .unwrap();
        assert!(other.is_empty(), "another thread ran this one's");
        run_thread_dtors();
        assert_eq!(take_log(), vec![('t', 5, 0)]);
    }
}
