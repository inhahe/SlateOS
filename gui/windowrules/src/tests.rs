//! Tests for the rules and the engine: matching, merging by priority,
//! counting and one-shot rules, remembered geometry, and the caps.

// A test module's job is to fail loudly the instant the code under test is
// wrong, so the defensive lints that forbid exactly that in production code
// are off here — as `CLAUDE.md` prescribes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::*;

#[test]
fn test_title_exact_match() {
    let c = MatchCriteria::TitleExact("Firefox".to_string());
    assert!(c.matches("Firefox", ""));
    assert!(!c.matches("firefox", ""));
    assert!(!c.matches("Firefox Browser", ""));
}

#[test]
fn test_title_contains_case_insensitive() {
    let c = MatchCriteria::TitleContains("fire".to_string());
    assert!(c.matches("Firefox", ""));
    assert!(c.matches("FIREFOX", ""));
    assert!(c.matches("On Fire!", ""));
    assert!(!c.matches("Chrome", ""));
}

#[test]
fn test_app_id_match() {
    let c = MatchCriteria::AppId("terminal".to_string());
    assert!(c.matches("", "terminal"));
    assert!(c.matches("", "TERMINAL"));
    assert!(c.matches("", "Terminal"));
    assert!(!c.matches("", "term"));
}

/// A window that declines to name its program matches no app rule, and a
/// rule that names no program matches no window.
///
/// Both halves are the same mistake seen from opposite ends: an unnamed
/// program is not *every* program. The second half is reachable from the
/// settings panel — pressing Save with the value box untouched writes a
/// rule whose id is the empty string — and if that rule matched every
/// anonymous window, one stray click would apply it to half the desktop.
#[test]
fn nothing_is_matched_by_the_absence_of_a_name() {
    let named = MatchCriteria::AppId("terminal".to_string());
    assert!(
        !named.matches("Terminal", ""),
        "an unnamed window is not every program"
    );

    let unnamed = MatchCriteria::AppId(String::new());
    assert!(
        !unnamed.matches("Terminal", "terminal"),
        "an unnamed rule names no program"
    );
    assert!(
        !unnamed.matches("", ""),
        "least of all when neither is named"
    );
}

/// The title is a different question, and must not answer this one.
///
/// A title says *which document*; an app id says *which program*. A rule
/// about the program must not fire because the words happened to appear in
/// the window's caption.
#[test]
fn an_app_rule_does_not_read_the_title() {
    let c = MatchCriteria::AppId("editor".to_string());
    assert!(!c.matches("editor", "notepad"));
    assert!(c.matches("editor", "editor"));
}

#[test]
fn test_any_matches_everything() {
    let c = MatchCriteria::Any;
    assert!(c.matches("anything", "any"));
    assert!(c.matches("", ""));
}

#[test]
fn test_criteria_description() {
    assert_eq!(
        MatchCriteria::TitleExact("foo".to_string()).description(),
        "Title = \"foo\""
    );
    assert_eq!(
        MatchCriteria::AppId("bar".to_string()).description(),
        "App: bar"
    );
    assert_eq!(MatchCriteria::Any.description(), "Any window");
}

#[test]
fn test_empty_actions() {
    let a = RuleActions::new();
    assert_eq!(a.active_count(), 0);
}

#[test]
fn test_actions_count() {
    let mut a = RuleActions::new();
    a.position = Some(PositionSpec::CenterOnMonitor(0));
    a.always_on_top = Some(true);
    a.opacity = Some(0.8);
    assert_eq!(a.active_count(), 3);
}

#[test]
fn test_actions_merge() {
    let mut base = RuleActions::new();
    base.position = Some(PositionSpec::CenterOnMonitor(0));
    base.opacity = Some(0.5);

    let mut overlay = RuleActions::new();
    overlay.opacity = Some(0.9);
    overlay.always_on_top = Some(true);

    base.merge(&overlay);
    assert_eq!(base.opacity, Some(0.9)); // overridden
    assert_eq!(base.always_on_top, Some(true)); // added
    assert!(base.position.is_some()); // preserved
}

#[test]
fn test_merge_does_not_clear() {
    let mut base = RuleActions::new();
    base.desktop = Some(2);
    let empty = RuleActions::new();
    base.merge(&empty);
    assert_eq!(base.desktop, Some(2)); // Not cleared by empty merge
}

