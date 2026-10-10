//! The grammars: each one's tables and lexers, generated from its
//! `parser.c` by `build.rs`; its external scanner, ported by hand from its
//! `scanner.c`, where it has one; and its highlight query.
//!
//! A grammar's module includes the generated file into a `generated`
//! submodule of its own, whose lints are relaxed with a reason: the code is
//! a converted state machine nobody edits, and its shape is the generator's.
//! The hand-ported scanner beside it is held to every lint the crate is.

pub(crate) mod ada;
pub(crate) mod bash;
pub(crate) mod c;
pub(crate) mod cpp;
pub(crate) mod css;
pub(crate) mod diff;
pub(crate) mod dockerfile;
pub(crate) mod dtd;
pub(crate) mod go;
pub(crate) mod html;
pub(crate) mod ini;
pub(crate) mod java;
pub(crate) mod javascript;
pub(crate) mod jsdoc;
pub(crate) mod json;
pub(crate) mod linkerscript;
pub(crate) mod lua;
pub(crate) mod make;
pub(crate) mod markdown;
pub(crate) mod markdown_inline;
pub(crate) mod nu;
pub(crate) mod powershell;
pub(crate) mod python;
pub(crate) mod query;
pub(crate) mod regex;
pub(crate) mod rust;
pub(crate) mod sql;
pub(crate) mod toml;
pub(crate) mod tsx;
pub(crate) mod typescript;
pub(crate) mod xml;
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

#[cfg(test)]
mod tests {
    /// **Every table of every grammar inflates to its length**: the tables
    /// are carried deflated, and one that did not inflate would be a grammar
    /// that reads nothing.
    #[test]
    fn every_table_inflates_to_its_length() {
        let grammars: [(&str, &[&crate::ffi::Deflated]); 32] = [
            ("ada", &super::ada::generated::TABLES),
            ("bash", &super::bash::generated::TABLES),
            ("c", &super::c::generated::TABLES),
            ("cpp", &super::cpp::generated::TABLES),
            ("css", &super::css::generated::TABLES),
            ("diff", &super::diff::generated::TABLES),
            ("dockerfile", &super::dockerfile::generated::TABLES),
            ("dtd", &super::dtd::generated::TABLES),
            ("go", &super::go::generated::TABLES),
            ("html", &super::html::generated::TABLES),
            ("ini", &super::ini::generated::TABLES),
            ("java", &super::java::generated::TABLES),
            ("javascript", &super::javascript::generated::TABLES),
            ("jsdoc", &super::jsdoc::generated::TABLES),
            ("json", &super::json::generated::TABLES),
            ("linkerscript", &super::linkerscript::generated::TABLES),
            ("lua", &super::lua::generated::TABLES),
            ("make", &super::make::generated::TABLES),
            ("markdown", &super::markdown::generated::TABLES),
            (
                "markdown_inline",
                &super::markdown_inline::generated::TABLES,
            ),
            ("nu", &super::nu::generated::TABLES),
            ("powershell", &super::powershell::generated::TABLES),
            ("python", &super::python::generated::TABLES),
            ("query", &super::query::generated::TABLES),
            ("regex", &super::regex::generated::TABLES),
            ("rust", &super::rust::generated::TABLES),
            ("sql", &super::sql::generated::TABLES),
            ("toml", &super::toml::generated::TABLES),
            ("tsx", &super::tsx::generated::TABLES),
            ("typescript", &super::typescript::generated::TABLES),
            ("xml", &super::xml::generated::TABLES),
            ("yaml", &super::yaml::generated::TABLES),
        ];
        for (name, tables) in grammars {
            assert!(tables.len() > 5, "{name}");
            for (i, table) in tables.iter().enumerate() {
                assert!(table.inflates(), "{name}: table {i}");
            }
        }
    }
}
