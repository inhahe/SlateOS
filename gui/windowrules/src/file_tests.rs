//! Tests for the rules file: what is written reads back as itself, the order
//! is the priority, and a rule that cannot be read is left out whole.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use yamldoc::Document;

use super::{CONFIG_NAME, load, read, store, write};
use crate::{
    InitialState, MAX_RULES, MatchCriteria, PositionSpec, SizeSpec, WindowRule, WindowRulesManager,
};

/// A rule's meaning, without the id and counters the file does not keep.
fn meaning(rule: &WindowRule) -> String {
    format!(
        "{} | {:?} | enabled {} | once {} | {:?}",
        rule.name, rule.criteria, rule.enabled, rule.one_shot, rule.actions
    )
}

/// One rule that says everything a rule can, and three plainer ones.
fn every_kind() -> Vec<WindowRule> {
    let mut all = WindowRule::new(0, "Everything: said", MatchCriteria::AppId("mail".into()));
    let a = &mut all.actions;
    a.position = Some(PositionSpec::Absolute { x: -20, y: 40 });
    a.size = Some(SizeSpec::Percentage {
        w_pct: 0.5,
        h_pct: 0.25,
    });
    a.desktop = Some(1);
    a.always_on_top = Some(true);
    a.always_on_bottom = Some(false);
    a.initial_state = Some(InitialState::Minimized);
    a.opacity = Some(0.75);
    a.skip_taskbar = Some(true);
    a.skip_alt_tab = Some(false);
    a.target_monitor = Some(2);
    a.no_decorations = Some(true);
    a.min_size = Some((200, 100));
    a.max_size = Some((1600, 1200));
    a.prevent_close = Some(true);
    a.prevent_move = Some(false);
    a.prevent_resize = Some(true);
    a.snap_zone = Some(7);
    a.to_tray = Some(true);
    all.one_shot = true;

    let mut centred = WindowRule::new(0, "centred", MatchCriteria::TitleExact("Notes".into()));
    centred.actions.position = Some(PositionSpec::CenterOnMonitor(1));
    centred.actions.size = Some(SizeSpec::Exact {
        width: 640,
        height: 480,
    });
    centred.enabled = false;

    let mut remembered = WindowRule::new(
        0,
        "remembered",
        MatchCriteria::TitleContains("draft".into()),
    );
    remembered.actions.position = Some(PositionSpec::RememberLast);
    remembered.actions.size = Some(SizeSpec::RememberLast);

    let mut everywhere = WindowRule::new(0, "every window", MatchCriteria::Any);
    everywhere.actions.position = Some(PositionSpec::Percentage {
        x_pct: 0.5,
        y_pct: 0.5,
    });
    vec![all, centred, remembered, everywhere]
}

fn written(rules: &[WindowRule]) -> Document {
    let mut doc = Document::new();
    write(&rules.iter().collect::<Vec<_>>(), &mut doc);
    doc
}

/// **What is written reads back as itself** -- every action, every way to
/// match, a rule turned off and a one-shot rule -- and the order it was
/// written in is its priority, highest first.
#[test]
fn what_is_written_reads_back_as_itself() {
    let rules = every_kind();
    let doc = written(&rules);
    let loaded = read(&doc).unwrap();
    assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
    let want: Vec<String> = rules.iter().map(meaning).collect();
    let got: Vec<String> = loaded.rules.iter().map(meaning).collect();
    assert_eq!(got, want, "\n{}", doc.to_text());
    let priorities: Vec<i32> = loaded.rules.iter().map(|r| r.priority).collect();
    assert!(priorities.windows(2).all(|p| p[0] > p[1]), "{priorities:?}");
    // And it reads as a person would write it, counting from one.
    let text = doc.to_text();
    for line in [
        "  \"Everything: said\":",
        "    app: mail",
        "    desktop: 2",
        "    monitor: 3",
        "    taskbar: false",
        "    can-close: false",
        "    tray: true",
        "    once: true",
        "    position: centre 2",
        "    enabled: false",
        "    position: remember",
        "    any: true",
        "    position: 50% 50%",
    ] {
        assert!(text.contains(line), "{line:?} in\n{text}");
    }
}

