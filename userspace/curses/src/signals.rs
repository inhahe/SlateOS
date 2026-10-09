//! The process's screen, and the signals curses handles for it -- what
//! `initscr` implies (`SP`, `CURRENT_SCREEN`) and `lib_tstp.c`: `SIGTSTP`
//! suspends the program cleanly, `SIGINT` and `SIGTERM` put the terminal
//! back before the program dies, `SIGWINCH` is noted for the next refresh.
//! As upstream, each is installed only where the program left the signal's
//! default action (`CatchIfDefault`).
//!
//! # A handler never interrupts curses
//!
//! Upstream's handlers call `endwin` and `doupdate` from inside the handler,
//! whatever the program was doing -- including when it was in the middle of
//! a curses call, half way through changing the very structures the handler
//! then reads. Here the screen is held by whoever is using it, a flag taken
//! with an atomic swap ([`with`]); a handler that finds it held does not
//! touch it, but leaves its action pending, and the holder does it as it
//! lets go -- the interrupted call finishes, then the program suspends or
//! ends. A handler that finds it free does its work at once, as upstream's
//! does.
//!
//! What is still upstream's is the rest of that: a handler that runs while
//! the program is outside curses does its work at once -- `endwin`, and on
//! the way back from a suspension the repaint.
//!
//! # And allocates nothing
//!
//! Upstream accepts the hazard of that work in so many words ("Much of this
//! is unsafe from a signal handler. But we'll _try_ to clean up the screen
//! and terminal settings on the way out."): it allocates, and a handler that
//! interrupted the program inside the allocator would wait on its lock for
//! good. Here, once the screen has been drawn, it allocates and frees
//! nothing: the output buffer is reserved whole when the screen is made,
//! capabilities are written straight from the terminal's description,
//! `tparm` expands into a buffer it keeps, the cursor optimiser builds its
//! moves in fixed buffers, the update borrows the lines it compares, and
//! `wcrtomb` writes into the caller's bytes. A test counts the allocator's
//! calls through all of it. What `exit` then does -- `atexit` handlers, the
//! standard streams flushed -- is the C library's and Rust's, as upstream's
//! is the C library's.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

use libcall::signal::{self as sig, Disposition, Handler, SigSet};

use crate::screen::{EndWin, Installed, Screen, Tstp};

/// The screen, and whether someone holds it.
struct Global {
    /// Held: a `&mut` to [`Global::screen`] exists.
    busy: AtomicBool,
    /// `initscr`'s screen, `None` before it.
    screen: UnsafeCell<Option<Screen>>,
}

// SAFETY: `screen` is reached only between taking `busy` -- an atomic swap
// from false to true, which only one taker can win -- and giving it back, so
// at most one `&mut` to it exists at a time, whichever thread or signal
// handler holds it. What crosses threads that way is a `Screen`, which is
// `Send` (checked below).
unsafe impl Sync for Global {}

/// `Screen` must be `Send` for [`Global`]'s `Sync` to be sound.
const _: fn() = || {
    fn check<T: Send>() {}
    check::<Screen>();
};

static GLOBAL: Global = Global {
    busy: AtomicBool::new(false),
    screen: UnsafeCell::new(None),
};

/// Actions handlers left for the holder of the screen.
static PENDING: AtomicU32 = AtomicU32::new(0);
/// `handle_SIGINT`'s: `endwin`, then `_exit (1)`.
const INTERRUPT: u32 = 1;
/// [`end_and_exit`]'s: `endwin`, then `exit` with [`EXIT_STATUS`].
const EXIT: u32 = 2;
/// `handle_SIGTSTP`'s: suspend, and come back.
const SUSPEND: u32 = 4;