#[test]
fn test_rule_matches_when_enabled() {
    let r = WindowRule::new(1, "test", MatchCriteria::AppId("vim".to_string()));
    assert!(r.matches("", "vim"));
}

#[test]
fn test_rule_does_not_match_when_disabled() {
    let mut r = WindowRule::new(1, "test", MatchCriteria::AppId("vim".to_string()));
    r.enabled = false;
    assert!(!r.matches("", "vim"));
}

#[test]
fn test_manager_default_rules() {
    let mgr = WindowRulesManager::new();
    assert!(mgr.total_rule_count() >= 3);
    assert_eq!(mgr.active_rule_count(), mgr.total_rule_count());
}

#[test]
fn test_add_rule() {
    let mut mgr = WindowRulesManager::new();
    let initial = mgr.total_rule_count();
    let rule = WindowRule::new(0, "new rule", MatchCriteria::Any);
    let id = mgr.add_rule(rule);
    assert!(id.is_some());
    assert_eq!(mgr.total_rule_count(), initial + 1);
}

#[test]
fn test_remove_rule() {
    let mut mgr = WindowRulesManager::new();
    let rule = WindowRule::new(0, "temp", MatchCriteria::Any);
    let id = mgr.add_rule(rule).unwrap();
    let before = mgr.total_rule_count();
    assert!(mgr.remove_rule(id));
    assert_eq!(mgr.total_rule_count(), before - 1);
}

#[test]
fn test_remove_nonexistent() {
    let mut mgr = WindowRulesManager::new();
    assert!(!mgr.remove_rule(9999));
}

#[test]
fn test_enable_disable() {
    let mut mgr = WindowRulesManager::new();
    let rule = WindowRule::new(0, "toggle", MatchCriteria::Any);
    let id = mgr.add_rule(rule).unwrap();
    assert!(mgr.set_enabled(id, false));
    assert!(!mgr.rule_by_id(id).unwrap().enabled);
    assert!(mgr.set_enabled(id, true));
    assert!(mgr.rule_by_id(id).unwrap().enabled);
}

#[test]
fn test_evaluate_first_match() {
    let mut mgr = WindowRulesManager::new();
    mgr.rules.clear();
    mgr.set_eval_mode(EvalMode::FirstMatch);

    let mut r1 = WindowRule::new(0, "high", MatchCriteria::Any);
    r1.priority = 100;
    r1.actions.opacity = Some(0.5);
    mgr.add_rule(r1);

    let mut r2 = WindowRule::new(0, "low", MatchCriteria::Any);
    r2.priority = 1;
    r2.actions.opacity = Some(0.9);
    r2.actions.always_on_top = Some(true);
    mgr.add_rule(r2);

    let result = mgr.evaluate("any", "any");
    // First match (highest priority) wins.
    assert_eq!(result.opacity, Some(0.5));
    assert_eq!(result.always_on_top, None); // low-priority rule not applied
}

#[test]
fn test_evaluate_merge_all() {
    let mut mgr = WindowRulesManager::new();
    mgr.rules.clear();
    mgr.set_eval_mode(EvalMode::MergeAll);

    let mut r1 = WindowRule::new(0, "high", MatchCriteria::Any);
    r1.priority = 100;
    r1.actions.opacity = Some(0.5);
    mgr.add_rule(r1);

    let mut r2 = WindowRule::new(0, "low", MatchCriteria::Any);
    r2.priority = 1;
    r2.actions.always_on_top = Some(true);
    mgr.add_rule(r2);

    let result = mgr.evaluate("any", "any");
    // Both rules contribute. The two set disjoint fields, so this says
    // nothing about who wins a contested one — see
    // `the_higher_priority_rule_wins_a_field_both_rules_set` for that.
    assert_eq!(result.opacity, Some(0.5));
    assert_eq!(result.always_on_top, Some(true));
}

#[test]
fn test_evaluate_no_match() {
    let mut mgr = WindowRulesManager::new();
    mgr.rules.clear();
    let mut r = WindowRule::new(0, "specific", MatchCriteria::AppId("firefox".to_string()));
    r.actions.opacity = Some(0.5);
    mgr.add_rule(r);

    let result = mgr.evaluate("", "chrome");
    assert_eq!(result.active_count(), 0);
}

