//! The converter against a `parser.c` in the generator's shape: small enough
//! to check every byte, with every kind of table the generator writes.

use super::*;

/// A grammar of five symbols and an alias, with an external scanner, a
/// keyword lexer, reserved words and a supertype -- every table there is.
const MINI: &str = r#"#include "tree_sitter/parser.h"

#if defined(__GNUC__) || defined(__clang__)
#pragma GCC diagnostic ignored "-Wmissing-field-initializers"
#endif

#define LANGUAGE_VERSION 15
#define STATE_COUNT 4
#define LARGE_STATE_COUNT 2
#define SYMBOL_COUNT 5
#define ALIAS_COUNT 1
#define TOKEN_COUNT 3
#define EXTERNAL_TOKEN_COUNT 1
#define FIELD_COUNT 1
#define MAX_ALIAS_SEQUENCE_LENGTH 2
#define MAX_RESERVED_WORD_SET_SIZE 1
#define PRODUCTION_ID_COUNT 2
#define SUPERTYPE_COUNT 1

enum ts_symbol_identifiers {
  anon_sym_SEMI = 1,
  sym_word = 2,
  sym_document = 3,
  sym__item = 4,
  alias_sym_name = 5,
};

static const char * const ts_symbol_names[] = {
  [ts_builtin_sym_end] = "end",
  [anon_sym_SEMI] = ";",
  [sym_word] = "word",
  [sym_document] = "document",
  [sym__item] = "_item",
  [alias_sym_name] = "na\"me",
};

static const TSSymbol ts_symbol_map[] = {
  [ts_builtin_sym_end] = ts_builtin_sym_end,
  [anon_sym_SEMI] = anon_sym_SEMI,
  [sym_word] = sym_word,
  [sym_document] = sym_document,
  [sym__item] = sym__item,
  [alias_sym_name] = alias_sym_name,
};

static const TSSymbolMetadata ts_symbol_metadata[] = {
  [ts_builtin_sym_end] = {
    .visible = false,
    .named = true,
  },
  [anon_sym_SEMI] = {
    .visible = true,
    .named = false,
  },
  [sym_word] = {
    .visible = true,
    .named = true,
  },
  [sym_document] = {
    .visible = true,
    .named = true,
  },
  [sym__item] = {
    .visible = false,
    .named = true,
    .supertype = true,
  },
  [alias_sym_name] = {
    .visible = true,
    .named = true,
  },
};

enum ts_field_identifiers {
  field_value = 1,
};

static const char * const ts_field_names[] = {
  [0] = NULL,
  [field_value] = "value",
};

static const TSMapSlice ts_field_map_slices[PRODUCTION_ID_COUNT] = {
  [1] = {.index = 0, .length = 1},
};

static const TSFieldMapEntry ts_field_map_entries[] = {
  [0] =
    {field_value, 0, .inherited = true},
};

static const TSSymbol ts_alias_sequences[PRODUCTION_ID_COUNT][MAX_ALIAS_SEQUENCE_LENGTH] = {
  [0] = {0},
  [1] = {
    [1] = alias_sym_name,
  },
};

static const uint16_t ts_non_terminal_alias_map[] = {
  0,
};

static const TSStateId ts_primary_state_ids[STATE_COUNT] = {
  [0] = 0,
  [1] = 1,
  [2] = 2,
  [3] = 2,
};

static const TSSymbol ts_supertype_symbols[SUPERTYPE_COUNT] = {
  sym__item,
};

static const TSMapSlice ts_supertype_map_slices[] = {
  [sym__item] = {.index = 0, .length = 1},
};

static const TSSymbol ts_supertype_map_entries[] = {
  [0] =
    sym_word,
};

static const TSCharacterRange sym_word_character_set_1[] = {
  {'A', 'Z'}, {'a', 'z'}, {0xc0, 0x24f},
};

static bool ts_lex(TSLexer *lexer, TSStateId state) {
  START_LEXER();
  eof = lexer->eof(lexer);
  switch (state) {
    case 0:
      if (eof) ADVANCE(3);
      ADVANCE_MAP(
        ';', 1,
        '\\', 2,
      );
      if (('\t' <= lookahead && lookahead <= '\r') ||
          lookahead == ' ') SKIP(0);
      if (set_contains(sym_word_character_set_1, 3, lookahead)) ADVANCE(2);
      END_STATE();
    case 1:
      ACCEPT_TOKEN(anon_sym_SEMI);
      END_STATE();
    case 2:
      ACCEPT_TOKEN(sym_word);
      if (set_contains(sym_word_character_set_1, 3, lookahead)) ADVANCE(2);
      END_STATE();
    case 3:
      ACCEPT_TOKEN(ts_builtin_sym_end);
      END_STATE();
    default:
      return false;
  }
}

