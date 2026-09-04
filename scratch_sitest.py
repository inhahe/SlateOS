"""One-shot: sysinfo had no tests at all. Give it some."""

import pathlib

TESTS = '''
// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that reaches into a structure it just built and finds it missing
    // has found a bug, and panicking is how it reports one.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;

    fn press(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    fn press_ctrl(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: String::new(),
        })
    }

    fn typed(c: char) -> Event {
        Event::Key(KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: c.to_string(),
        })
    }

    #[test]
    fn a_fresh_window_has_a_selection_and_something_to_show() {
        let app = SysInfoState::new();
        assert!(
            !app.visible_tree_rows().is_empty(),
            "the tree should not open empty"
        );
        assert!(
            app.visible_tree_rows().contains(&app.selected_category),
            "the selection should be on a row the tree is showing"
        );
        assert!(
            !app.current_properties().is_empty(),
            "the selected category should have properties"
        );
    }

    #[test]
    fn the_arrows_walk_the_visible_rows_and_stop_at_the_ends() {
        let mut app = SysInfoState::new();
        let rows = app.visible_tree_rows();
        assert!(rows.len() >= 2);
        // Start at the top and walk to the bottom.
        app.selected_category = rows[0];
        assert_eq!(
            app.handle_event(&press(Key::Up)),
            EventResult::Ignored,
            "Up at the top should stay put"
        );
        for row in rows.iter().skip(1) {
            assert_eq!(app.handle_event(&press(Key::Down)), EventResult::Consumed);
            assert_eq!(app.selected_category, *row);
        }
        assert_eq!(
            app.handle_event(&press(Key::Down)),
            EventResult::Ignored,
            "Down at the bottom should stay put"
        );
    }

    #[test]
    fn the_selection_never_lands_on_a_collapsed_row() {
        // The tree hides its children when a node is collapsed, and the
        // selection walks the *visible* rows, so this is the invariant that
        // makes the highlight always visible.
        let mut app = SysInfoState::new();
        for _ in 0..40 {
            app.handle_event(&press(Key::Down));
            assert!(
                app.visible_tree_rows().contains(&app.selected_category),
                "selection left the visible rows at {:?}",
                app.selected_category
            );
        }
    }

    #[test]
    fn collapsing_a_node_hides_its_children_and_expanding_brings_them_back() {
        let mut app = SysInfoState::new();
        // Find a row that actually has children to hide.
        let Some(parent) = app
            .visible_tree_rows()
            .into_iter()
            .find(|c| {
                let mut probe = SysInfoState::new();
                probe.selected_category = *c;
                probe.collapse_selected();
                probe.visible_tree_rows().len() < app.visible_tree_rows().len()
            })
        else {
            return; // no expandable node in the tree; nothing to assert
        };
        let before = app.visible_tree_rows().len();
        app.selected_category = parent;
        app.collapse_selected();
        let collapsed = app.visible_tree_rows().len();
        assert!(collapsed < before, "collapsing hid nothing");
        assert!(
            app.visible_tree_rows().contains(&app.selected_category),
            "collapsing stranded the selection"
        );
        app.expand_selected();
        assert_eq!(
            app.visible_tree_rows().len(),
            before,
            "expanding did not restore the rows"
        );
    }

    #[test]
    fn the_left_and_right_arrows_collapse_and_expand() {
        let mut app = SysInfoState::new();
        let start = app.visible_tree_rows().len();
        // Walk to a row whose Left actually collapses something.
        for _ in 0..40 {
            app.handle_event(&press(Key::Left));
            if app.visible_tree_rows().len() < start {
                app.handle_event(&press(Key::Right));
                assert_eq!(
                    app.visible_tree_rows().len(),
                    start,
                    "Right did not undo Left"
                );
                return;
            }
            app.handle_event(&press(Key::Down));
        }
    }

    #[test]
    fn search_finds_a_property_that_is_really_there() {
        let app = SysInfoState::new();
        // Take a real property from the tree and look for it, rather than
        // guessing at a string: a search test that invents its own needle
        // proves only that the needle is absent.
        let (cat, prop) = app
            .visible_tree_rows()
            .into_iter()
            .find_map(|c| {
                let mut probe = SysInfoState::new();
                probe.selected_category = c;
                probe.current_properties().first().map(|p| (c, p.clone()))
            })
            .expect("some category has a property");
        let hits = app.search_all(&prop.name);
        assert!(
            hits.iter().any(|(hc, hp)| *hc == cat && hp.name == prop.name),
            "searching for {:?} did not find it under {:?}",
            prop.name,
            cat
        );
    }

    #[test]
    fn search_is_case_insensitive_and_an_absent_needle_finds_nothing() {
        let app = SysInfoState::new();
        let prop = app
            .current_properties()
            .first()
            .cloned()
            .expect("the opening category has properties");
        assert!(
            !app.search_all(&prop.name.to_uppercase()).is_empty(),
            "an upper-case search should still match"
        );
        assert!(
            app.search_all("zzzz-no-such-property-zzzz").is_empty(),
            "an absent needle should find nothing"
        );
    }

    #[test]
    fn ctrl_f_opens_the_search_box_and_typing_goes_into_it() {
        // Otherwise "e" typed into a search would be Ctrl+E's export, and the
        // arrow keys would move the tree out from under the box.
        let mut app = SysInfoState::new();
        assert!(!app.search_focused);
        assert_eq!(app.handle_event(&press_ctrl(Key::F)), EventResult::Consumed);
        assert!(app.search_focused);
        let before = app.selected_category;
        app.handle_event(&typed('c'));
        app.handle_event(&typed('p'));
        assert_eq!(
            app.selected_category, before,
            "typing in the search box moved the tree selection"
        );
        app.handle_event(&press(Key::Escape));
        assert!(!app.search_focused, "Escape should close the search box");
    }

    #[test]
    fn a_key_the_app_has_no_use_for_is_not_consumed() {
        let mut app = SysInfoState::new();
        assert_eq!(app.handle_event(&press(Key::F9)), EventResult::Ignored);
    }

    #[test]
    fn a_key_release_does_nothing() {
        let mut app = SysInfoState::new();
        let before = app.selected_category;
        let release = Event::Key(KeyEvent {
            key: Key::Down,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.handle_event(&release), EventResult::Ignored);
        assert_eq!(app.selected_category, before);
    }

    #[test]
    fn the_export_names_the_categories_and_their_properties() {
        let app = SysInfoState::new();
        let text = app.export_text();
        assert!(!text.is_empty(), "the export is empty");
        let cat = app.selected_category;
        assert!(
            text.contains(cat.label()),
            "the export omits the category {:?}",
            cat.label()
        );
        if let Some(prop) = app.current_properties().first() {
            assert!(
                text.contains(&prop.name),
                "the export omits the property {:?}",
                prop.name
            );
        }
    }

    #[test]
    fn rendering_at_a_new_size_adopts_it_and_draws_something() {
        // The first frame is drawn before any `Resize` arrives, and a
        // compositor may grant a size that was never asked for.
        let mut app = SysInfoState::new();
        let tree = app.render(1600.0, 900.0);
        assert_eq!((app.window_width, app.window_height), (1600.0, 900.0));
        assert!(!tree.is_empty(), "the app drew nothing");
    }

    #[test]
    fn every_category_renders_without_panicking_at_an_awkward_size() {
        // A category reachable with an arrow key that panics when drawn is a
        // crash the user reaches by holding Down.
        let mut app = SysInfoState::new();
        for _ in 0..40 {
            for (w, h) in [(1.0, 1.0), (640.0, 480.0), (3840.0, 2160.0)] {
                let tree = app.render(w, h);
                assert!(
                    !tree.is_empty(),
                    "{:?} drew nothing at {w}x{h}",
                    app.selected_category
                );
            }
            app.handle_event(&press(Key::Down));
        }
    }
}
'''

p = pathlib.Path("apps/sysinfo/src/main.rs")
s = p.read_text(encoding="utf-8")
assert "#[cfg(test)]" not in s, "already has tests"
s = s.rstrip("\n") + "\n" + TESTS
p.write_text(s, encoding="utf-8", newline="\n")
print("sysinfo tests added")
