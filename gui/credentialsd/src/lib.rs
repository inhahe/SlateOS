//! The credential service's own windows and its loop -- and, once it can
//! read the password manager's vault, the daemon that runs them.
//!
//! `gui/credentials` holds the service's judgement and its protocol, and
//! nothing that draws: a program asking for a password links it, and should
//! not link a widget library to ask. What the service shows the user, and
//! the loop that serves the programs, live here:
//!
//! - [`prompt`]: the windows that ask whether a program may have a
//!   password and, when several would do, which.
//! - [`daemon`]: each program's connection accepted and answered, one at a
//!   time, a record of each written, and the vault locked on time.
//!
//! What comes here next (`roadmap.md`, the credential service's item): the
//! adapter over the password manager's vault file, which waits on lane E
//! sharing it (`requests/c-e-share-the-password-vault-with-the-credential-service.md`),
//! and the binary -- register, open the vault, [`daemon::run`].

pub mod daemon;
pub mod prompt;

pub use prompt::WindowPrompt;