/// **The rule nearest the top wins**: loaded into the engine, the first of
/// two rules that match a window and disagree is the one applied.
#[test]
fn the_rule_nearest_the_top_wins() {
    let doc = Document::parse(
        "rules:\n  first:\n    app: mail\n    desktop: 1\n  second:\n    app: mail\n    desktop: 3\n",
    );
    let loaded = read(&doc).unwrap();
    let mut engine = WindowRulesManager::empty();
    assert_eq!(engine.replace_rules(loaded.rules), 0);
    assert_eq!(engine.evaluate("Inbox", "mail").desktop, Some(0));
}

/// **A rule that cannot be read is left out whole, and says why** -- a
/// misspelt key, a value out of range, a match that is missing, doubled or
/// empty, a name used twice -- and the rules around it are read as usual. A
/// misspelt match is never read as matching everything.
#[test]
fn a_rule_that_cannot_be_read_is_left_out_whole() {
    let doc = Document::parse(
        "rules:\n\
         \x20 good:\n    app: mail\n    taskbar: false\n\
         \x20 misspelt key:\n    app: mail\n    can-clos: false\n\
         \x20 misspelt match:\n    ap: mail\n    can-close: false\n\
         \x20 two matches:\n    app: mail\n    title: Inbox\n\
         \x20 empty match:\n    app: ''\n\
         \x20 desktop nought:\n    app: mail\n    desktop: 0\n\
         \x20 too opaque:\n    app: mail\n    opacity: 1.5\n\
         \x20 strange state:\n    app: mail\n    state: hidden\n\
         \x20 not a flag:\n    app: mail\n    can-close: flase\n\
         \x20 bad position:\n    app: mail\n    position: left\n\
         \x20 bad size:\n    app: mail\n    size: 5 6 7\n\
         \x20 any false:\n    any: false\n\
         \x20 good:\n    app: notes\n\
         \x20 last:\n    title-contains: draft\n",
    );
    let loaded = read(&doc).unwrap();
    let names: Vec<&str> = loaded.rules.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["good", "last"]);
    let refused: Vec<&str> = loaded.problems.iter().map(|p| p.rule.as_str()).collect();
    assert_eq!(
        refused,
        [
            "misspelt key",
            "misspelt match",
            "two matches",
            "empty match",
            "desktop nought",
            "too opaque",
            "strange state",
            "not a flag",
            "bad position",
            "bad size",
            "any false",
            "good",
        ]
    );
    // Each says what, in words.
    let said = |rule: &str| {
        loaded
            .problems
            .iter()
            .find(|p| p.rule == rule)
            .map(ToString::to_string)
            .unwrap()
    };
    assert!(said("misspelt key").contains("`can-clos`"));
    assert!(said("misspelt match").contains("says nothing to match"));
    assert!(said("good").contains("same name"));
    assert!(said("desktop nought").contains("counts from 1"));
    // Nothing read the misspelt match as `any`.
    assert!(
        loaded
            .rules
            .iter()
            .all(|r| r.criteria != MatchCriteria::Any)
    );
}

/// **No more rules than the engine keeps are read**, and the rest are each
/// a problem rather than silently gone.
#[test]
fn no_more_rules_than_the_engine_keeps() {
    let mut text = String::from("rules:\n");
    for n in 0..=MAX_RULES {
        text.push_str(&format!("  rule {n}:\n    app: app{n}\n"));
    }
    let loaded = read(&Document::parse(&text)).unwrap();
    assert_eq!(loaded.rules.len(), MAX_RULES);
    assert_eq!(loaded.problems.len(), 1);
    assert_eq!(loaded.problems[0].rule, format!("rule {MAX_RULES}"));
}

