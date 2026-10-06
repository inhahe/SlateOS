#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::cell::RefCell;
use std::rc::Rc;

use sound::Errno;
use sound::mixer::{IOCTL_ELEM_WRITE, Simulated};

use super::*;

/// A simulated card the test keeps a hold of while the shell uses it.
#[derive(Clone)]
pub(crate) struct Shared(pub(crate) Rc<RefCell<Simulated>>);

impl ControlSys for Shared {
    fn ioctl(&mut self, request: u32, payload: &mut [u8]) -> Result<(), Errno> {
        self.0.borrow_mut().ioctl(request, payload)
    }
}

/// SlateOS's card at `level`, muted or not, and an output on it.
pub(crate) fn card(level: i64, muted: bool) -> (Shared, Output) {
    let shared = Shared(Rc::new(RefCell::new(Simulated::new(level, muted))));
    let master = Master::open(shared.clone()).unwrap();
    (shared, Output::card(Box::new(master)))
}

/// **Where no card was asked for, nothing is read or written**: the
/// shell's own number is the volume.
#[test]
fn with_no_card_nothing_is_read_or_written() {
    let mut own = Output::Own;
    assert_eq!(own.read(), Ok(None));
    assert_eq!(own.write(30, true), Ok(()));
    assert_eq!(own.out_of_reach(), None);
}

/// **A card is read, and written only where the level or the mute
/// changed** -- a press that moved neither asks nothing of the card.
#[test]
fn a_card_is_written_only_where_something_changed() {
    let (card_state, mut output) = card(40, false);
    assert_eq!(output.read(), Ok(Some((40, false))));
    let writes = || {
        card_state
            .0
            .borrow()
            .requests
            .iter()
            .filter(|&&r| r == IOCTL_ELEM_WRITE)
            .count()
    };
    output.write(40, false).unwrap();
    assert_eq!(writes(), 0, "nothing changed");
    output.write(60, false).unwrap();
    assert_eq!(writes(), 1, "the level alone");
    output.write(60, true).unwrap();
    assert_eq!(writes(), 2, "the mute alone");
    let state = card_state.0.borrow();
    assert_eq!(state.volume, [60]);
    assert_eq!(state.switch, Some(vec![0]));
}

/// **A card that fails is out of reach from then on**, and says why; what
/// is asked of it after is refused without asking the card.
#[test]
fn a_card_that_fails_is_out_of_reach_from_then_on() {
    let (card_state, mut output) = card(40, false);
    card_state.0.borrow_mut().refuse = Some(Errno::ENOTTY);
    assert_eq!(output.read(), Err("No sound card reachable"));
    assert_eq!(output.out_of_reach(), Some("No sound card reachable"));
    let asked = card_state.0.borrow().requests.len();
    assert_eq!(output.write(10, false), Err("No sound card reachable"));
    assert_eq!(
        card_state.0.borrow().requests.len(),
        asked,
        "not asked again"
    );

    let (card_state, mut output) = card(40, false);
    output.read().unwrap();
    card_state.0.borrow_mut().refuse = Some(Errno::EINVAL);
    assert_eq!(output.write(50, false), Err("Sound card not answering"));
}

/// **Why a card cannot be used, in a few words**: a device that will not
/// open and one that answers nothing are one sentence.
#[test]
fn why_a_card_cannot_be_used_is_said_in_a_few_words() {
    for e in [
        MixerError::Open(Errno::ENOENT),
        MixerError::Open(Errno::ENODEV),
        MixerError::Refused(Errno::ENOTTY),
        MixerError::Refused(Errno::ENOSYS),
    ] {
        assert_eq!(why(e), "No sound card reachable", "{e:?}");
    }
    assert_eq!(why(MixerError::NoMaster), "No volume control");
    assert_eq!(
        why(MixerError::Refused(Errno::EINVAL)),
        "Sound card not answering"
    );
    assert_eq!(why(MixerError::Garbled), "Sound card not answering");
}

/// **On a system the card is not opened on, the desktop's output is out of
/// reach** -- not the shell's own number, which only a shell that asked for
/// no card has.
#[test]
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn on_another_system_the_desktops_output_is_out_of_reach() {
    assert_eq!(
        Output::open().out_of_reach(),
        Some("No sound card reachable")
    );
}
