//! The credential service: a program asking the password manager for a
//! password.
//!
//! `design-decisions.md` §1417 (the operator's, answering C-Q25): a program
//! may ask for a saved password over a secure connection -- only if it holds
//! a capability, a system-issued key, for exactly that, and only after the
//! user, shown which program is asking, allows it. Lane A built the key
//! (§1518: the kernel answers, for a connection to a service, whether the
//! process at the other end holds that service's key); this crate is the
//! rest.
//!
//! - [`protocol`]: what a program asks ([`Query`]) and what it is answered
//!   ([`Answer`]), one of each per connection to the [`SERVICE`], every field
//!   bounded; and [`Secret`], a password that is overwritten when dropped
//!   and never printed.
//! - [`client`]: a program's side -- [`ask`].
//! - [`service`]: the service's judgement -- who may ask, when the user is
//!   asked, and which login is given -- against a vault and a prompt it is
//!   handed, and [`service::serve`], one connection answered.
//! - [`matching`]: which saved login a target is about.
//!
//! The service's two remaining parts are its prompt window and its vault --
//! the password manager's own, shared by lane E
//! (`requests/c-e-share-the-password-vault-with-the-credential-service.md`).
//! Until both exist nothing registers [`SERVICE`], and a program that asks
//! is told there is no service.
//!
//! The binary beside this library (`src/main.rs`) is the credential store
//! that came before §1417, with a vault of its own under a home-made cipher
//! (`known-issues/C-THE-CREDENTIAL-STORE-ENCRYPTS-EVERY-SECRET-WITH-THE-SAME-KEYSTREAM.md`).
//! Nothing starts it, and it goes once the service reads lane E's vault.

pub mod client;
pub mod matching;
pub mod protocol;
pub mod service;

pub use client::ask;
pub use protocol::{Answer, Query, SERVICE, Secret};