#[test]
fn test_one_shot_removal() {
    let mut mgr = WindowRulesManager::new();
    mgr.rules.clear();
    let mut r = WindowRule::new(0, "once", MatchCriteria::Any);
    r.one_shot = true;
    r.actions.always_on_top = Some(true);
    let id = mgr.add_rule(r).unwrap();

    let result = mgr.evaluate("x", "y");
    assert_eq!(result.always_on_top, Some(true));

    // Rule should be removed after one-shot.
    assert!(mgr.rule_by_id(id).is_none());
    assert_eq!(mgr.total_rule_count(), 0);
}

#[test]
fn test_match_count_incremented() {
    let mut mgr = WindowRulesManager::new();
    mgr.rules.clear();
    let r = WindowRule::new(0, "counter", MatchCriteria::Any);
    let id = mgr.add_rule(r).unwrap();

    mgr.evaluate("x", "y");
    mgr.evaluate("a", "b");

    assert_eq!(mgr.rule_by_id(id).unwrap().match_count, 2);
}

#[test]
fn test_remember_state() {
    let mut mgr = WindowRulesManager::new();
    mgr.remember_state("terminal", 100, 200, 800, 600);

    // Create a rule that uses RememberLast.
    mgr.rules.clear();
    let mut r = WindowRule::new(0, "term", MatchCriteria::AppId("terminal".to_string()));
    r.actions.position = Some(PositionSpec::RememberLast);
    r.actions.size = Some(SizeSpec::RememberLast);
    mgr.add_rule(r);

    let result = mgr.evaluate("", "terminal");
    assert_eq!(
        result.position,
        Some(PositionSpec::Absolute { x: 100, y: 200 })
    );
    assert_eq!(
        result.size,
        Some(SizeSpec::Exact {
            width: 800,
            height: 600
        })
    );
}

#[test]
fn test_remember_state_updates() {
    let mut mgr = WindowRulesManager::new();
    mgr.remember_state("vim", 10, 20, 100, 100);
    mgr.remember_state("vim", 50, 60, 200, 300);

    mgr.rules.clear();
    let mut r = WindowRule::new(0, "vim", MatchCriteria::AppId("vim".to_string()));
    r.actions.position = Some(PositionSpec::RememberLast);
    mgr.add_rule(r);

    let result = mgr.evaluate("", "vim");
    assert_eq!(
        result.position,
        Some(PositionSpec::Absolute { x: 50, y: 60 })
    );
}

#[test]
fn test_remember_no_state_returns_none() {
    let mut mgr = WindowRulesManager::new();
    mgr.rules.clear();
    let mut r = WindowRule::new(0, "unknown", MatchCriteria::AppId("unknown".to_string()));
    r.actions.position = Some(PositionSpec::RememberLast);
    mgr.add_rule(r);

    let result = mgr.evaluate("", "unknown");
    assert_eq!(result.position, None); // No remembered state, cleared to None
}

#[test]
fn test_remember_eviction() {
    let mut mgr = WindowRulesManager::new();
    // Fill to capacity.
    for i in 0..MAX_REMEMBERED {
        mgr.remember_state(&format!("app{i}"), 0, 0, 100, 100);
    }
    // One more should evict the oldest.
    mgr.remember_state("newest", 999, 999, 999, 999);
    assert!(mgr.remembered.len() <= MAX_REMEMBERED);
}

#[test]
fn test_priority_change() {
    let mut mgr = WindowRulesManager::new();
    let r = WindowRule::new(0, "pr", MatchCriteria::Any);
    let id = mgr.add_rule(r).unwrap();

    assert!(mgr.increase_priority(id));
    assert_eq!(mgr.rule_by_id(id).unwrap().priority, 1);

    assert!(mgr.decrease_priority(id));
    assert_eq!(mgr.rule_by_id(id).unwrap().priority, 0);
}

#[test]
fn test_duplicate_rule() {
    let mut mgr = WindowRulesManager::new();
    let mut r = WindowRule::new(0, "original", MatchCriteria::Any);
    r.actions.opacity = Some(0.7);
    r.match_count = 42;
    let id = mgr.add_rule(r).unwrap();

    let dup_id = mgr.duplicate_rule(id).unwrap();
    let dup = mgr.rule_by_id(dup_id).unwrap();
    assert_eq!(dup.name, "original (copy)");
    assert_eq!(dup.actions.opacity, Some(0.7));
    assert_eq!(dup.match_count, 0); // Reset
}

