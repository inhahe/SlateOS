//! Linker scripts: tree-sitter-linkerscript 1.0.0 (MIT, Amaan Qureshi) --
//! GNU ld's command language, `SECTIONS { .text : { *(.text*) } }`, which the
//! kernel, the services and the userspace programs each link by.
//! `grammars/linkerscript/` holds its `parser.c` and its highlight query as
//! published; it has no external scanner.
//!
//! The package publishes no test corpus, so the tests below parse scripts
//! instead: `examples/` holds copies of the kernel's and a service's
//! (`kernel/linker.ld`, `services/init/linker.ld`, as they were on
//! 2026-09-29) and one written here of what those do not use -- `MEMORY`,
//! `OVERLAY`, `PROVIDE`, `/DISCARD/`, a region's `>` and `AT >`. Its locals
//! query is not read: its captures are named for an older Neovim
//! (`@definition.var`), not tree-sitter's `@local.*`.
//!
//! # What the grammar does not know
//!
//! Some of ld's language the grammar has no rule for, and it parses as an
//! error -- coloured by what error recovery makes of it, which is most of
//! it: a program header's `FLAGS(...)`, an output section's `ALIGN(...)` (or
//! any attribute) after its colon, a second `:phdr` after a section,
//! `INCLUDE`, `OUTPUT_FORMAT` and `OUTPUT_ARCH`. The kernel's script uses
//! the first two. The test pins exactly these, so a grammar that learns one
//! -- or loses something else -- shows at once; `known-issues.md`
//! (`TD-C-THE-LINKER-SCRIPT-GRAMMAR-DOES-NOT-KNOW-ALL-OF-LD`) has the fix.

super::generated!("linkerscript");

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/linkerscript/highlights.scm");

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a test: a parse that fails is the failure"
)]
mod tests {
    use super::*;

    /// `text`'s tree, as an S-expression.
    fn sexp(text: &str) -> String {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter::Language::new(generated::language_fn()))
            .unwrap();
        parser.parse(text, None).unwrap().root_node().to_sexp()
    }

    /// The text of every error node in `text`'s tree, in the text's order.
    fn errors(text: &str) -> Vec<String> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter::Language::new(generated::language_fn()))
            .unwrap();
        let tree = parser.parse(text, None).unwrap();
        let mut found = Vec::new();
        let mut stack = vec![tree.root_node()];
        let mut cursor = tree.walk();
        while let Some(node) = stack.pop() {
            if node.is_error() || node.is_missing() {
                found.push((node.start_byte(), text[node.byte_range()].to_owned()));
            }
            stack.extend(node.children(&mut cursor));
        }
        found.sort();
        found.into_iter().map(|(_, t)| t).collect()
    }

    /// **The tree's own linker scripts parse, but for exactly the grammar's
    /// known gaps** (see the module docs): the kernel's `FLAGS(...)` and
    /// its sections' `ALIGN(...)` after their colons, a service's second
    /// `:phdr`, and `INCLUDE`, `OUTPUT_FORMAT` and `OUTPUT_ARCH`.
    #[test]
    fn the_trees_linker_scripts_parse_but_for_the_known_gaps() {
        let kernel = errors(include_str!(
            "../../grammars/linkerscript/examples/kernel.ld"
        ));
        assert_eq!(
            kernel,
            [
                "FLAGS((1 << 0) | (1 << 1))",
                "FLAGS((1 << 0) | (1 << 2))",
                "FLAGS((1 << 0))",
                "FLAGS((1 << 0) | (1 << 1))",
                ":",
                ":",
                ":",
                ":",
                ":",
            ]
        );
        let service = errors(include_str!(
            "../../grammars/linkerscript/examples/service.ld"
        ));
        assert_eq!(service, ["FLAGS(7)", "FLAGS(4)", ":load"]);
        let memory = errors(include_str!(
            "../../grammars/linkerscript/examples/memory.ld"
        ));
        assert_eq!(
            memory,
            [
                "OUTPUT_FORMAT(\"elf64-x86-64\")\nOUTPUT_ARCH(i386:x86-64)",
                "INCLUDE common.ld",
                ":",
            ]
        );
        let tree = sexp(include_str!(
            "../../grammars/linkerscript/examples/memory.ld"
        ));
        for node in [
            "(memory_command",
            "(overlay_command",
            "(provide_command",
            "(output_section",
            "(keep_command",
            "(entry_command",
        ] {
            assert!(tree.contains(node), "{node}: {tree}");
        }
    }
}