/// The status [`end_and_exit`] was asked for.
static EXIT_STATUS: AtomicI32 = AtomicI32::new(0);
/// `_nc_globals.have_sigwinch`.
static HAVE_SIGWINCH: AtomicBool = AtomicBool::new(false);
/// `_nc_globals.cleanup_nested`: a cleanup is under way.
static CLEANUP_NESTED: AtomicU32 = AtomicU32::new(0);
/// `_nc_globals.init_signals`: `SIGINT`, `SIGTERM` and `SIGWINCH` have been
/// looked at, which happens once.
static INIT_SIGNALS: AtomicBool = AtomicBool::new(false);

/// Take the screen, if nobody holds it.
fn acquire() -> bool {
    !GLOBAL.busy.swap(true, Ordering::Acquire)
}

/// The screen, while it is held.
///
/// # Safety
///
/// The caller holds it: [`acquire`] returned true, and it has not been let
/// go since -- so no other reference to it exists -- and the reference is
/// gone before it is.
unsafe fn held() -> &'static mut Option<Screen> {
    // SAFETY: the caller's contract: the flag makes this the only reference.
    unsafe { &mut *GLOBAL.screen.get() }
}

/// Let go of the screen -- and first do whatever handlers left pending
/// while it was held, holding it again for each.
fn release() {
    loop {
        GLOBAL.busy.store(false, Ordering::Release);
        if PENDING.load(Ordering::Acquire) == 0 {
            return;
        }
        // Someone else took it in between: they will see what is pending as
        // they let go.
        if !acquire() {
            return;
        }
        let pending = PENDING.swap(0, Ordering::AcqRel);
        // SAFETY: just acquired; the reference ends with `perform`.
        perform(unsafe { held() }, pending);
    }
}

/// Lets go of the screen if a closure holding it unwinds, so that a panic
/// does not leave it held for good.
struct Unwinding;

impl Drop for Unwinding {
    fn drop(&mut self) {
        GLOBAL.busy.store(false, Ordering::Release);
    }
}

/// `f` with the screen's slot -- `None` before `initscr` -- or `None` when
/// the screen is already held: by a call this one interrupted (a program's
/// own signal handler calling curses), or by another thread.
pub fn with_slot<R>(f: impl FnOnce(&mut Option<Screen>) -> R) -> Option<R> {
    if !acquire() {
        return None;
    }
    let guard = Unwinding;
    // SAFETY: just acquired; the reference ends with `f`.
    let r = f(unsafe { held() });
    core::mem::forget(guard);
    release();
    Some(r)
}

/// `f` with the screen; `None` before `initscr`, or while it is held (see
/// [`with_slot`]).
pub fn with<R>(f: impl FnOnce(&mut Screen) -> R) -> Option<R> {
    with_slot(|slot| slot.as_mut().map(f)).flatten()
}

/// Leave `action` for whoever holds the screen, then try to take it: if
/// nobody held it after all, letting go does the action now.
fn post(action: u32) {
    PENDING.fetch_or(action, Ordering::AcqRel);
    if acquire() {
        release();
    }
}

/// The actions `pending` names, done with the screen held.
fn perform(slot: &mut Option<Screen>, pending: u32) {
    if pending & INTERRUPT != 0 {
        if let Some(sp) = slot.as_mut() {
            sp.endwin();
            // "in case of reuse"
            sp.endwin = EndWin::Initial;
        }
        exit_now(1);
    }
    if pending & EXIT != 0 {
        if let Some(sp) = slot.as_mut() {
            sp.endwin();
        }
        std::process::exit(EXIT_STATUS.load(Ordering::Acquire));
    }
    if pending & SUSPEND != 0 {
        suspend(slot);
    }
}

/// `_exit (status)`: out at once, with nothing flushed.
fn exit_now(status: i32) -> ! {
    #[cfg(unix)]
    {
        libcall::process::exit_immediately(status)
    }
    #[cfg(not(unix))]
    {
        // No handler is ever installed off Unix, so nothing gets here.
        std::process::exit(status)
    }
}

/// For a program's own signal handler: `endwin`, then `exit (status)` --
/// what `watch`'s handler for `SIGINT`, `SIGTERM` and `SIGHUP` does. Safe to
/// call from a handler: when the screen is held by the call the signal
/// interrupted, this returns at once and the call ends the program as it
/// finishes. Called from anywhere else it does not return.
pub fn end_and_exit(status: i32) {
    EXIT_STATUS.store(status, Ordering::Release);
    post(EXIT);
}