#[test]
fn test_rules_sorted_by_priority() {
    let mut mgr = WindowRulesManager::new();
    mgr.rules.clear();

    let mut r1 = WindowRule::new(0, "low", MatchCriteria::Any);
    r1.priority = 1;
    mgr.add_rule(r1);

    let mut r2 = WindowRule::new(0, "high", MatchCriteria::Any);
    r2.priority = 100;
    mgr.add_rule(r2);

    let mut r3 = WindowRule::new(0, "mid", MatchCriteria::Any);
    r3.priority = 50;
    mgr.add_rule(r3);

    let sorted = mgr.rules();
    assert_eq!(sorted[0].name, "high");
    assert_eq!(sorted[1].name, "mid");
    assert_eq!(sorted[2].name, "low");
}

#[test]
fn test_eval_mode_switch() {
    let mut mgr = WindowRulesManager::new();
    assert_eq!(mgr.eval_mode(), EvalMode::FirstMatch);
    mgr.set_eval_mode(EvalMode::MergeAll);
    assert_eq!(mgr.eval_mode(), EvalMode::MergeAll);
}

#[test]
fn test_remember_empty_key_ignored() {
    let mut mgr = WindowRulesManager::new();
    mgr.remember_state("", 100, 200, 800, 600);
    assert!(mgr.remembered.is_empty());
}

#[test]
fn test_rule_by_id_mut() {
    let mut mgr = WindowRulesManager::new();
    let r = WindowRule::new(0, "mutable", MatchCriteria::Any);
    let id = mgr.add_rule(r).unwrap();
    mgr.rule_by_id_mut(id).unwrap().name = "changed".to_string();
    assert_eq!(mgr.rule_by_id(id).unwrap().name, "changed");
}

#[test]
fn test_action_summary() {
    let mut a = RuleActions::new();
    assert_eq!(a.summary(), "");
    a.always_on_top = Some(true);
    a.opacity = Some(0.5);
    let s = a.summary();
    assert!(s.contains("on-top"));
    assert!(s.contains("opacity"));
}

#[test]
fn test_max_rules_cap() {
    let mut mgr = WindowRulesManager::new();
    mgr.rules.clear();
    for i in 0..MAX_RULES {
        let r = WindowRule::new(0, &format!("rule{}", i), MatchCriteria::Any);
        assert!(mgr.add_rule(r).is_some());
    }
    // One more should fail.
    let r = WindowRule::new(0, "overflow", MatchCriteria::Any);
    assert!(mgr.add_rule(r).is_none());
}

#[test]
fn test_position_spec_variants() {
    let abs = PositionSpec::Absolute { x: 100, y: 200 };
    let center = PositionSpec::CenterOnMonitor(1);
    let pct = PositionSpec::Percentage {
        x_pct: 0.5,
        y_pct: 0.5,
    };
    let rem = PositionSpec::RememberLast;
    // Just ensure they're distinct.
    assert_ne!(abs, center);
    assert_ne!(pct, rem);
}

#[test]
fn test_size_spec_variants() {
    let exact = SizeSpec::Exact {
        width: 800,
        height: 600,
    };
    let pct = SizeSpec::Percentage {
        w_pct: 0.5,
        h_pct: 0.5,
    };
    let rem = SizeSpec::RememberLast;
    assert_ne!(exact, pct);
    assert_ne!(pct, rem);
}

#[test]
fn test_initial_state_variants() {
    assert_ne!(InitialState::Normal, InitialState::Maximized);
    assert_ne!(InitialState::Minimized, InitialState::Fullscreen);
}

#[test]
fn test_default_trait_impls() {
    let _ = RuleActions::default();
    let _ = WindowRulesManager::default();
    // The defaults are the rules a user has before writing any; `empty` has
    // none at all.
    assert!(WindowRulesManager::default().total_rule_count() > 0);
    assert_eq!(WindowRulesManager::empty().total_rule_count(), 0);
}