static bool ts_lex_keywords(TSLexer *lexer, TSStateId state) {
  START_LEXER();
  eof = lexer->eof(lexer);
  switch (state) {
    case 0:
      if (lookahead == 'x') ADVANCE(1);
      END_STATE();
    case 1:
      ACCEPT_TOKEN(anon_sym_SEMI);
      END_STATE();
    default:
      return false;
  }
}

static const TSLexerMode ts_lex_modes[STATE_COUNT] = {
  [0] = {.lex_state = 0, .external_lex_state = 1},
  [1] = {.lex_state = 0, .reserved_word_set_id = 1},
  [2] = {(TSStateId)(-1),},
  [3] = {.lex_state = 0},
};

static const TSSymbol ts_reserved_words[2][MAX_RESERVED_WORD_SET_SIZE] = {
  [1] = {
    anon_sym_SEMI,
  },
};

static const uint16_t ts_parse_table[LARGE_STATE_COUNT][SYMBOL_COUNT] = {
  [0] = {
    [ts_builtin_sym_end] = ACTIONS(1),
    [anon_sym_SEMI] = ACTIONS(1),
  },
  [1] = {
    [sym_document] = STATE(3),
    [sym_word] = ACTIONS(3),
  },
};

static const uint16_t ts_small_parse_table[] = {
  [0] = 2,
    ACTIONS(5), 1,
      ts_builtin_sym_end,
    ACTIONS(7), 1,
      anon_sym_SEMI,
  [7] = 1,
    ACTIONS(9), 1,
      ts_builtin_sym_end,
};

static const uint32_t ts_small_parse_table_map[] = {
  [SMALL_STATE(2)] = 0,
  [SMALL_STATE(3)] = 7,
};

static const TSParseActionEntry ts_parse_actions[] = {
  [0] = {.entry = {.count = 0, .reusable = false}},
  [1] = {.entry = {.count = 1, .reusable = false}}, RECOVER(),
  [3] = {.entry = {.count = 1, .reusable = true}}, SHIFT(2),
  [5] = {.entry = {.count = 1, .reusable = true}}, REDUCE(sym_document, 1, -1, 1),
  [7] = {.entry = {.count = 1, .reusable = true}}, SHIFT_REPEAT(3),
  [9] = {.entry = {.count = 2, .reusable = true}}, ACCEPT_INPUT(), SHIFT_EXTRA(),
};

enum ts_external_scanner_symbol_identifiers {
  ts_external_token_word = 0,
};

static const TSSymbol ts_external_scanner_symbol_map[EXTERNAL_TOKEN_COUNT] = {
  [ts_external_token_word] = sym_word,
};

static const bool ts_external_scanner_states[2][EXTERNAL_TOKEN_COUNT] = {
  [1] = {
    [ts_external_token_word] = true,
  },
};

#ifdef __cplusplus
extern "C" {
#endif
void *tree_sitter_mini_external_scanner_create(void);
void tree_sitter_mini_external_scanner_destroy(void *);
bool tree_sitter_mini_external_scanner_scan(void *, TSLexer *, const bool *);
unsigned tree_sitter_mini_external_scanner_serialize(void *, char *);
void tree_sitter_mini_external_scanner_deserialize(void *, const char *, unsigned);

#ifdef TREE_SITTER_HIDE_SYMBOLS
#define TS_PUBLIC
#elif defined(_WIN32)
#define TS_PUBLIC __declspec(dllexport)
#else
#define TS_PUBLIC __attribute__((visibility("default")))
#endif

TS_PUBLIC const TSLanguage *tree_sitter_mini(void) {
  static const TSLanguage language = {
    .abi_version = LANGUAGE_VERSION,
    .symbol_count = SYMBOL_COUNT,
    .parse_table = &ts_parse_table[0][0],
    .lex_modes = (const void*)ts_lex_modes,
    .lex_fn = ts_lex,
    .keyword_lex_fn = ts_lex_keywords,
    .keyword_capture_token = sym_word,
    .external_scanner = {
      &ts_external_scanner_states[0][0],
      ts_external_scanner_symbol_map,
      tree_sitter_mini_external_scanner_create,
      tree_sitter_mini_external_scanner_destroy,
      tree_sitter_mini_external_scanner_scan,
      tree_sitter_mini_external_scanner_serialize,
      tree_sitter_mini_external_scanner_deserialize,
    },
    .primary_state_ids = ts_primary_state_ids,
    .name = "mini",
    .max_reserved_word_set_size = 1,
    .metadata = {
      .major_version = 1,
      .minor_version = 2,
      .patch_version = 3,
    },
  };
  return &language;
}
#ifdef __cplusplus
}
#endif
"#;

