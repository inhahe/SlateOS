//! The program's clipboard: text cut or copied in any of its fields, for a
//! paste in any other.
//!
//! Until 2026-09-30 each text field kept a clipboard of its own
//! ([`TextInput`](crate::textinput::TextInput),
//! [`TextArea`](crate::textarea::TextArea)), so text copied in one field of a
//! window could not be pasted into the field beside it; a window that wanted
//! that carried the text from field to field itself, as `apps/email` does.
//! The fields share this one now.
//!
//! **Only this program's.** Copying between programs needs the system's
//! clipboard, which waits on how programs are to reach it
//! (`open-questions.md` C-Q29, `known-issues.md`
//! `TD-C-FIFTEEN-PRIVATE-CLIPBOARDS-AND-A-SERVICE-NOBODY-TALKS-TO`; lane E has
//! asked lane A for a pair of syscalls into the kernel's own clipboard,
//! `requests/e-a-a-clipboard-door-for-applications.md`). When that is settled
//! this is the one place to connect it: every field in the toolkit already
//! copies and pastes through here.
//!
//! **Per thread, which for a program is the same thing.** A program's
//! windows are driven from the one thread that runs its event loop, so that
//! thread's clipboard is the program's. The difference shows only in tests,
//! which the harness runs on threads of their own -- and there it is the
//! point: one test's copy cannot turn up in another test's paste.

use std::cell::RefCell;

thread_local! {
    static TEXT: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Put `text` on the clipboard, in place of what was there.
pub fn set_text(text: &str) {
    TEXT.with(|clip| {
        // Nothing borrows the clipboard across a call out of this module, so
        // the borrow cannot be taken already; if it somehow were, keeping what
        // is there is the answer that loses nothing the user can see twice.
        if let Ok(mut clip) = clip.try_borrow_mut() {
            clip.clear();
            clip.push_str(text);
        }
    });
}

/// What is on the clipboard: empty when nothing has been cut or copied.
#[must_use]
pub fn text() -> String {
    TEXT.with(|clip| clip.try_borrow().map(|c| c.clone()).unwrap_or_default())
}

/// Whether nothing is on the clipboard.
#[must_use]
pub fn is_empty() -> bool {
    TEXT.with(|clip| clip.try_borrow().map_or(true, |c| c.is_empty()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// **What is put on the clipboard is what comes off it, until something
    /// else is put there.**
    #[test]
    fn what_goes_on_comes_off() {
        assert!(is_empty());
        assert_eq!(text(), "");
        set_text("hello");
        assert!(!is_empty());
        assert_eq!(text(), "hello");
        assert_eq!(text(), "hello", "reading took it off");
        set_text("bye");
        assert_eq!(text(), "bye");
    }

    /// **Another thread has a clipboard of its own**: a program's is its
    /// event loop's, and a test's is its own.
    #[test]
    fn another_thread_has_its_own() {
        set_text("here");
        let there = std::thread::spawn(|| {
            let before = text();
            set_text("there");
            before
        })
        .join()
        .expect("the other thread panicked");
        assert_eq!(there, "", "another thread saw this one's copy");
        assert_eq!(text(), "here", "another thread's copy reached this one");
    }
}