/// **A user's rules replace every rule held, the defaults included**, each
/// with a fresh id -- and geometry remembered for "remember last" survives,
/// as it describes windows, not rules. Past the cap, the rest are counted.
#[test]
fn a_users_rules_replace_every_rule_held() {
    let mut mgr = WindowRulesManager::new();
    mgr.remember_state("terminal", 10, 20, 300, 200);
    let mut mine = WindowRule::new(0, "mine", MatchCriteria::AppId("terminal".into()));
    mine.actions.position = Some(PositionSpec::RememberLast);
    assert_eq!(mgr.replace_rules(vec![mine.clone(), mine]), 0);
    assert_eq!(mgr.total_rule_count(), 2);
    let ids: Vec<RuleId> = mgr.rules().iter().map(|r| r.id).collect();
    assert_ne!(ids[0], ids[1]);
    assert_eq!(
        mgr.evaluate("Terminal", "terminal").position,
        Some(PositionSpec::Absolute { x: 10, y: 20 })
    );
    let flood: Vec<WindowRule> = (0..MAX_RULES + 3)
        .map(|n| WindowRule::new(0, &format!("r{n}"), MatchCriteria::Any))
        .collect();
    assert_eq!(mgr.replace_rules(flood), 3);
    assert_eq!(mgr.total_rule_count(), MAX_RULES);
}

/// **A rule read again unchanged keeps what it has done**: its count, and --
/// for a once-rule already used -- being used. The file is read again on
/// every change to it, and most changes are about other rules; moving a rule
/// or turning it off and on is not changing it. Changing what it matches or
/// does is, and so is deleting it and writing it again.
#[test]
fn a_rule_read_again_unchanged_keeps_what_it_has_done() {
    let mut mgr = WindowRulesManager::empty();
    let mut once = WindowRule::new(0, "first terminal", MatchCriteria::AppId("terminal".into()));
    once.one_shot = true;
    once.actions.desktop = Some(1);
    let mut mail = WindowRule::new(0, "mail", MatchCriteria::AppId("mail".into()));
    mail.actions.skip_taskbar = Some(true);
    assert_eq!(mgr.replace_rules(vec![once.clone(), mail.clone()]), 0);
    assert_eq!(mgr.evaluate("Terminal", "terminal").desktop, Some(1));
    assert_eq!(mgr.evaluate("Terminal", "terminal").desktop, None);
    for _ in 0..2 {
        assert_eq!(mgr.evaluate("Inbox", "mail").skip_taskbar, Some(true));
    }
    let count = |mgr: &WindowRulesManager, name: &str| {
        mgr.rules()
            .iter()
            .find(|r| r.name == name)
            .map(|r| r.match_count)
    };

    // Another rule added above them; mail moved down; the once-rule turned
    // off and then on again.
    let notes = WindowRule::new(0, "notes", MatchCriteria::AppId("notes".into()));
    let mut moved = mail.clone();
    moved.priority = -5;
    let mut off = once.clone();
    off.enabled = false;
    mgr.replace_rules(vec![notes.clone(), moved.clone(), off]);
    mgr.replace_rules(vec![notes.clone(), moved.clone(), once.clone()]);
    assert_eq!(
        mgr.evaluate("Terminal", "terminal").desktop,
        None,
        "a used once-rule was armed again"
    );
    assert_eq!(
        count(&mgr, "mail"),
        Some(2),
        "an unchanged rule lost its count"
    );
    assert_eq!(count(&mgr, "first terminal"), None);

    // Changed, a rule is a new one: its count starts again, and a once-rule
    // fires once more.
    let mut changed_mail = mail.clone();
    changed_mail.actions.opacity = Some(0.5);
    let mut changed_once = once.clone();
    changed_once.actions.desktop = Some(2);
    mgr.replace_rules(vec![changed_mail, changed_once.clone()]);
    assert_eq!(count(&mgr, "mail"), Some(0));
    assert_eq!(mgr.evaluate("Terminal", "terminal").desktop, Some(2));
    assert_eq!(mgr.evaluate("Terminal", "terminal").desktop, None);

    // Deleted and written again, it is armed again.
    mgr.replace_rules(Vec::new());
    mgr.replace_rules(vec![changed_once]);
    assert_eq!(mgr.evaluate("Terminal", "terminal").desktop, Some(2));
}

