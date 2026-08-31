//! gnulib's `yesno` — the answer to a `? ` prompt.
//!
//! Upstream has exactly one of these (`lib/yesno.c`) and every utility that
//! stops to ask a question calls it: `rm -i`, `cp -i`, `mv -i`, `ln -i`,
//! `install`, and — with its own wrapper but the same rule — `find -ok`. That
//! is the argument for this module. The question is worded differently by each
//! of them, but *what counts as yes* is one rule, and a utility that answered
//! it differently from its neighbour would be a trap: a person who has learned
//! that `rm -i` takes `yes` would reasonably type `yes` at `cp -i` too.
//!
//! Two private copies existed before this module and had already drifted:
//!
//! * `rm`'s read the line with `read_until(b'\n')`, as bytes.
//! * `find -ok`'s read it with `read_line` into a `String`, which **fails on
//!   input that is not UTF-8** and returns "no" for it. A terminal in a
//!   single-byte locale sends exactly that, so `y` typed at `find -ok` after a
//!   stray high byte was declined where the same key at `rm -i` was accepted.
//!
//! # The rule
//!
//! gnulib reads one line with `getline` and passes it to `rpmatch`, which under
//! the C locale is the regular expression `^[yY]`. So:
//!
//! * The **first byte** decides, and nothing else does. `yes`, `yeah` and
//!   `yellow` are all yes; `Y` is yes; `ye` typed with no newline before end of
//!   input is yes.
//! * A leading space is not a yes — `rpmatch` anchors, and does not skip
//!   whitespace. Measured: `" y"` declines.
//! * An empty line is no, and so is end of input. gnulib treats a `getline`
//!   that returns `<= 0` as no without distinguishing the two, which is why
//!   [`Answers::line`] returns one `None` for both.
//! * There is **no re-prompt and no third answer.** Anything that is not a yes
//!   is a no; the caller does not ask again.
//!
//! Note what is *not* here: a locale. Upstream's `rpmatch` consults
//! `LC_MESSAGES`, so a French `oui` is a yes under `fr_FR`. This tree has one
//! locale, and the C one's `yesexpr` is what is implemented; when locales
//! arrive, this module is the one place that has to learn about them, which is
//! the other half of the reason it is a module.
//!
//! # Why a trait rather than a function that reads stdin
//!
//! A prompt is the part of these utilities that most needs testing and is the
//! least testable through a real terminal — the interesting cases are a queue
//! of several answers consumed by several prompts, and an input that ends part
//! way through. So the source of lines is a trait: the shipped implementation
//! is [`StdinAnswers`] and a test hands over [`Canned`].
//!
//! [`Canned`] is deliberately not `#[cfg(test)]`. It is used by the tests of
//! the *binaries*, which are separate compilation units from this library, and
//! a `cfg(test)` item here would be invisible to every one of them.

use std::io::{self, BufRead};

/// Where a prompt's answer comes from.
///
/// One line per call, newline included, or `None` at end of input.
pub trait Answers {
    /// The next line of input, with its trailing newline if it had one, or
    /// `None` at end of input or on a read that failed.
    ///
    /// The two are one value on purpose: gnulib's `yesno` cannot tell them
    /// apart either — `getline` returns `-1` for both — and treats each as a
    /// decline. A caller that distinguished them would be inventing a
    /// behaviour upstream does not have.
    fn line(&mut self) -> Option<Vec<u8>>;
}

/// The shipped source: standard input, held open across the whole run.
///
/// One value rather than a fresh `io::stdin()` per prompt, because several
/// prompts consume several lines of *one* stream — `rm -i a b c` with `y\ny\nn`
/// on stdin removes two files. Rust's `Stdin` is a handle to one shared
/// buffered reader, so this would in fact work either way; holding it is how
/// the code says that it means to.
pub struct StdinAnswers {
    stdin: io::Stdin,
}

impl StdinAnswers {
    #[must_use]
    pub fn new() -> Self {
        StdinAnswers { stdin: io::stdin() }
    }
}

impl Default for StdinAnswers {
    fn default() -> Self {
        Self::new()
    }
}

impl Answers for StdinAnswers {
    fn line(&mut self) -> Option<Vec<u8>> {
        let mut buf: Vec<u8> = Vec::new();
        // `read_until`, not `read_line`: an answer is bytes from a terminal,
        // not necessarily UTF-8, and `read_line` would fail the whole read on a
        // stray high byte and report end of input. That is the bug `find -ok`
        // had. See the module docs.
        match self.stdin.lock().read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(buf),
        }
    }
}

