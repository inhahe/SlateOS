#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use super::*;

/// The tokens alone, without where they began.
fn kinds(input: &str) -> Vec<Token> {
    tokenize(input).into_iter().map(|s| s.token).collect()
}

fn ident(s: &str) -> Token {
    Token::Ident(s.to_string())
}

/// **A declaration comes apart into its name, its colon, its value and its
/// semicolon**, and a run of spaces is one separator.
#[test]
fn a_declaration_comes_apart() {
    assert_eq!(
        kinds("color :  red;"),
        [
            ident("color"),
            Token::Whitespace,
            Token::Colon,
            Token::Whitespace,
            ident("red"),
            Token::Semicolon,
        ]
    );
}

/// **Numbers are read as CSS writes them**: signs, fractions, exponents --
/// and a unit after one is a dimension, `%` a percentage.
#[test]
fn numbers_are_read_as_css_writes_them() {
    assert_eq!(kinds("12"), [Token::Number(12.0)]);
    assert_eq!(kinds("-0.5"), [Token::Number(-0.5)]);
    assert_eq!(kinds("+.25"), [Token::Number(0.25)]);
    assert_eq!(kinds("1e3"), [Token::Number(1000.0)]);
    assert_eq!(kinds("2E-2"), [Token::Number(0.02)]);
    assert_eq!(kinds("50%"), [Token::Percentage(50.0)]);
    assert_eq!(kinds("4px"), [Token::Dimension(4.0, "px".into())]);
    assert_eq!(kinds("1.5EM"), [Token::Dimension(1.5, "EM".into())]);
    // `1em` is not 1e and an `m`: an exponent needs its digits.
    assert_eq!(kinds("1em"), [Token::Dimension(1.0, "em".into())]);
    assert_eq!(kinds("-2px"), [Token::Dimension(-2.0, "px".into())]);
}

/// **A name may begin with a hyphen, and a custom property's with two**;
/// a hyphen before a digit is a number's sign.
#[test]
fn names_may_begin_with_hyphens() {
    assert_eq!(kinds("--gap"), [ident("--gap")]);
    assert_eq!(kinds("-moz-thing"), [ident("-moz-thing")]);
    assert_eq!(kinds("-2"), [Token::Number(-2.0)]);
    assert_eq!(
        kinds("a - b"),
        [
            ident("a"),
            Token::Whitespace,
            Token::Delim('-'),
            Token::Whitespace,
            ident("b")
        ]
    );
}

/// **A name followed by `(` is a function**, the parenthesis taken with it.
#[test]
fn a_name_and_a_parenthesis_is_a_function() {
    assert_eq!(
        kinds("rgb(1,2)"),
        [
            Token::Function("rgb".into()),
            Token::Number(1.0),
            Token::Comma,
            Token::Number(2.0),
            Token::CloseParen,
        ]
    );
    assert_eq!(
        kinds("var(--a)"),
        [
            Token::Function("var".into()),
            ident("--a"),
            Token::CloseParen
        ]
    );
}

/// **A hash is its name**; a `#` with no name after it is a delimiter.
#[test]
fn a_hash_is_its_name() {
    assert_eq!(kinds("#fff"), [Token::Hash("fff".into())]);
    assert_eq!(kinds("#save"), [Token::Hash("save".into())]);
    assert_eq!(kinds("#1a2b3c"), [Token::Hash("1a2b3c".into())]);
    assert_eq!(kinds("# "), [Token::Delim('#'), Token::Whitespace]);
}

/// **Strings keep their text, escapes resolved**, and one a line break cuts
/// off is a bad string.
#[test]
fn strings_keep_their_text() {
    assert_eq!(kinds("\"Noto Sans\""), [Token::Str("Noto Sans".into())]);
    assert_eq!(
        kinds("'it''s'"),
        [Token::Str("it".into()), Token::Str("s".into())]
    );
    assert_eq!(kinds("\"a\\\"b\""), [Token::Str("a\"b".into())]);
    assert_eq!(kinds("\"\\41 B\""), [Token::Str("AB".into())]);
    assert_eq!(
        kinds("\"cut\noff\""),
        [
            Token::BadString,
            Token::Whitespace,
            ident("off"),
            Token::Str(String::new())
        ]
    );
    assert_eq!(kinds("\"open"), [Token::Str("open".into())]);
}

/// **Comments are separators**, and one never closed runs to the end.
#[test]
fn comments_are_separators() {
    assert_eq!(
        kinds("a/* note */b"),
        [ident("a"), Token::Whitespace, ident("b")]
    );
    assert_eq!(
        kinds("a /* x */ /* y */ b"),
        [ident("a"), Token::Whitespace, ident("b")]
    );
    assert_eq!(kinds("a /* never closed"), [ident("a"), Token::Whitespace]);
}

/// **Every token says where it began**, in bytes.
#[test]
fn every_token_says_where_it_began() {
    let spans: Vec<usize> = tokenize("é: 1px").into_iter().map(|s| s.at).collect();
    assert_eq!(spans, [0, 2, 3, 4]);
}

/// **What is not CSS comes out as delimiters**, never lost and never a panic.
#[test]
fn what_is_not_css_is_delimiters() {
    assert_eq!(
        kinds("> & ! *"),
        [
            Token::Delim('>'),
            Token::Whitespace,
            Token::Delim('&'),
            Token::Whitespace,
            Token::Delim('!'),
            Token::Whitespace,
            Token::Delim('*'),
        ]
    );
    assert_eq!(kinds("\\"), [Token::Delim('\\')]);
    assert_eq!(kinds("@media"), [Token::AtKeyword("media".into())]);
    assert!(kinds("").is_empty());
}