/// **The defaults are what a new manager holds**, in the order
/// [`default_rules`] promises: highest priority first.
#[test]
fn the_defaults_are_what_a_new_manager_holds() {
    let mgr = WindowRulesManager::new();
    let held: Vec<String> = mgr.rules().iter().map(|r| r.name.clone()).collect();
    let defaults: Vec<String> = default_rules().into_iter().map(|r| r.name).collect();
    assert_eq!(held, defaults);
    assert!(default_rules().iter().all(|r| r.id == 0));
}

/// **Used once-rules are never held past the cap**, however many are added
/// one at a time between reads of the file.
#[test]
fn used_once_rules_are_capped() {
    let mut mgr = WindowRulesManager::empty();
    for n in 0..MAX_RULES + 5 {
        let mut rule = WindowRule::new(0, &format!("r{n}"), MatchCriteria::Any);
        rule.one_shot = true;
        mgr.add_rule(rule);
        let _ = mgr.evaluate("t", "a");
    }
    assert_eq!(mgr.spent.len(), MAX_RULES);
    assert_eq!(mgr.spent[0].name, "r5", "the oldest go first");
}

#[test]
fn the_higher_priority_rule_wins_a_field_both_rules_set() {
    // This is the case the old merge test left untested, and the case the
    // old implementation got backwards: it merged from the top of the
    // priority order downwards, and `merge` lets the incoming side win, so
    // the *last* rule merged — the least important one — decided every
    // field the two disagreed about.
    let mut mgr = WindowRulesManager::new();
    mgr.rules.clear();
    mgr.set_eval_mode(EvalMode::MergeAll);

    let mut low = WindowRule::new(0, "low", MatchCriteria::Any);
    low.priority = 1;
    low.actions.opacity = Some(0.9);
    low.actions.desktop = Some(7);
    mgr.add_rule(low);

    let mut high = WindowRule::new(0, "high", MatchCriteria::Any);
    high.priority = 100;
    high.actions.opacity = Some(0.5);
    mgr.add_rule(high);

    let result = mgr.evaluate("any", "any");
    assert_eq!(result.opacity, Some(0.5), "the priority-100 rule must win");
    // A field only the low-priority rule mentions still applies: `None`
    // means "no opinion", not "off".
    assert_eq!(result.desktop, Some(7));
}

#[test]
fn priority_order_does_not_depend_on_the_order_rules_were_added() {
    // Same two rules, added the other way round. A merge that happened to
    // read as "last one added wins" would pass one of these and fail the
    // other.
    for high_first in [true, false] {
        let mut mgr = WindowRulesManager::new();
        mgr.rules.clear();
        mgr.set_eval_mode(EvalMode::MergeAll);

        let mut low = WindowRule::new(0, "low", MatchCriteria::Any);
        low.priority = 1;
        low.actions.desktop = Some(1);
        let mut high = WindowRule::new(0, "high", MatchCriteria::Any);
        high.priority = 100;
        high.actions.desktop = Some(100);

        if high_first {
            mgr.add_rule(high);
            mgr.add_rule(low);
        } else {
            mgr.add_rule(low);
            mgr.add_rule(high);
        }
        assert_eq!(mgr.evaluate("a", "b").desktop, Some(100));
    }
}

#[test]
fn the_two_modes_break_a_priority_tie_the_same_way() {
    // Equal priorities are ordered by insertion, so the earlier-added rule
    // sorts first — which is the rule `FirstMatch` picks, and the rule the
    // settings list shows at the top. `MergeAll` therefore has to let that
    // same rule win a contested field, or the two modes would disagree
    // about which of two identical-priority rules is the authoritative
    // one, and the list would be showing the wrong order for one of them.
    for mode in [EvalMode::FirstMatch, EvalMode::MergeAll] {
        let mut mgr = WindowRulesManager::new();
        mgr.rules.clear();
        mgr.set_eval_mode(mode);

        let mut first = WindowRule::new(0, "first", MatchCriteria::Any);
        first.actions.desktop = Some(1);
        mgr.add_rule(first);
        let mut second = WindowRule::new(0, "second", MatchCriteria::Any);
        second.actions.desktop = Some(2);
        mgr.add_rule(second);

        assert_eq!(
            mgr.evaluate("a", "b").desktop,
            Some(1),
            "{mode:?} should defer to the earlier-added rule"
        );
        assert_eq!(
            mgr.rules().first().map(|r| r.name.clone()),
            Some("first".to_string()),
            "and the settings list should show it first"
        );
    }
}