/// **No file is the defaults; a file's rules are the whole of a user's
/// rules**, and storing keeps what else the file says -- a comment above all.
#[test]
fn no_file_is_the_defaults_and_storing_keeps_the_rest() {
    settingsfile::testing::with_scratch_config("windowrules-file", |root| {
        assert!(load().is_none());
        let rules = every_kind();
        store(&rules.iter().collect::<Vec<_>>()).unwrap();
        let path = settingsfile::path_for(CONFIG_NAME).unwrap();
        assert!(path.starts_with(root));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# Window rules:"), "{text}");
        let loaded = load().unwrap();
        assert_eq!(loaded.rules.len(), rules.len());

        // A comment the user adds survives the next store, and the rules are
        // replaced, not added to.
        std::fs::write(&path, format!("# mine\n{text}")).unwrap();
        let one = &rules[1..2];
        store(&one.iter().collect::<Vec<_>>()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# mine\n"), "{text}");
        let loaded = load().unwrap();
        assert_eq!(
            loaded.rules.iter().map(meaning).collect::<Vec<_>>(),
            [meaning(&rules[1])]
        );

        // Storing no rules is a user with none -- not the defaults back --
        // and still keeps the comment.
        store(&[]).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# mine\n"), "{text}");
        let loaded = load().unwrap();
        assert!(loaded.rules.is_empty() && loaded.problems.is_empty());

        // A file of comments alone says nothing about rules: the defaults.
        std::fs::write(&path, "# none yet\n").unwrap();
        assert!(load().is_none());
        // And storing into it puts the rules below the comment.
        store(&one.iter().collect::<Vec<_>>()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# none yet\nrules:\n"), "{text}");
    });
}

/// **A document that says nothing about rules is the defaults; a `rules` key
/// with nothing under it is none**, and one holding something other than
/// named rules is a problem and no rules -- never the defaults, which would
/// be guessing what the user meant.
#[test]
fn the_rules_key_decides_between_the_defaults_and_none() {
    for nothing in ["", "# a comment\n", "other: 1\n"] {
        assert!(read(&Document::parse(nothing)).is_none(), "{nothing:?}");
    }
    for none in [
        "rules:\n",
        "rules: ~\n",
        "rules: null\n",
        "rules: {}\n",
        "# x\nrules:\n# y\n",
    ] {
        let loaded = read(&Document::parse(none)).unwrap();
        assert!(loaded.rules.is_empty(), "{none:?}");
        assert!(
            loaded.problems.is_empty(),
            "{none:?}: {:?}",
            loaded.problems
        );
    }
    for wrong in ["rules: none\n", "rules:\n  - app: mail\n"] {
        let loaded = read(&Document::parse(wrong)).unwrap();
        assert!(loaded.rules.is_empty(), "{wrong:?}");
        assert_eq!(loaded.problems.len(), 1, "{wrong:?}");
        let said = loaded.problems[0].to_string();
        assert!(said.starts_with("no window rule is applied: "), "{said}");
    }
}

/// **Writing no rules reads back as none** wherever the document started --
/// with no key, with an empty one, or with one holding something that is not
/// rules -- and writing rules over a list replaces it.
#[test]
fn writing_no_rules_reads_back_as_none() {
    for start in [
        "",
        "# Window rules\n",
        "rules:\n",
        "rules: none\n",
        "rules:\n  - app: mail\n",
        "rules:\n  old:\n    app: mail\n",
    ] {
        let mut doc = Document::parse(start);
        write(&[], &mut doc);
        let loaded = read(&doc).unwrap_or_else(|| panic!("{start:?} became the defaults"));
        assert!(
            loaded.rules.is_empty() && loaded.problems.is_empty(),
            "{start:?} -> {:?}",
            doc.to_text()
        );

        let rules = every_kind();
        let mut doc = Document::parse(start);
        write(&rules.iter().collect::<Vec<_>>(), &mut doc);
        let loaded = read(&doc).unwrap();
        assert!(
            loaded.problems.is_empty(),
            "{start:?}: {:?}",
            loaded.problems
        );
        assert_eq!(
            loaded.rules.iter().map(meaning).collect::<Vec<_>>(),
            rules.iter().map(meaning).collect::<Vec<_>>(),
            "{start:?} -> {}",
            doc.to_text()
        );
    }
}
