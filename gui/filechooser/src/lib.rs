//! A program's Open and Save window, shown by the file explorer.
//!
//! `design-decisions.md` §1415 (the operator's, answering C-Q30): a program
//! no longer draws its own Open or Save window and reads your folders to fill
//! it. It asks; the file explorer -- a trusted part of the system -- shows its
//! own window in "choose a file" form above the program; you browse with the
//! explorer's views; and the program is told the file you chose and nothing
//! else. §1463 records how lane C built its part, and what the other lanes'
//! parts are.
//!
//! - [`protocol`]: what a program asks ([`Request`]) and what it is answered
//!   ([`Reply`]), one of each per connection to the [`SERVICE`] the explorer
//!   registers, every field bounded so neither side can be made to hold more
//!   than a request or a reply needs.
//! - [`Picker`]: the toolkit's [`FilePicker`](guitk::dialog::FilePicker) with
//!   the explorer in front of it -- the same calls, asked of the explorer when
//!   there is one to ask and drawn by the program as before when there is not
//!   (a development host; a session the explorer is not serving).
//! - [`service`]: the explorer's side, reading what it is asked and answering.
//!
//! Until a program can be handed an open file rather than a name (lanes A and
//! D), the reply is a path -- no weaker than a program reading the folders
//! itself, and the shape the stronger answer will arrive in.

pub mod client;
pub mod protocol;
pub mod service;

pub use client::{Connect, Picker, SystemConnect};
pub use protocol::{Filter, Mode, Reply, Request, SERVICE};
