//! The credential service's own windows -- and, once it can read the
//! password manager's vault, the daemon that runs it.
//!
//! `gui/credentials` holds the service's judgement and its protocol, and
//! nothing that draws: a program asking for a password links it, and should
//! not link a widget library to ask. What the service shows the user lives
//! here: [`prompt`], the windows that ask whether a program may have a
//! password and, when several would do, which.
//!
//! What comes here next (`roadmap.md`, the credential service's item): the
//! adapter over the password manager's vault file, which waits on lane E
//! sharing it (`requests/c-e-share-the-password-vault-with-the-credential-service.md`),
//! and the daemon that registers the service, accepts each program's
//! connection and answers it through `credentials::service::serve`.

pub mod prompt;

pub use prompt::WindowPrompt;
