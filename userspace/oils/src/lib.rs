#![deny(clippy::all)]

//! Oils (OSH) — a bash/POSIX-superset shell for SlateOS.
//!
//! This is a Rust reimplementation of the OSH shell language (not a
//! cross-compile of upstream Oils' C++; see `design-decisions.md §72` and
//! `open-questions.md Q26` for the rationale). It parses POSIX-sh + bash-core
//! syntax and executes it against SlateOS syscalls, forking/execing real
//! external programs via [`std::process::Command`].
//!
//! Pipeline of stages:
//! 1. [`lexer`] — source text → tokens (words keep quoting; substitutions keep
//!    raw inner source).
//! 2. [`parser`] — tokens → [`ast::Program`] (recursive descent; recursively
//!    parses command/parameter substitutions).
//! 3. [`interp`] — tree-walking execution via [`interp::Shell`].
//!
//! Arithmetic (`$(( … ))`) is handled by [`arith`].
//!
//! Shell strings are **bytes**, not Rust `String`s — a SlateOS path may hold
//! any byte but `/` and NUL, so a shell that cannot carry one cannot name every
//! file. [`bytes`] defines that type and the operations `bstr` lacks.

pub mod arith;
pub mod assoc;
pub mod ast;
pub mod bind_keys;
pub mod bind_tables;
pub mod brace;
pub mod bytes;
/// The regex engine `[[ str =~ re ]]` matches with. It was a module here until
/// `grep`, `sed`, `awk` and `expr` each turned out to be matching regular
/// expressions with `str::contains`; it is now the `ere` crate, so the shell
/// and the utilities cannot disagree about what a pattern means. Re-exported
/// under its old path because that is still where a reader of osh looks.
pub use ::ere;
pub(crate) mod escape;
pub mod histexpand;
/// Test scaffolding: the `$PATH` a test may search, with the two directories
/// cargo injects struck out. Compiled only for the in-process tests; the
/// end-to-end tests in `tests/` pull the same file in with `#[path]` rather
/// than link it, so none of it reaches the shipped shell. See the module docs
/// for why one definition has to serve both.
#[cfg(test)]
pub(crate) mod hostpath;
pub mod interp;
pub mod lexer;
pub mod parser;
pub mod unparse;
pub mod width;
/// Windows host only: the command line a child parses is not always the one
/// Rust knows how to spell. On SlateOS a child is handed an argv vector and
/// there is no command line to encode.
#[cfg(windows)]
pub(crate) mod wincmd;
pub mod wordscan;

pub use interp::{Shell, StartupFiles};
pub use parser::parse;
