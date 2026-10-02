### TD-OILS-ASSOC-KEY-TRIM. `osh` trims leading/trailing whitespace from an unquoted associative-array subscript key — 2026-07-19 — RESOLVED 2026-07-19

**Where:** `userspace/oils/src/parser.rs` subscript extraction
(`try_assignment` store path, `split_name_subscript` read path,
`parse_array_elem` keyed-element path); `userspace/oils/src/lexer.rs`
array-literal element tokenization (`read_word_inner` / new
`read_array_elem_word`).

**What (was):** After the lexer fix that keeps `h[a b]=v` as one assignment
word (interior spaces preserved), a subscript with *leading/trailing*
spaces was still trimmed: `declare -A h; h[ x ]=v; echo "${!h[@]}"` printed
`x` in osh but ` x ` in bash. Two distinct causes: (1) the three
subscript-parse sites lowered the subscript via `word_from_source`, which
re-tokenizes and word-splits (dropping surrounding whitespace); (2) inside
an array literal `([ x ]=v)`, the lexer word-split the element on the
interior spaces, so it wasn't even recognised as one keyed element.

**Fix shipped:** (1) all three subscript-parse sites now use
`word_verbatim_from_source` (parses a single word with no
splitting/trimming), so the expanded text — surrounding whitespace
included — is the literal associative key on both store and read paths;
arithmetic/indexed subscripts arithmetic-evaluate and ignore the
whitespace, so they are unaffected. (2) A new `Lexer::read_array_elem_word`
(via a `array_elem` flag on `read_word_inner`) slurps a leading
`[subscript]=value` element across unquoted interior spaces, matching
bash's array-literal tokenization, so `declare -A m=([ x ]=v [y z]=w)`
keys on ` x ` / `y z`. Also fixed `declare -p` to quote associative keys
that hold shell metacharacters (`["a b"]`, ANSI-C `$'…'` for control
chars) via a new `quote_declare_key`, so printed subscripts round-trip.
Tests: `interp::assoc_key_preserves_surrounding_whitespace`,
`interp::declare_p_quotes_assoc_keys_needing_it`,
`lexer::array_literal_keyed_element_keeps_spaces`. Verified against bash
across store/read/`declare -p`/array-literal cases.
