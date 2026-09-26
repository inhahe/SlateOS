//! The question a document application asks before unsaved work is lost.
//!
//! "This has changes that are not saved -- save them?", answered Save,
//! Don't save or Cancel. Every editor in `apps/` needs it at the same moments:
//! when its window is asked to close, and before anything replaces or drops a
//! document -- opening another in a program that holds one, closing a tab in
//! one that holds several. The window library lets an application decline a
//! close for exactly this (`oswindow::app::Response::KeepOpen`).
//!
//! # Why one crate
//!
//! The first five applications to ask each drew the question by hand: five
//! cards, five sets of button rectangles, five key maps. Beside them sat a
//! toolkit dialog that already does all of it -- `guitk::modal::AlertDialog`,
//! with keyboard focus, hover, Escape, a scrim, and the destructive colour for
//! the one button that loses work. A question the user meets in every editor
//! should be the same question in every editor; this is it.
//!
//! # What stays with the application
//!
//! What the question holds up -- the `pending` value: a tab, the window, an
//! Open -- and what each answer does. Saving is the application's own, and so
//! is where to put a document that has no file yet.
//!
//! # Keys
//!
//! S (or Enter, since Save has the focus) saves, D does not, Escape cancels;
//! Tab moves between the buttons as in any toolkit dialog. Every other key is
//! swallowed: a keystroke that reached the document under the question would be
//! a change made while being asked about the changes.

use guitk::event::{Event, Key};
use guitk::modal::{AlertDialog, ButtonRole, ButtonSet, DialogButton, DialogResult};
use guitk::palette::Palette;
use guitk::render::{RenderCommand, RenderTree};

/// How long, in milliseconds, is certainly longer than the dialog's fade-in.
///
/// The toolkit's dialog fades in over about a quarter of a second, advanced by
/// `Tick` events. Most document applications have no clock -- nothing in them
/// moves by itself -- so a question left to fade would be drawn at nothing and
/// stay that way until some unrelated event arrived. It is shown whole at
/// once instead: the user has just asked to close the window, and the answer
/// to that belongs on the screen now. It is also what anyone who has asked for
/// less motion wants.
const FADE_DONE_MS: u64 = 1_000;

/// The answers to the question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Save, then go on -- if the save works.
    Save,
    /// Go on without saving: the changes are lost.
    Discard,
    /// Do not go on. The document stays open, and still unsaved.
    Cancel,
}

/// The question, while it is being asked, and what it is holding up.
///
/// `P` is the application's own: which tab, the window, an Open. It comes
/// back from [`pending`](Self::pending) when the answer arrives, so the
/// question and what it was about cannot be separated.
///
/// `Clone` and `Debug` because the toolkit's dialog is both, and some
/// applications derive them on their whole state.
#[derive(Clone, Debug)]
pub struct Question<P> {
    dialog: AlertDialog,
    pending: P,
}

impl<P: Copy> Question<P> {
    /// Ask about unsaved changes. `message` says whose (see [`message_for`]),
    /// `prompt` what is about to happen -- "Save them before closing?".
    #[must_use]
    pub fn new(message: &str, prompt: &str, pending: P) -> Self {
        let buttons = ButtonSet::custom(vec![
            DialogButton::new("Save", DialogResult::Yes, ButtonRole::Primary),
            // In the error colour: the one answer that loses something.
            DialogButton::new("Don't save", DialogResult::No, ButtonRole::Destructive),
            DialogButton::cancel(),
        ]);
        let mut dialog = AlertDialog::warning("Unsaved changes", message)
            .with_buttons(buttons)
            .with_detail(prompt)
            // A click beside the card answers nothing, as with any desktop's
            // modal question: a stray click must not be what takes it away.
            // Escape and Cancel are the ways out.
            .with_click_outside_dismiss(false);
        dialog.show();
        dialog.tick(FADE_DONE_MS);
        Self { dialog, pending }
    }

    /// What the question is holding up.
    #[must_use]
    pub fn pending(&self) -> P {
        self.pending
    }

    /// Whose changes the question is about.
    #[must_use]
    pub fn message(&self) -> &str {
        self.dialog.message()
    }

    /// Offer `event` to the question: `Some` once it is answered, `None` while
    /// it is still being asked.
    ///
    /// Call it before anything else looks at the event, for every event while
    /// the question is up. It takes every key and every click; a click
    /// beside its card answers nothing.
    pub fn handle(&mut self, event: &Event) -> Option<Choice> {
        if let Event::Key(key) = event
            && key.pressed
        {
            let typed = key.typed().next().map(|c| c.to_ascii_lowercase());
            match (key.key, typed) {
                (Key::S, _) | (_, Some('s')) => return Some(Choice::Save),
                (Key::D, _) | (_, Some('d')) => return Some(Choice::Discard),
                _ => {}
            }
        }
        // What the dialog does with it is its business -- focus, hover,
        // Escape -- and whether the event mattered is not: the question has
        // the window until it is answered.
        let _ = self.dialog.handle_event(event);
        self.dialog.result().map(|result| match result {
            DialogResult::Yes => Choice::Save,
            DialogResult::No => Choice::Discard,
            _ => Choice::Cancel,
        })
    }

    /// Draw the question over everything else, the whole window dimmed
    /// behind it.
    ///
    /// Drawing is also what places its buttons, as with every toolkit dialog:
    /// a click is tested against where they were last drawn, so a question
    /// cannot be clicked before it has been seen.
    pub fn render(&mut self, palette: &Palette, width: f32, height: f32, tree: &mut RenderTree) {
        self.dialog.render(palette, width, height, tree);
    }