#[test]
fn first_match_counts_only_the_rule_that_fired() {
    let mut mgr = WindowRulesManager::new();
    mgr.rules.clear();
    mgr.set_eval_mode(EvalMode::FirstMatch);

    let mut high = WindowRule::new(0, "high", MatchCriteria::Any);
    high.priority = 100;
    let high_id = mgr.add_rule(high).unwrap();
    let mut low = WindowRule::new(0, "low", MatchCriteria::Any);
    low.priority = 1;
    let low_id = mgr.add_rule(low).unwrap();

    mgr.evaluate("a", "b");
    assert_eq!(mgr.rule_by_id(high_id).unwrap().match_count, 1);
    assert_eq!(
        mgr.rule_by_id(low_id).unwrap().match_count,
        0,
        "a rule that never applied has not been hit"
    );
}

#[test]
fn merge_all_counts_every_rule_that_applied() {
    let mut mgr = WindowRulesManager::new();
    mgr.rules.clear();
    mgr.set_eval_mode(EvalMode::MergeAll);
    let a = mgr
        .add_rule(WindowRule::new(0, "a", MatchCriteria::Any))
        .unwrap();
    let b = mgr
        .add_rule(WindowRule::new(0, "b", MatchCriteria::Any))
        .unwrap();

    mgr.evaluate("x", "y");
    assert_eq!(mgr.rule_by_id(a).unwrap().match_count, 1);
    assert_eq!(mgr.rule_by_id(b).unwrap().match_count, 1);
}

#[test]
fn a_one_shot_rule_that_did_not_apply_survives_first_match() {
    // In FirstMatch mode only the winner fires, so a lower-priority
    // one-shot rule must still be there next time.
    let mut mgr = WindowRulesManager::new();
    mgr.rules.clear();
    mgr.set_eval_mode(EvalMode::FirstMatch);

    let mut high = WindowRule::new(0, "high", MatchCriteria::Any);
    high.priority = 100;
    mgr.add_rule(high);
    let mut once = WindowRule::new(0, "once", MatchCriteria::Any);
    once.priority = 1;
    once.one_shot = true;
    let once_id = mgr.add_rule(once).unwrap();

    mgr.evaluate("a", "b");
    assert!(mgr.rule_by_id(once_id).is_some());
}

#[test]
fn every_action_field_takes_part_in_merge_and_in_the_count() {
    // The point of generating the three traversals from one field list is
    // that they cannot drift apart. This checks the property directly: set
    // every field on one side, merge into an empty one, and the count must
    // come back equal — which it cannot if `merge` skips a field the
    // struct declares.
    let mut all = RuleActions::new();
    all.position = Some(PositionSpec::CenterOnMonitor(0));
    all.size = Some(SizeSpec::Exact {
        width: 800,
        height: 600,
    });
    all.desktop = Some(1);
    all.always_on_top = Some(true);
    all.always_on_bottom = Some(true);
    all.initial_state = Some(InitialState::Normal);
    all.opacity = Some(0.5);
    all.skip_taskbar = Some(true);
    all.skip_alt_tab = Some(true);
    all.target_monitor = Some(1);
    all.no_decorations = Some(true);
    all.min_size = Some((1, 1));
    all.max_size = Some((2, 2));
    all.prevent_close = Some(true);
    all.prevent_move = Some(true);
    all.prevent_resize = Some(true);
    all.snap_zone = Some(1);

    let declared = all.active_count();
    assert!(declared >= 17, "every declared field should be set here");

    let mut empty = RuleActions::new();
    assert_eq!(empty.active_count(), 0);
    empty.merge(&all);
    assert_eq!(
        empty.active_count(),
        declared,
        "a field the struct declares but merge does not copy"
    );
}

#[test]
fn merging_an_empty_set_of_actions_clears_nothing() {
    let mut actions = RuleActions::new();
    actions.opacity = Some(0.25);
    actions.merge(&RuleActions::new());
    assert_eq!(actions.opacity, Some(0.25));
}
