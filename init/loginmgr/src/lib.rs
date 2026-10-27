//! `loginmgr`: the one privileged program that signs people in.
//!
//! It is a display manager without a display. init starts it; it starts the
//! login screen (lane C's `greeter`) as an account of its own with no
//! privilege, answers the screen's questions over the service
//! `org.slateos.LoginManager` ([`protocol`]), and once someone has signed
//! in, tells logind a session began and starts their desktop *as them*. When
//! the desktop exits -- a log-out -- it ends the session and starts the
//! screen again. This is the shape of gdm, lightdm and sddm, agreed with lane
//! C in `requests/b-c-one-process-signs-people-in-and-it-is-not-the-desktop.md`
//! and recorded in design-decisions §1076.
//!
//! The screen never sees a password hash (§341): it hands the typed password
//! here and gets a verdict, from `authlib`, whose tally of wrong guesses is
//! the one `login` and `su` count against. The account database's private
//! part stays root's: the screen gets the accounts it offers from
//! `ListAccounts`.
//!
//! # Where the parts are
//!
//! - [`protocol`]: the service's calls and their encoding, and the client the
//!   login screen uses.
//! - [`manager`]: every rule -- who may call, what each call does, what
//!   follows an exit -- with no system call in it, tested on the host.
//! - [`system`]: the real account database, logind and launcher.
//! - `main.rs`: the service's threads and the supervisor that waits for the
//!   two programs.
//!
//! # What it cannot do yet
//!
//! Start a program as another user holding only the capabilities it is
//! given: see [`system::KernelLauncher`]. Until lane A's kernel can, it says
//! so and starts nothing.

pub mod manager;
pub mod protocol;
pub mod system;