    /// [`render`](Self::render), as a list of commands, for an application
    /// that draws into something other than a [`RenderTree`].
    #[must_use]
    pub fn commands(&mut self, palette: &Palette, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut tree = RenderTree::new();
        self.render(palette, width, height, &mut tree);
        tree.commands
    }

    /// The middle of the button for `choice`, where it was last drawn --
    /// for a test, or anything else that has to aim at it. `None` before the
    /// question has been drawn.
    #[must_use]
    pub fn button_centre(&self, choice: Choice) -> Option<(f32, f32)> {
        let index = match choice {
            Choice::Save => 0,
            Choice::Discard => 1,
            Choice::Cancel => 2,
        };
        self.dialog
            .button_rect(index)
            .map(|(x, y, w, h)| (x + w / 2.0, y + h / 2.0))
    }
}

/// What the question says about whose changes they are.
///
/// One name: "notes.json has changes that are not saved." Several: "3
/// documents have changes that are not saved: a.json, b.json and c.json."
/// None -- a caller asking about nothing -- is said plainly rather than as an
/// empty list.
#[must_use]
pub fn message_for(names: &[&str]) -> String {
    match names {
        [] => String::from("There are changes that are not saved."),
        [one] => format!("{one} has changes that are not saved."),
        [init @ .., last] => format!(
            "{} documents have changes that are not saved: {} and {last}.",
            names.len(),
            init.join(", ")
        ),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use guitk::event::{KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};

    const W: f32 = 1000.0;
    const H: f32 = 700.0;

    fn question() -> Question<u8> {
        Question::new(
            &message_for(&["deck.slides"]),
            "Save them before closing?",
            7,
        )
    }

    fn press(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    fn typed(c: char) -> Event {
        Event::Key(KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: c.to_string(),
        })
    }

    fn click(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })
    }

    fn drawn(q: &mut Question<u8>) -> String {
        q.commands(&Palette::for_mode(false), W, H)
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            // A space, not a separator: the dialog wraps a long message
            // over several lines, and a sentence must still read as one.
            .join(" ")
    }

    #[test]
    fn each_key_gives_its_answer() {
        for (event, expected) in [
            (press(Key::S), Choice::Save),
            (typed('S'), Choice::Save),
            (press(Key::Enter), Choice::Save),
            (press(Key::D), Choice::Discard),
            (typed('d'), Choice::Discard),
            (press(Key::Escape), Choice::Cancel),
        ] {
            assert_eq!(question().handle(&event), Some(expected), "{event:?}");
        }
    }

    /// Tab moves the focus, so Enter answers whichever button has it.
    #[test]
    fn tab_moves_between_the_answers() {
        let mut q = question();
        assert_eq!(q.handle(&press(Key::Tab)), None);
        assert_eq!(q.handle(&press(Key::Enter)), Some(Choice::Discard));
    }

    /// A key that is not an answer is taken and answers nothing: it must not
    /// reach the document, and it must not lose it.
    #[test]
    fn any_other_key_is_swallowed() {
        let mut q = question();
        for event in [press(Key::N), press(Key::Y), typed('x'), press(Key::Delete)] {
            assert_eq!(q.handle(&event), None, "{event:?}");
        }
        assert_eq!(q.handle(&press(Key::S)), Some(Choice::Save));
    }

    #[test]
    fn each_button_gives_its_answer_where_it_is_drawn() {
        for choice in [Choice::Save, Choice::Discard, Choice::Cancel] {
            let mut q = question();
            assert_eq!(
                q.button_centre(choice),
                None,
                "nothing is placed before a frame"
            );
            drawn(&mut q);
            let (x, y) = q.button_centre(choice).expect("drawn");
            assert_eq!(q.handle(&click(x, y)), Some(choice));
        }
    }

    /// A click beside the card is not an answer: a stray click must not take
    /// the question away.
    #[test]
    fn a_click_outside_answers_nothing() {
        let mut q = question();
        drawn(&mut q);
        assert_eq!(q.handle(&click(2.0, 2.0)), None);
        let (x, y) = q.button_centre(Choice::Cancel).expect("drawn");
        assert_eq!(q.handle(&click(x, y)), Some(Choice::Cancel), "still asking");
    }

    /// It is on the screen from the first frame, with no clock to fade it in.
    #[test]
    fn it_is_drawn_at_once() {
        let mut q = question();
        let text = drawn(&mut q);
        assert!(
            text.contains("deck.slides has changes that are not saved."),
            "{text}"
        );
        assert!(text.contains("Save them before closing?"), "{text}");
        for label in ["Save", "Don't save", "Cancel"] {
            assert!(text.contains(label), "{label} is not drawn: {text}");
        }
        let scrim = q
            .commands(&Palette::for_mode(false), W, H)
            .into_iter()
            .find_map(|c| match c {
                RenderCommand::FillRect { color, width, .. } if (width - W).abs() < 0.5 => {
                    Some(color)
                }
                _ => None,
            })
            .expect("a scrim over the whole window");
        assert!(scrim.a > 100, "the scrim is still fading in: {scrim:?}");
    }

    #[test]
    fn the_answer_comes_back_with_what_it_held_up() {
        let q = question();
        assert_eq!(q.pending(), 7);
        assert_eq!(q.message(), "deck.slides has changes that are not saved.");
    }

    #[test]
    fn the_message_names_whose_changes() {
        assert_eq!(message_for(&[]), "There are changes that are not saved.");
        assert_eq!(
            message_for(&["a.json"]),
            "a.json has changes that are not saved."
        );
        assert_eq!(
            message_for(&["a.json", "b.json", "c.json"]),
            "3 documents have changes that are not saved: a.json, b.json and c.json."
        );
    }
}