fn blob<'g>(g: &'g Grammar, field: &str) -> &'g [u8] {
    &g.blobs
        .iter()
        .find(|b| b.field == field)
        .expect(field)
        .bytes
}

/// **Every table is read, and laid out as C lays out the generator's
/// structs**, byte for byte.
#[test]
fn every_table_is_laid_out_as_c_lays_it_out() {
    let g = parse(MINI).unwrap();
    assert_eq!(g.name, "mini");
    assert_eq!(g.abi, 15);
    assert_eq!(g.keyword_capture_token, 2);
    assert_eq!(g.metadata, Some([1, 2, 3]));
    assert_eq!(g.external_tokens(), ["word"]);
    assert_eq!(g.symbol_count(), 6);
    assert_eq!(g.symbol_names[5].as_deref(), Some(&b"na\"me"[..]));
    assert_eq!(g.field_names, [None, Some(b"value".to_vec())]);

    // [2][5] u16: large states by symbol.
    assert_eq!(
        blob(&g, "parse_table"),
        [
            1, 0, 1, 0, 0, 0, 0, 0, 0, 0, /**/ 0, 0, 0, 0, 3, 0, 3, 0, 0, 0
        ]
    );
    assert_eq!(
        blob(&g, "small_parse_table"),
        [
            2, 0, 5, 0, 1, 0, 0, 0, 7, 0, 1, 0, 1, 0, 1, 0, 9, 0, 1, 0, 0, 0
        ]
    );
    assert_eq!(blob(&g, "small_parse_table_map"), [0, 0, 0, 0, 7, 0, 0, 0]);
    let actions = blob(&g, "parse_actions");
    assert_eq!(actions.len(), 12 * 8);
    let action = |i: usize| &actions[i * 8..i * 8 + 8];
    assert_eq!(action(1), [1, 0, 0, 0, 0, 0, 0, 0], "an entry: count 1");
    assert_eq!(action(2), [3, 0, 0, 0, 0, 0, 0, 0], "recover");
    assert_eq!(action(3), [1, 1, 0, 0, 0, 0, 0, 0], "a reusable entry");
    assert_eq!(action(4), [0, 0, 2, 0, 0, 0, 0, 0], "shift to 2");
    assert_eq!(
        action(6),
        [1, 1, 3, 0, 0xff, 0xff, 1, 0],
        "reduce, precedence -1"
    );
    assert_eq!(action(8), [0, 0, 3, 0, 0, 1, 0, 0], "shift, repeating");
    assert_eq!(action(10), [2, 0, 0, 0, 0, 0, 0, 0], "accept");
    assert_eq!(action(11), [0, 0, 0, 0, 1, 0, 0, 0], "shift an extra");

    assert_eq!(
        blob(&g, "symbol_metadata"),
        [0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 1, 1, 0]
    );
    assert_eq!(
        blob(&g, "public_symbol_map"),
        [0, 0, 1, 0, 2, 0, 3, 0, 4, 0, 5, 0]
    );
    assert_eq!(blob(&g, "alias_sequences"), [0, 0, 0, 0, 0, 0, 5, 0]);
    assert_eq!(blob(&g, "field_map_slices"), [0, 0, 0, 0, 0, 0, 1, 0]);
    assert_eq!(blob(&g, "field_map_entries"), [1, 0, 0, 1]);
    // ABI 15: three u16s a state, the last the reserved-word set.
    assert_eq!(
        blob(&g, "lex_modes"),
        [
            0, 0, 1, 0, 0, 0, /**/ 0, 0, 0, 0, 1, 0, /**/ 0xff, 0xff, 0, 0, 0, 0,
            /**/ 0, 0, 0, 0, 0, 0
        ]
    );
    assert_eq!(blob(&g, "primary_state_ids"), [0, 0, 1, 0, 2, 0, 2, 0]);
    assert_eq!(blob(&g, "reserved_words"), [0, 0, 1, 0]);
    assert_eq!(blob(&g, "supertype_symbols"), [4, 0]);
    assert_eq!(blob(&g, "supertype_map_slices")[16..], [0, 0, 1, 0]);
    assert_eq!(blob(&g, "supertype_map_entries"), [2, 0]);
    assert_eq!(blob(&g, "external_scanner.symbol_map"), [2, 0]);
    assert_eq!(blob(&g, "external_scanner.states"), [0, 1]);
    assert_eq!(
        g.char_sets,
        [(
            "sym_word_character_set_1".to_owned(),
            vec![(65, 90), (97, 122), (0xc0, 0x24f)]
        )]
    );
}

