## TD-EXPLORER-SORT-IS-CODEPOINT-NOT-COLLATION

**Status: OPEN 2026-08-16** (lane C). `apps/explorer/src/columns.rs`,
`impl Ord for ColumnValue`.

Sorting a text column folds case and then compares Unicode scalar values. That
is well-defined, consistent, and agrees with `textfind`'s idea of equality — but
it is not the order a reader of the language expects. `éclair` sorts after
`zulu`, because U+00E9 is greater than `z`; a French speaker expects it between
`alpha` and `zulu`. Every script that is not ASCII is affected, and names that
differ only by an accent are separated rather than adjacent.

**Why it is not fixed here.** Correct placement is the Unicode Collation
Algorithm, which is a data table (DUCET, ~30 000 entries) plus a
locale-tailoring layer, not an algorithm one writes from memory. The tree has
no Unicode data of any kind today. Doing it properly means deciding first
whether the OS ships collation data at all, and if so whether it is
locale-tailored or root-only — which is a design question, not a bug fix.

**The proper fix**, when the prerequisite exists: a `textfmt::collate` (or a
`textcollate` crate) carrying DUCET primary/secondary/tertiary weights, with
`textfind::compare` delegating to it and keeping its current behaviour as the
documented fallback when no table is loaded. Every caller then improves at
once, since `compare` is the single place case-insensitive ordering is decided.

**What it costs while open:** a mis-ordered file list for non-ASCII names.
Nothing is unsafe and nothing is blocked; the sort is total and stable, just
not idiomatic for the locale.