/// `_nc_handle_sigwinch`'s half that reads the flag: whether a `SIGWINCH`
/// came since the last look.
pub fn take_sigwinch() -> bool {
    HAVE_SIGWINCH.swap(false, Ordering::AcqRel)
}

/// `handle_SIGWINCH`: noted, for the next `doupdate`.
extern "C" fn handle_sigwinch(_sig: i32) {
    HAVE_SIGWINCH.store(true, Ordering::Release);
}

/// `handle_SIGINT`, for `SIGINT` and `SIGTERM`: the terminal put back and
/// the program ended, with status 1. A second such signal while the first
/// is being handled ends the program at once.
extern "C" fn handle_sigint(signal: i32) {
    let _errno = sig::ErrnoGuard::save();
    if CLEANUP_NESTED.fetch_add(1, Ordering::AcqRel) == 0
        && (signal == sig::SIGINT || signal == sig::SIGTERM)
        && sig::ignore(signal).is_ok()
    {
        post(INTERRUPT);
        // Still here: the screen is held by the call this interrupted, which
        // cleans up and ends the program as it finishes.
        return;
    }
    exit_now(1);
}

/// `handle_SIGTSTP`.
extern "C" fn handle_sigtstp(_sig: i32) {
    let _errno = sig::ErrnoGuard::save();
    post(SUSPEND);
}

/// `handle_SIGTSTP`'s work: the program's modes kept if it is in curses and
/// in the foreground, the screen ended, the process stopped as `SIGTSTP`
/// would have stopped it -- and on its return, typed-ahead input thrown
/// away, the shell's modes (which the user may have changed meanwhile)
/// kept, and the screen repainted. Timer and window-size signals wait
/// meanwhile.
fn suspend(slot: &mut Option<Screen>) {
    // "Don't do this if we're not in curses", nor if "an interactive shell
    // may already have taken ownership of the tty".
    if let Some(sp) = slot.as_mut()
        && sp.endwin == EndWin::Running
        && matches!(
            (libcall::termios::foreground_group(0), libcall::process::process_group()),
            (Ok(fg), Ok(own)) if fg == own
        )
    {
        sp.def_prog_mode();
    }
    // "Block window change and timer signals."
    let mut mask = SigSet::empty();
    // Both are signals; neither addition can be refused.
    let _ = mask.add(sig::SIGALRM);
    let _ = mask.add(sig::SIGWINCH);
    // Upstream does not look at what these report; a failure leaves the
    // signals as they were, which is the most that could be done anyway.
    let omask = sig::block(&mask).unwrap_or_else(|_| SigSet::empty());
    let sigttou_blocked = omask.contains(sig::SIGTTOU);
    if !sigttou_blocked {
        let mut ttou = SigSet::empty();
        let _ = ttou.add(sig::SIGTTOU);
        let _ = sig::block(&ttou);
    }
    // "End window mode, which also resets the terminal state to the
    // original (pre-curses) modes."
    if let Some(sp) = slot.as_mut() {
        sp.endwin();
    }
    // "Unblock SIGTSTP", and `SIGTTOU` if it was not blocked to begin with.
    let mut unblock = SigSet::empty();
    let _ = unblock.add(sig::SIGTSTP);
    if !sigttou_blocked {
        let _ = unblock.add(sig::SIGTTOU);
    }
    let _ = sig::unblock(&unblock);
    // "Now we want to resend SIGSTP to this process and suspend it".
    let saved = sig::save_action(sig::SIGTSTP);
    let _ = sig::set_default(sig::SIGTSTP);
    if let Ok(pid) = i32::try_from(std::process::id()) {
        let _ = libcall::kill(pid, sig::SIGTSTP);
    }
    // "Process gets suspended...time passes...process resumes"
    if let Ok(saved) = saved {
        let _ = sig::restore_action(sig::SIGTSTP, &saved);
    }
    if let Some(sp) = slot.as_mut() {
        sp.flushinp();
        // "If the user modified the tty state while suspended, he wants
        // those changes to stick."
        sp.def_shell_mode();
        // "This relies on the fact that doupdate() will restore the
        // program-mode tty state, and issue enter_ca_mode if need be."
        sp.doupdate();
    }
    // "Reset the signals."
    let _ = sig::set_mask(&omask);
}

