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
//! **And the system's.** The program's clipboard is exchanged with the
//! system's -- which another program's copy reaches -- by whoever holds the
//! program's connection to it, in two moves that need nothing from the
//! fields: after a copy here, [`take_outgoing`] gives the copy's text to hand
//! over; when the program's window gains the keyboard, the system's text is
//! read and given to [`adopt_incoming`], which makes it the program's copy if
//! another program put it there. A copy can only be made in the window that
//! has the keyboard, so nothing else can change the system's clipboard while
//! this program has it, and reading it on each gain of the keyboard is all
//! the reading there is. The desktop's session exchanges its own; a program's
//! event loop exchanges a program's (`requests/c-f-carry-the-clipboard-over-the-compositor-connection.md`).
//! Today the system's clipboard carries text: a picture copied here stays
//! here, the copy's text -- empty for a picture alone -- going out as what
//! another program pastes. (Which way programs reach the system's clipboard
//! is `open-questions.md` C-Q29; this exchange is the same for any of its
//! answers.)
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
    /// The generation last exchanged with the system's clipboard -- given
    /// to it, or taken from it: a copy made since is one to hand over.
    exchanged_generation: u64,
    /// The text last exchanged with the system's clipboard: what reading it
    /// back must not take for another program's copy.
    exchanged: Option<String>,
}

thread_local! {
    static CLIP: RefCell<Clip> = const {
        RefCell::new(Clip {
            text: String::new(),
            picture: None,
            generation: 0,
            exchanged_generation: 0,
            exchanged: None,
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

/// The text of a copy made in this program since the clipboard was last
/// exchanged with the system's, to hand over to it -- once: asked again with
/// no copy between, `None`.
///
/// Empty for a picture copied alone, which the system's clipboard cannot
/// carry yet: handing over nothing is what makes another program's paste
/// find nothing, rather than the text copied before the picture.
#[must_use]
pub fn take_outgoing() -> Option<String> {
    CLIP.with(|clip| {
        let mut clip = clip.try_borrow_mut().ok()?;
        if clip.generation == clip.exchanged_generation {
            return None;
        }
        clip.exchanged_generation = clip.generation;
        let text = clip.text.clone();
        clip.exchanged = Some(text.clone());
        Some(text)
    })
}

/// The system's clipboard, read -- `None` where nothing has been copied
/// there -- taken as this program's copy if another program put it there:
/// a copy of its text, plain, as any other copy is.
///
/// What this program handed over itself, read back, changes nothing: its
/// own copy stays as it was, with the formatting and the pictures a rich
/// field keeps beside it.
pub fn adopt_incoming(text: Option<String>) {
    let Some(text) = text else {
        return;
    };
    CLIP.with(|clip| {
        let Ok(mut clip) = clip.try_borrow_mut() else {
            return;
        };
        if clip.exchanged.as_deref() == Some(text.as_str()) {
            return;
        }
        clip.text = text.clone();
        clip.picture = None;
        clip.generation = clip.generation.wrapping_add(1);
        clip.exchanged_generation = clip.generation;
        clip.exchanged = Some(text);
    });
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

    /// **A copy made here is handed over once; a copy made elsewhere is
    /// taken as this program's own**, and what was handed over, read back,
    /// changes nothing.
    #[test]
    fn the_clipboard_is_exchanged_with_the_systems() {
        set_text("mine");
        assert_eq!(take_outgoing(), Some("mine".to_string()));
        assert_eq!(take_outgoing(), None, "once");
        let ours = generation();
        adopt_incoming(Some("mine".to_string()));
        assert_eq!(generation(), ours, "our own, read back: nothing new");
        adopt_incoming(None);
        assert_eq!(generation(), ours, "nothing copied anywhere: nothing new");
        adopt_incoming(Some("theirs".to_string()));
        assert_eq!(text(), "theirs");
        assert_ne!(generation(), ours, "another program's copy is a new one");
        assert_eq!(take_outgoing(), None, "not handed back");
        let shot = picture_of_one_pixel();
        set_picture(shot);
        assert_eq!(
            take_outgoing(),
            Some(String::new()),
            "a picture alone: nothing for another program to paste"
        );
        adopt_incoming(Some("theirs".to_string()));
        assert!(
            super::picture().is_none(),
            "another program's copy, read after ours, takes the picture away"
        );
    }

    fn picture_of_one_pixel() -> Picture {
        picture()
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
