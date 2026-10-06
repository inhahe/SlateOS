#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::super::decl::{Property, Value};
use super::*;

fn subject<'a>(kind: &'a str, classes: &'a [String], name: Option<&'a str>) -> Subject<'a> {
    Subject {
        kind,
        classes,
        name,
        enabled: true,
        ..Subject::default()
    }
}

/// The properties `declared` set, in order.
fn props(declared: &[&Declared]) -> Vec<Property> {
    declared
        .iter()
        .map(|d| match d {
            Declared::Value(p, _) => *p,
            other => panic!("{other:?}"),
        })
        .collect()
}

/// **A block is its declarations, and a state's block applies in that
/// state**, after the rest.
#[test]
fn a_block_and_its_states() {
    let (block, warnings) = parse_block(
        "color: red; &:hover { background-color: blue } padding: 1px; :focus { opacity: 0.5 }",
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(block.base.len(), 5, "color, then padding's four sides");
    assert_eq!(block.states.len(), 2);
    let mut s = subject("Button", &[], None);
    assert_eq!(block.applying(&s).count(), 5);
    s.hover = true;
    let applied: Vec<&Declared> = block.applying(&s).collect();
    assert_eq!(applied.len(), 6);
    assert_eq!(
        props(&applied).last(),
        Some(&Property::BackgroundColor),
        "the state's last"
    );
}

/// **A style sheet's rules choose widgets by kind, class, name and state**,
/// and apply in the order written.
#[test]
fn rules_choose_by_kind_class_name_and_state() {
    let (sheet, warnings) = parse_sheet(
        "Button { color: red } .danger { color: orange } #save { opacity: 0.5 } \
         Label { color: green } *:disabled { cursor: not-allowed } Button:hover { color: blue }",
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let danger = ["danger".to_string()];
    let mut button = subject("Button", &danger, Some("save"));
    let applied = sheet.applying(&[button]);
    assert_eq!(
        props(&applied),
        [Property::Color, Property::Color, Property::Opacity]
    );
    // The class's colour is later, so it is the one in force.
    let Declared::Value(_, Value::Color(_)) = applied[1] else {
        panic!()
    };
    button.hover = true;
    assert_eq!(sheet.applying(&[button]).len(), 4);
    button.enabled = false;
    assert_eq!(sheet.applying(&[button]).len(), 5);
    let label = subject("Label", &[], None);
    assert_eq!(props(&sheet.applying(&[label])), [Property::Color]);
}

/// **`>` asks for a child, and only a child**; one rule's several selectors
/// each choose.
#[test]
fn a_child_and_a_list() {
    let (sheet, warnings) = parse_sheet("Toolbar > Button, #ok { opacity: 0.5 }");
    assert!(warnings.is_empty(), "{warnings:?}");
    let toolbar = subject("Toolbar", &[], None);
    let panel = subject("Panel", &[], None);
    let button = subject("Button", &[], None);
    assert_eq!(sheet.applying(&[toolbar, button]).len(), 1);
    assert_eq!(
        sheet.applying(&[toolbar, panel, button]).len(),
        0,
        "a grandchild is no child"
    );
    assert_eq!(sheet.applying(&[button]).len(), 0);
    let ok = subject("Button", &[], Some("ok"));
    assert_eq!(sheet.applying(&[panel, ok]).len(), 1);
}

/// **What is not read is said, and the rest kept**: an unknown property, a
/// descendant combinator, `!important`, an at-rule, a sibling combinator,
/// an unknown state.
#[test]
fn what_is_not_read_is_said_and_the_rest_kept() {
    let (sheet, warnings) = parse_sheet(
        "Button { colour: red; color: blue; padding: 1px !important } \
         Toolbar Button { color: red } @media screen { Label { color: red } } \
         A + B { color: red } Button:wobbly { color: red } Label { color: green }",
    );
    let messages: Vec<&str> = warnings.iter().map(|w| w.message.as_str()).collect();
    assert_eq!(warnings.len(), 6, "{messages:?}");
    assert!(messages[0].contains("not a property"));
    assert!(messages[1].contains("!important"));
    assert!(messages[2].contains("descendant"));
    assert!(messages[3].contains("@media"));
    assert!(messages[4].contains("sibling"));
    assert!(messages[5].contains(":wobbly"));
    // The two good rules stand, the first with the one good declaration.
    assert_eq!(sheet.rules.len(), 2);
    assert_eq!(sheet.rules[0].block.base.len(), 1);
}

/// **A warning says where.**
#[test]
fn a_warning_says_where() {
    let text = "color: red;\n  wobble: 1px";
    let (_, warnings) = parse_block(text);
    assert_eq!(warnings.len(), 1);
    assert_eq!(&text[warnings[0].at..warnings[0].at + 6], "wobble");
}

/// **A nested block may only be a state's**, and only one level deep.
#[test]
fn a_nested_block_is_a_states() {
    let (_, warnings) = parse_block("Button { color: red }");
    assert_eq!(warnings.len(), 1);
    let (_, warnings) = parse_block("&:hover { &:focus { color: red } }");
    assert_eq!(warnings.len(), 1);
    let (block, warnings) = parse_block("&:hover:focus { color: red }");
    assert!(warnings.is_empty());
    assert_eq!(block.states[0].0, [State::Hover, State::Focus]);
}

/// **An empty or broken sheet is no rules, never a panic.**
#[test]
fn an_empty_or_broken_sheet_is_no_rules() {
    for text in [
        "",
        "   ",
        "}",
        "{",
        "Button {",
        "> Button {}",
        "Button > {}",
        ";",
        ".{}",
        "#",
    ] {
        let (sheet, _) = parse_sheet(text);
        assert!(sheet.rules.iter().all(|r| r.block.is_empty()), "{text}");
    }
}
