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

pub mod arith;
pub mod ast;
pub mod brace;
pub mod ere;
pub mod interp;
pub mod lexer;
pub mod parser;

pub use interp::Shell;
pub use parser::parse;