/// A canned queue of answers, for the tests of the utilities that prompt.
///
/// Answers are consumed in order and end-of-queue is end of input, so a test
/// that supplies two lines to three prompts declines the third — which is the
/// behaviour a script piping a short file into `cp -i` gets, and is worth being
/// able to write a test for.
pub struct Canned {
    lines: Vec<Vec<u8>>,
    at: usize,
}

impl Canned {
    #[must_use]
    pub fn new(lines: &[&str]) -> Self {
        Canned {
            lines: lines.iter().map(|l| l.as_bytes().to_vec()).collect(),
            at: 0,
        }
    }

    /// How many answers have been asked for. A test that means to assert *no*
    /// prompt happened asserts on this rather than on the transcript, because
    /// a prompt that was written but whose text changed would still be caught.
    #[must_use]
    pub fn consumed(&self) -> usize {
        self.at
    }
}

impl Answers for Canned {
    fn line(&mut self) -> Option<Vec<u8>> {
        let line = self.lines.get(self.at).cloned();
        self.at = self.at.saturating_add(1);
        line
    }
}

/// Whether a line read from the answer source is a yes. `^[yY]` under the C
/// locale; see the module docs for what that does and does not accept.
#[must_use]
pub fn is_yes(line: Option<&[u8]>) -> bool {
    matches!(line.and_then(<[u8]>::first), Some(b'y' | b'Y'))
}

/// gnulib's `yesno()`: read one line and say whether it was a yes.
///
/// The prompt itself is the caller's — upstream writes it with `fprintf
/// (stderr, …)` before calling `yesno`, and the wording differs per utility.
/// What every caller must do, and what this function cannot do for it, is
/// **flush** that prompt first: it ends in `? ` with no newline, so a buffered
/// stderr would leave the user staring at a blank line waiting for input.
pub fn yesno(answers: &mut dyn Answers) -> bool {
    is_yes(answers.line().as_deref())
}

#[cfg(test)]
mod tests {
    use super::{Answers, Canned, is_yes, yesno};

    /// The `^[yY]` rule, in both directions. Every case here is measured
    /// against coreutils 9.4 through `rm -i`.
    #[test]
    fn what_counts_as_yes() {
        assert!(is_yes(Some(b"y\n")));
        assert!(is_yes(Some(b"Y\n")));
        assert!(is_yes(Some(b"yes\n")));
        assert!(is_yes(Some(b"YES\n")), "only the first byte is looked at");
        assert!(is_yes(Some(b"yeah, whatever")), "and no anchor at the end");
        assert!(
            is_yes(Some(b"y")),
            "end of input without a newline is still y"
        );
        assert!(!is_yes(Some(b"n\n")));
        assert!(!is_yes(Some(b"nope\n")));
        assert!(!is_yes(Some(b"maybe\n")));
        assert!(
            !is_yes(Some(b" y\n")),
            "rpmatch anchors: a leading space is no"
        );
        assert!(!is_yes(Some(b"\n")), "an empty line is no");
        assert!(!is_yes(Some(b"")));
        assert!(!is_yes(None), "end of input is no");
    }

    /// A byte that is not valid UTF-8 in front of the answer does not stop the
    /// answer being read — this is the `find -ok` divergence the module exists
    /// to remove, pinned so it cannot come back.
    #[test]
    fn an_answer_that_is_not_utf8_is_still_an_answer() {
        assert!(is_yes(Some(b"y\xff\n")));
        assert!(!is_yes(Some(b"\xffy\n")), "and still anchored at byte 0");
    }

    /// Answers are consumed one per call, and the queue running out is end of
    /// input rather than a repeat of the last answer.
    #[test]
    fn the_queue_is_consumed_in_order_and_then_ends() {
        let mut canned = Canned::new(&["y", "n", "Y"]);
        assert!(yesno(&mut canned));
        assert!(!yesno(&mut canned));
        assert!(yesno(&mut canned));
        assert_eq!(canned.consumed(), 3);
        assert!(
            !yesno(&mut canned),
            "past the end is end of input, which is no"
        );
        assert!(canned.line().is_none());
    }
}
