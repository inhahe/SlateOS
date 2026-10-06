//! Signals: what a widget tree tells its program -- a button clicked, a box
//! ticked, a field's text changed -- and the slots that hear them.
//!
//! `roadmap-detailed.md` §3.5 (*Signals and Slots*) asks for a signal/slot
//! mechanism "that maps to Rust channels or callback registration". A widget
//! that the user acts on emits a [`Signal`]; the tree gathers them after each
//! event and hands each to every way a program has asked to hear it:
//!
//! - **as a queue** -- [`WidgetTree::take_signals`] after handling an event,
//!   and a `match` on each: the idiom for a program that owns its state,
//!   since a Rust closure cannot borrow it;
//! - **on a channel** -- [`WidgetTree::connect_channel`], for a program whose
//!   state lives on another thread;
//! - **to a callback** -- [`WidgetTree::connect`], for one widget's signals
//!   or every widget's, until [`WidgetTree::disconnect`].
//!
//! Only what the *user* does is signalled: a program that ticks a box or sets
//! a field's text knows it did, and hearing its own change back is how a
//! program ends up answering itself.
//!
//! [`WidgetTree::take_signals`]: super::WidgetTree::take_signals
//! [`WidgetTree::connect_channel`]: super::WidgetTree::connect_channel
//! [`WidgetTree::connect`]: super::WidgetTree::connect
//! [`WidgetTree::disconnect`]: super::WidgetTree::disconnect

use super::{CheckState, WidgetId};

/// Something the user did to a widget that its program may act on.
#[derive(Clone, Debug, PartialEq)]
pub struct Signal {
    /// The widget it was done to.
    pub from: WidgetId,
    /// What was done.
    pub kind: SignalKind,
}

/// What a widget's user did.
#[derive(Clone, Debug, PartialEq)]
pub enum SignalKind {
    /// A button was clicked: pressed and released over it, or Space or
    /// Enter while it had the keyboard.
    Clicked,
    /// A checkbox was ticked, unticked or set to its middle state: the state
    /// it is in now.
    Toggled(CheckState),
    /// A radio button was chosen -- the rest of its group are not now.
    Chosen,
    /// A text field's or a text area's text was changed.
    Edited,
    /// Enter was pressed in a one-line text field: what a form takes as
    /// "done".
    Submitted,
    /// A slider was moved, to this value.
    Moved(f64),
}

/// A callback's handle, for [`WidgetTree::disconnect`].
///
/// [`WidgetTree::disconnect`]: super::WidgetTree::disconnect
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SlotId(pub(super) u64);

/// A callback connected to a tree's signals: one widget's, or every one's.
pub(super) struct Slot {
    pub(super) id: SlotId,
    pub(super) from: Option<WidgetId>,
    pub(super) call: Box<dyn FnMut(&Signal)>,
}

/// The most signals a tree keeps for [`WidgetTree::take_signals`]: past it
/// the oldest are dropped, so a program that hears its signals another way
/// and never takes them does not keep every one it was ever sent.
///
/// [`WidgetTree::take_signals`]: super::WidgetTree::take_signals
pub const MAX_QUEUED_SIGNALS: usize = 1024;