/// **The Rust names every table, the lexers and the language**, in the terms
/// `gui/syntax`'s `ffi` module provides.
#[test]
fn the_rust_names_every_table_the_lexers_and_the_language() {
    let out = parse(MINI).unwrap().render("mini");
    let rust = &out.rust;
    for needle in [
        "static PARSE_TABLE: crate::ffi::Aligned<[u8; 20]> = crate::ffi::Aligned(*include_bytes!(concat!(env!(\"OUT_DIR\"), \"/mini/parse_table.bin\")));",
        "static EXTERNAL_SCANNER_STATES: crate::ffi::Aligned<[u8; 2]>",
        "c\"na\\\"me\".as_ptr()",
        "field_names: FIELD_NAMES.0.as_ptr(),",
        "const SYM_WORD_CHARACTER_SET_1: &[(i32, i32)] = &[(65, 90), (97, 122), (192, 591), ];",
        "fn lex_main(lexer: &mut Lexer<'_>, start: u16) -> bool {",
        "crate::ffi::lexer_entry!(ts_lex_keywords, lex_keywords);",
        "keyword_lex_fn: Some(ts_lex_keywords),",
        "keyword_capture_token: 2,",
        "external_scanner: crate::ffi::scanner_table::<Scanner>(EXTERNAL_SCANNER_STATES.0.as_ptr().cast::<bool>(), EXTERNAL_SCANNER_SYMBOL_MAP.0.as_ptr().cast::<u16>()),",
        "lex_modes: LEX_MODES.0.as_ptr().cast::<crate::ffi::LexerMode>(),",
        "name: c\"mini\".as_ptr(),",
        "max_reserved_word_set_size: 1,",
        "supertype_count: 1,",
        "metadata: crate::ffi::LanguageMetadata { major_version: 1, minor_version: 2, patch_version: 3 },",
        "pub(crate) const EXTERNAL_TOKENS: [&str; 1] = [\"word\", ];",
    ] {
        assert!(rust.contains(needle), "missing: {needle}\n\n{rust}");
    }
    let files: Vec<&str> = out.blobs.iter().map(|(f, _)| f.as_str()).collect();
    assert!(
        files.contains(&"parse_table.bin") && files.contains(&"external_scanner_states.bin"),
        "{files:?}"
    );
}

/// **An ABI-14 grammar keeps its two-field lexer modes and has no name, no
/// reserved words and no supertypes** -- the runtime reads those only from
/// version 15.
#[test]
fn an_abi_14_grammar_keeps_its_own_layout() {
    let old = MINI
        .replace("#define LANGUAGE_VERSION 15", "#define LANGUAGE_VERSION 14")
        .replace(
            "static const TSLexerMode ts_lex_modes",
            "static const TSLexMode ts_lex_modes",
        )
        .replace(
            "[1] = {.lex_state = 0, .reserved_word_set_id = 1},",
            "[1] = {.lex_state = 0},",
        );
    let g = parse(&old).unwrap();
    assert_eq!(
        blob(&g, "lex_modes"),
        [0, 0, 1, 0, 0, 0, 0, 0, 0xff, 0xff, 0, 0, 0, 0, 0, 0]
    );
    let rust = g.render("old").rust;
    assert!(rust.contains("abi_version: 14,"));
    assert!(rust.contains("name: core::ptr::null(),"));
}

/// **A grammar the converter cannot vouch for is refused**, with the reason:
/// a newer ABI, a table longer than its count, an action it does not know,
/// a count missing.
#[test]
fn a_grammar_it_cannot_vouch_for_is_refused() {
    let cases = [
        (
            MINI.replace("#define LANGUAGE_VERSION 15", "#define LANGUAGE_VERSION 16"),
            "ABI version 16",
        ),
        (
            MINI.replace("  [3] = 2,\n};", "  [3] = 2,\n  [4] = 2,\n};"),
            "element 4 of 4",
        ),
        (
            MINI.replace("SHIFT_REPEAT(3)", "SHIFT_TWICE(3)"),
            "SHIFT_TWICE",
        ),
        (MINI.replace("#define FIELD_COUNT 1\n", ""), "FIELD_COUNT"),
        (MINI.replace("ADVANCE(3);", "GOTO(3);"), "GOTO"),
    ];
    for (source, why) in cases {
        let err = parse(&source).unwrap_err();
        assert!(err.to_string().contains(why), "{why}: {err}");
    }
}
