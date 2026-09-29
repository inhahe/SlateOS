//! The grammars: each one's tables and lexers, generated from its
//! `parser.c` by `build.rs`; its external scanner, ported by hand from its
//! `scanner.c`, where it has one; and its highlight query.
//!
//! A grammar's module includes the generated file into a `generated`
//! submodule of its own, whose lints are relaxed with a reason: the code is
//! a converted state machine nobody edits, and its shape is the generator's.
//! The hand-ported scanner beside it is held to every lint the crate is.

pub(crate) mod bash;
pub(crate) mod c;
pub(crate) mod cpp;
pub(crate) mod css;
pub(crate) mod html;
pub(crate) mod javascript;
pub(crate) mod json;
pub(crate) mod markdown;
pub(crate) mod markdown_inline;
pub(crate) mod python;
pub(crate) mod rust;
pub(crate) mod toml;
pub(crate) mod tsx;
pub(crate) mod typescript;
pub(crate) mod yaml;

/// The generated file for a grammar, in a module of its own. `$scanner`,
/// when given, is the type the generated code calls `Scanner` -- and its
/// tokens must be the grammar's, name for name, or the grammar does not
/// build: the runtime hands a scanner one flag for each of the grammar's
/// tokens, and the scanner reads as many flags as it has tokens.
macro_rules! generated {
    ($dir:literal) => {
        #[allow(
            clippy::all,
            clippy::pedantic,
            unused_assignments,
            unused_variables,
            unreachable_code,
            unused_parens,
            unused_imports,
            dead_code,
            reason = "generated from the grammar's parser.c by build.rs: a converted state machine, in the generator's shape"
        )]
        pub(crate) mod generated {
            include!(concat!(env!("OUT_DIR"), "/", $dir, "/language.rs"));
            crate::ffi::language_fn!();
        }
    };
    ($dir:literal, $scanner:ty) => {
        #[allow(
            clippy::all,
            clippy::pedantic,
            unused_assignments,
            unused_variables,
            unreachable_code,
            unused_parens,
            unused_imports,
            dead_code,
            reason = "generated from the grammar's parser.c by build.rs: a converted state machine, in the generator's shape"
        )]
        pub(crate) mod generated {
            type Scanner = $scanner;
            include!(concat!(env!("OUT_DIR"), "/", $dir, "/language.rs"));
            crate::ffi::language_fn!();
            const _: () = assert!(
                crate::ffi::same_names(
                    <Scanner as crate::ffi::ExternalScanner>::TOKENS,
                    &EXTERNAL_TOKENS
                ),
                "the scanner's tokens are not its grammar's, in the grammar's order"
            );
        }
    };
}
pub(crate) use generated;