/// `CatchIfDefault (sig, handler)`: `handler` installed for `signal` if its
/// action is still the default -- or is `handler` already, or for
/// `SIGWINCH`, is to be ignored -- restarting interrupted calls except for
/// `SIGWINCH`, which is meant to interrupt them.
fn catch_if_default(signal: i32, handler: Handler) -> bool {
    let takes = match sig::disposition(signal) {
        Ok(Disposition::Default) => true,
        Ok(Disposition::Handler(h)) => h == handler as usize,
        Ok(Disposition::Ignore) => signal == sig::SIGWINCH,
        Err(_) => false,
    };
    takes && sig::set_handler(signal, handler, signal != sig::SIGWINCH).is_ok()
}

/// `_nc_signal_handler`'s `SIGTSTP` half: on `enable == false`, the
/// signal ignored for the length of an update; on `true`, what that
/// replaced put back -- or, the first time, the suspending handler
/// installed if the program left `SIGTSTP` at its default, and the signal
/// left alone for good if not.
pub fn tstp(state: &mut Tstp, enable: bool) {
    if state.ignore {
        return;
    }
    if !enable {
        state.installed = Installed::Ignore;
        state.replaced = sig::ignore(sig::SIGTSTP).ok();
    } else if state.installed != Installed::Nothing {
        if let Some(saved) = state.replaced {
            // The action `ignore` read back; putting it back can only fail
            // for a number that is no signal.
            let _ = sig::restore_action(sig::SIGTSTP, &saved);
        }
    } else if matches!(sig::disposition(sig::SIGTSTP), Ok(Disposition::Default)) {
        if sig::set_handler(sig::SIGTSTP, handle_sigtstp, true).is_ok() {
            state.installed = Installed::Handler;
        }
    } else {
        state.ignore = true;
    }
}

/// `_nc_signal_handler (TRUE)` as `newterm` calls it: `SIGTSTP` as
/// [`tstp`] does it, and -- once per process -- `SIGINT`, `SIGTERM` and
/// `SIGWINCH` caught where the program left them alone.
pub fn install(state: &mut Tstp) {
    tstp(state, true);
    if !INIT_SIGNALS.load(Ordering::Acquire) {
        catch_if_default(sig::SIGINT, handle_sigint);
        catch_if_default(sig::SIGTERM, handle_sigint);
        catch_if_default(sig::SIGWINCH, handle_sigwinch);
        INIT_SIGNALS.store(true, Ordering::Release);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    // One test touches the process-wide screen, so no two can race on it.
    #[test]
    fn the_screen_is_held_by_one_caller_at_a_time() {
        // Nothing set up: there is a slot, empty.
        assert_eq!(with_slot(|s| s.is_none()), Some(true));
        assert!(with(|_| ()).is_none());
        // Held, it cannot be taken again -- as by a handler interrupting.
        let nested = with_slot(|_| with_slot(|_| ()));
        assert_eq!(nested, Some(None));
        // And it is let go afterwards.
        assert_eq!(with_slot(|_| 1), Some(1));
        // A panic inside does not leave it held.
        let caught = std::panic::catch_unwind(|| {
            with_slot(|_| panic!("inside"));
        });
        assert!(caught.is_err());
        assert_eq!(with_slot(|_| 2), Some(2));
    }

    #[test]
    fn a_window_change_is_taken_once() {
        handle_sigwinch(sig::SIGWINCH);
        assert!(take_sigwinch());
        assert!(!take_sigwinch());
    }
}
