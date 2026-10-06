//! The program's clipboard: text or a picture cut or copied in any of its
//! fields, for a paste in any other.
//!
//! Until 2026-09-30 each text field kept a clipboard of its own
//! ([`TextInput`](crate::textinput::TextInput),
//! [`TextArea`](crate::textarea::TextArea)), so text copied in one field of a
//! window could not be pasted into the field beside it; a window that wanted
//! that carried the text from field to field itself, as `apps/email` does.
//! The fields share this one now.
//!
//! **Text and a picture.** One copy can offer both, as a copy does on every
//! desktop -- a rich field's copy of a picture puts the picture here, and the
//! text around it -- and a paste takes what its field can use: a text field
//! the text, a rich field the picture where there is no text
//! ([`crate::richinput`]). What a field keeps beside the clipboard, as a rich
//! field keeps the formatting of its own copy, it ties to the copy by
//! [`generation`], which every copy changes: the formatting is brought back
//! only while what it was copied with is still what is here.
//!
//! **Only this program's.** Copying between programs needs the system's
//! clipboard, which waits on how programs are to reach it
//! (`open-questions.md` C-Q29, `known-issues.md`
//! `TD-C-FIFTEEN-PRIVATE-CLIPBOARDS-AND-A-SERVICE-NOBODY-TALKS-TO`; lane E has
//! asked lane A for a pair of syscalls into the kernel's own clipboard,
//! `requests/e-a-a-clipboard-door-for-applications.md`). When that is settled
//! this is the one place to connect it: every field in the toolkit already
//! copies and pastes through here, a picture going out as a PNG file
//! ([`Picture::to_png`]) and coming in as any file `imagecodec` reads.
//!
//! **Per thread, which for a program is the same thing.** A program's
//! windows are driven from the one thread that runs its event loop, so that
//! thread's clipboard is the program's. The difference shows only in tests,
//! which the harness runs on threads of their own -- and there it is the
//! point: one test's copy cannot turn up in another test's paste.

use std::cell::RefCell;

use crate::picture::Picture;

/// What is on the clipboard.
#[derive(Debug)]
struct Clip {
    /// The copy's text; empty where it has none.
    text: String,
    /// The copy's picture, where it has one.
    picture: Option<Picture>,
    /// How many copies there have been: what a field keeping more beside a
    /// copy checks it against.
    generation: u64,
}

thread_local! {
    static CLIP: RefCell<Clip> = const {
        RefCell::new(Clip {
            text: String::new(),
            picture: None,
            generation: 0,
        })
    };
}

/// Put a copy on the clipboard -- its text, its picture, or both -- in place
/// of what was there.
pub fn set(text: &str, picture: Option<Picture>) {
    CLIP.with(|clip| {
        // Nothing borrows the clipboard across a call out of this module, so
        // the borrow cannot be taken already; if it somehow were, keeping what
        // is there is the answer that loses nothing the user can see twice.
        if let Ok(mut clip) = clip.try_borrow_mut() {
            clip.text.clear();
            clip.text.push_str(text);
            clip.picture = picture;
            clip.generation = clip.generation.wrapping_add(1);
        }
    });
}

/// Put `text` on the clipboard, in place of what was there.
pub fn set_text(text: &str) {
    set(text, None);
}

/// Put `picture` on the clipboard, alone, in place of what was there.
pub fn set_picture(picture: Picture) {
    set("", Some(picture));
}

/// The text on the clipboard: empty when what was copied has none.
#[must_use]
pub fn text() -> String {
    CLIP.with(|clip| {
        clip.try_borrow()
            .map(|c| c.text.clone())
            .unwrap_or_default()
    })
}

/// The picture on the clipboard, if what was copied has one.
#[must_use]
pub fn picture() -> Option<Picture> {
    CLIP.with(|clip| clip.try_borrow().ok().and_then(|c| c.picture.clone()))
}

/// Whether there is text on the clipboard.
#[must_use]
pub fn has_text() -> bool {
    CLIP.with(|clip| clip.try_borrow().is_ok_and(|c| !c.text.is_empty()))
}

/// Whether there is a picture on the clipboard.
#[must_use]
pub fn has_picture() -> bool {
    CLIP.with(|clip| clip.try_borrow().is_ok_and(|c| c.picture.is_some()))
}

/// Whether nothing is on the clipboard: no text and no picture.
#[must_use]
pub fn is_empty() -> bool {
    !has_text() && !has_picture()
}

/// A number every copy changes: what a field that keeps more beside a copy
/// -- a rich field, its formatting -- keeps with it, and checks is still
/// this before a paste uses it.
#[must_use]
pub fn generation() -> u64 {
    CLIP.with(|clip| clip.try_borrow().map_or(0, |c| c.generation))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn picture() -> Picture {
        Picture::new(imagecodec::Image {
            width: 1,
            height: 1,
            pixels: vec![0xFF00_FF00],
        })
        .expect("a picture")
    }

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

    /// **A copy can carry a picture, alone or with text, and the next copy
    /// takes both away.**
    #[test]
    fn a_copy_carries_a_picture() {
        let shot = picture();
        set_picture(shot.clone());
        assert!(!is_empty(), "a picture alone is something");
        assert!(!has_text());
        assert_eq!(super::picture(), Some(shot.clone()));
        set("around it", Some(shot.clone()));
        assert_eq!(text(), "around it");
        assert_eq!(super::picture(), Some(shot));
        set_text("words");
        assert!(!has_picture(), "a copy of text alone took the picture away");
        assert!(has_text());
    }

    /// **Every copy changes the generation**, the same text copied again
    /// included: what a field kept beside the last copy is not taken for the
    /// next one's.
    #[test]
    fn every_copy_is_a_generation() {
        let before = generation();
        set_text("same");
        let once = generation();
        set_text("same");
        let twice = generation();
        assert_ne!(once, before);
        assert_ne!(twice, once, "the same text, copied again, is a new copy");
        assert_eq!(generation(), twice, "reading changes nothing");
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
