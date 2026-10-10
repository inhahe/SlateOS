"""Mutation test for flashcards' editors, questions, pointer layer and keeping.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The card editor could not be typed into, so no card could be made or changed;
a new deck could not be named; a delete went at once with its whole history;
nothing answered the pointer (known-issues,
TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED); and nothing
was kept -- a spaced-repetition program that forgot its reviews at close.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # -- the editors -------------------------------------------------------------------------------------
    (
        "a field takes no typing",
        "                    textline::apply_key(self.input(which), key, FIELD_CAPACITY, &clipboard, 13.0);",
        "                    textline::apply_key(&mut TextInput::new(), key, FIELD_CAPACITY, &clipboard, 13.0);\n"
        "                let _ = which;",
        ["a_card_can_be_typed_and_saved", "the_editor_answers_the_pointer"],
    ),
    (
        "Shift+Tab walks forward",
        "                    at.checked_sub(1).unwrap_or(fields.len().saturating_sub(1))",
        "                    at.saturating_add(1).checked_rem(fields.len()).unwrap_or(0)",
        ["tab_walks_the_editors_fields"],
    ),
    (
        "an editor lets keys through to the view behind",
        "        if matches!(self.view, AppView::CardEditor | AppView::DeckEditor) {",
        "        if false && matches!(self.view, AppView::CardEditor | AppView::DeckEditor) {",
        ["an_editor_takes_every_key", "a_card_can_be_typed_and_saved"],
    ),
    (
        "n makes a deck without a name again",
        '            "n" => self.open_new_deck_editor(),',
        '            "n" => self.add_deck("New Deck", ""),',
        ["a_deck_is_named_and_can_be_renamed", "test_new_deck_key"],
    ),
    (
        "e edits no deck",
        '            "e" => self.open_edit_deck(self.selected_deck),',
        '            "e" => {}',
        ["a_deck_is_named_and_can_be_renamed"],
    ),
    (
        "a nameless deck is accepted",
        "        if name.is_empty() {",
        "        if false && name.is_empty() {",
        ["a_nameless_deck_is_refused"],
    ),
    (
        "a rename renames nothing",
        "                deck.name.clone_from(&name);",
        "",
        ["a_deck_is_named_and_can_be_renamed", "decks_and_cards_are_kept_as_they_change"],
    ),
    (
        "a field press puts the keyboard nowhere",
        "            f.hit(Target::Field(*field), rect);",
        "",
        ["the_editor_answers_the_pointer"],
    ),
    # -- the questions ---------------------------------------------------------------------------------------
    (
        "a deck is deleted without asking",
        '            "Delete" | "x" => self.ask_to_delete(Doomed::Deck(self.selected_deck)),',
        '            "Delete" | "x" => self.remove_deck(self.selected_deck),',
        ["deleting_asks_first_and_only_y_deletes", "test_delete_deck_key"],
    ),
    (
        "only the Y key means yes",
        "                key.single_char().map_or(key.key == Key::Y, typed_y)",
        "                key.single_char().map_or(key.key == Key::Y, |_| key.key == Key::Y)",
        ["deleting_asks_first_and_only_y_deletes"],
    ),
    (
        "the question is not a redraw",
        "            pending_delete: self.pending_delete,",
        "            pending_delete: None,",
        ["deleting_asks_first_and_only_y_deletes"],
    ),
    (
        "the question's Delete records no hit box",
        "        f.hit(Target::ConfirmDelete, delete);",
        "",
        ["deleting_asks_first_and_only_y_deletes"],
    ),
    # -- the pointer -------------------------------------------------------------------------------------------
    (
        "a deck row records no hit box",
        "            f.hit(Target::DeckRow(i), row);",
        "",
        ["a_deck_press_chooses_and_a_second_opens"],
    ),
    (
        "a press on the chosen deck does not open it",
        "                if i == self.selected_deck {",
        "                if false && i == self.selected_deck {",
        ["a_deck_press_chooses_and_a_second_opens"],
    ),
    (
        "a card row records no hit box",
        "            f.hit(Target::CardRow(list_i), row);",
        "",
        ["a_card_press_chooses_and_a_second_edits"],
    ),
    (
        "a press on the chosen card does not edit it",
        "                if p == self.selected_card {",
        "                if false && p == self.selected_card {",
        ["a_card_press_chooses_and_a_second_edits"],
    ),
    (
        "the tag chip is drawn only with a filter set",
        "            if !tags.is_empty() {\n                f.hit(Target::TagChip, chip);",
        "            if self.tag_filter.is_some() {\n                f.hit(Target::TagChip, chip);",
        ["the_tag_chip_is_there_with_no_filter", "every_deck_view_button_answers_the_pointer"],
    ),
    (
        "the search box records no hit box",
        "        f.hit(Target::Search, search);",
        "",
        ["every_deck_view_button_answers_the_pointer"],
    ),
    (
        "the card does not turn over by pointer",
        "            f.hit(Target::StudyCard, card_rect);",
        "            let _ = card_rect;",
        ["the_card_and_its_button_both_turn_it_over"],
    ),
    (
        "the flip button records no hit box",
        "            f.hit(Target::StudyCard, prompt);",
        "",
        # Only the test that presses the button by its own box can see this:
        # `study_answers_the_pointer` presses the topmost `StudyCard`, which
        # without the button's box is the card -- and the card turns over too.
        ["the_card_and_its_button_both_turn_it_over"],
    ),
    (
        "a rating records no hit box",
        "                f.hit(Target::Rate(*rating), rect);",
        "",
        ["study_answers_the_pointer"],
    ),
    (
        "Back only closes the search",
        "            AppView::DeckDetail => {\n                self.search_active = false;",
        "            AppView::DeckDetail => {",
        ["back_goes_back_from_every_view"],
    ),
    # -- the lists ---------------------------------------------------------------------------------------------------
    (
        "the deck list does not follow the chosen deck",
        "            self.deck_scroll = self.selected_deck.saturating_add(1).saturating_sub(visible);",
        "            let _ = visible;",
        ["the_deck_list_scrolls_and_follows_the_chosen_deck"],
    ),
    (
        "the deck list never scrolls",
        "            .skip(self.deck_scroll)",
        "            .skip(0)",
        ["the_deck_list_scrolls_and_follows_the_chosen_deck"],
    ),
    (
        "the card list shows eight rows at any height",
        "            rows_top,\n            ((room / Self::CARD_ROW_H).floor().max(1.0)) as usize,",
        "            rows_top,\n            8 + 0 * ((room / Self::CARD_ROW_H).floor().max(1.0)) as usize,",
        ["the_card_list_fits_the_window_and_scrolls"],
    ),
    # -- what is drawn -------------------------------------------------------------------------------------------------
    (
        "a long answer is cut at the card's edge",
        "                lines.extend(text::wrap(line, width, 18.0, FontWeightHint::Regular));",
        "                lines.push(line.to_owned());",
        ["a_long_answer_wraps_on_the_study_card"],
    ),
    (
        "a letter key with no text names nothing",
        "            && let Some(ch) = key_char(key.key, key.modifiers.shift)",
        "            && let Some(ch) = key_char(key.key, key.modifiers.shift).filter(|_| false)",
        ["a_letter_key_names_its_letter_without_text", "every_advertised_key_does_something"],
    ),
    (
        "the list of keys lets keys through",
        "        if self.show_help {\n            if plain && matches!(key.key, Key::Escape | Key::Enter) {",
        "        if false {\n            if plain && matches!(key.key, Key::Escape | Key::Enter) {",
        ["the_list_of_keys_is_modal"],
    ),
    # -- what is kept ----------------------------------------------------------------------------------------------------
    (
        "a review is not kept",
        "        self.keep(deck_idx);",
        "",
        ["a_review_is_kept_across_a_restart"],
    ),
    (
        "the first change keeps only the changed deck",
        "            vec![idx]",
        "            vec![idx]\n        } else if true {\n            vec![idx]",
        ["the_first_change_keeps_every_deck"],
    ),
    (
        "a rename is not kept",
        "                self.keep(idx);",
        "",
        ["decks_and_cards_are_kept_as_they_change"],
    ),
    (
        "a deleted deck's file stays",
        "            self.forget(&gone);",
        "            let _ = gone;",
        ["decks_and_cards_are_kept_as_they_change"],
    ),
    (
        "the window keeps nothing",
        "        app.persist = true;",
        "        app.persist = false;",
        ["a_review_is_kept_across_a_restart", "a_failed_keep_is_reported"],
    ),
    (
        "a window made with new keeps its changes",
        "            persist: false,",
        "            persist: true,",
        ["an_app_made_with_new_keeps_nothing"],
    ),
    # No row for "an unreadable file's number is reused" since 2026-10-09: a
    # new deck's file number is random below 2^52 (design-decisions §1239),
    # so the check against the files in the folder guards odds no test can
    # reach; an_unreadable_deck_file_is_left_alone_and_reported still holds a
    # new deck off the unreadable file.
    (
        "a deck file's description is not read",
        "                description = unescape_field(about);",
        "                let _ = about;",
        ["a_deck_file_keeps_its_name_and_description", "decks_and_cards_are_kept_as_they_change"],
    ),
    (
        "a deck file's name is not read",
        "                name.get_or_insert_with(|| unescape_field(title));",
        # The annotation keeps `name`'s type, which the call gave it; without
        # it the mutant does not compile and so tests nothing.
        "                let _: &Option<String> = &name;\n"
        "                let _ = title;",
        ["a_deck_file_keeps_its_name_and_description", "decks_and_cards_are_kept_as_they_change"],
    ),
    (
        "the notice is drawn under the header again",
        "                y: notice_y + i as f32 * 15.0,",
        "                y: 1.0 + i as f32 * 11.0,",
        ["the_notice_is_drawn_below_the_header"],
    ),
    (
        "a chord works an editor's own keys",
        '        let plain = textline::is_plain(key.modifiers);\n        match key.key {\n            Key::Tab if plain => {',
        '        let plain = true;\n        match key.key {\n            Key::Tab if plain => {',
        ['a_chord_is_neither_a_flashcards_key_nor_typing'],
    ),
    (
        'a chord raises the keys',
        '        if key.key == Key::F1 && plain {',
        '        if key.key == Key::F1 {',
        ['a_chord_is_neither_a_flashcards_key_nor_typing'],
    ),
    (
        'a chorded Y answers the delete question',
        '                textline::types_into_field(key) && key.single_char().is_some_and(typed_y)',
        '                key.single_char().is_some_and(typed_y)',
        ['a_chord_is_neither_a_flashcards_key_nor_typing'],
    ),
    (
        'AltGr is taken for Ctrl',
        '        if textline::is_ctrl_chord(key.modifiers) {\n            return match key.key {\n                Key::S => {',
        '        if key.modifiers.ctrl {\n            return match key.key {\n                Key::S => {',
        ['a_chord_is_neither_a_flashcards_key_nor_typing'],
    ),
    (
        "Alt's and the Windows key's chords work the shortcuts",
        '        if !plain && !textline::types_into_field(key) {',
        '        if false {',
        ['a_chord_is_neither_a_flashcards_key_nor_typing'],
    ),
    # -- the text boxes, the toolkit's fields (c-e-a-theme-can-shape-the-controls)
    (
        'the search box is drawn the same wherever the pointer is',
        '                hovered: self.hover == Some(Target::Search),',
        '                hovered: false,',
        ['the_text_boxes_are_the_toolkits_fields'],
    ),
    (
        'the search box is not marked while it has the keyboard',
        '                focused: self.search_active,',
        '                focused: false,',
        ['the_text_boxes_are_the_toolkits_fields'],
    ),
    (
        'an editor field is drawn the same wherever the pointer is',
        '                    hovered: self.hover == Some(Target::Field(*field)),',
        '                    hovered: false,',
        ['the_text_boxes_are_the_toolkits_fields'],
    ),
    (
        'an editor field is not marked while it has the keyboard',
        '                    hovered: self.hover == Some(Target::Field(*field)),\n                    focused,',
        '                    hovered: self.hover == Some(Target::Field(*field)),\n                    focused: false,',
        ['the_text_boxes_are_the_toolkits_fields'],
    ),
    (
        "the text boxes take the toolkit's focus width, not the user's",
        '        self.focus_ring_width = settings.focus_ring_width();',
        '        let _ = settings;',
        ['the_text_boxes_are_the_toolkits_fields'],
    ),
]

# Two windows of Flashcards save into one library (2026-10-09,
# design-decisions §1239): each wrote its own copy of a deck over the other's,
# so the last to save threw away the other's cards and reviews.
TWO = "two_windows_each_add_a_card_to_one_deck_and_both_are_kept"
HEAR = "a_window_hears_another_windows_save"
FIRST_SIGHT = "a_first_run_window_takes_the_decks_another_kept"
DECK_GONE = "a_deck_deleted_in_another_window_goes_unless_changed_here"
CARD_BACK = "a_card_deleted_elsewhere_while_changed_here_is_saved_back"
SESSION = "a_card_deleted_elsewhere_leaves_a_study_session_in_its_place"
STAYS_DELETED = "a_card_deleted_in_another_window_stays_deleted"
CHOSEN_STAYS = "the_chosen_deck_stays_chosen_when_one_before_it_goes"
UNSAVED = "a_change_not_yet_saved_survives_hearing_another_save"
OLD_FORMAT = "cards_without_numbers_are_numbered_alike_in_every_window"
ORDER = "the_decks_are_listed_in_the_order_they_were_made"
UNREADABLE = "a_save_leaves_a_deck_file_it_cannot_read_as_it_is"

MUTATIONS += [
    (
        "a save writes this window's deck over the file",
        "            (Some(base), Some(theirs)) => merge_decks(base, &mine, &theirs),",
        "            (Some(_), Some(_)) => mine.clone(),",
        [TWO],
    ),
    (
        "a save takes the file for unchanged without asking",
        "        let read = if stamp_now.is_some() && stamp_now == self.file_stamps.get(&id).copied() {",
        "        let read = if true {",
        [TWO],
    ),
    (
        "a save over a file it cannot read goes ahead",
        "            read_deck_file(&file)\n        };",
        "            read_deck_file(&file).or(Ok::<_, String>(None))\n        };",
        [UNREADABLE],
    ),
    (
        "a save does not move on what it compares with",
        "                self.base.insert(id, merged.clone());\n",
        "",
        [STAYS_DELETED],
    ),
    (
        "a failed save says nothing",
        '            Err(err) => self.keep_failed(format!("Could not keep {}: {err}", mine.name)),',
        "            Err(_) => {}",
        [UNSAVED, "closing_while_a_keep_fails_asks_first"],
    ),
    (
        "a first run's changes are measured from nothing",
        "            app.base = app\n                .decks\n                .iter()\n                .filter_map(|d| Some((d.file_id?, d.clone())))\n                .collect();\n",
        "            let _ = &app.base;\n",
        [TWO],
    ),
    (
        "the included decks take file numbers of their own",
        "            deck.file_id = Some(number);",
        "            deck.file_id = None;\n            let _ = number;",
        [TWO],
    ),
    (
        "a card's number is not written",
        '            out.push_str(&format!("I: {}\\n", card.id));',
        "",
        [TWO],
    ),
    (
        "a card's number is not read",
        "                'I' => id = value.trim().parse::<u64>().ok(),",
        "                'I' => {}",
        [TWO],
    ),
    (
        "cards read without numbers are numbered at random",
        "            .unwrap_or_else(|| self.next_counted_card_id());",
        "            .unwrap_or_else(|| self.fresh_card_id());",
        [OLD_FORMAT],
    ),
    (
        "another window's save is not heard",
        "            if stamp.is_some() && stamp == self.file_stamps.get(id).copied() {",
        "            if true {",
        [HEAR],
    ),
    (
        "a reread throws away what is not saved",
        "                    (Some(base), Some(mine)) if mine != base => merge_decks(base, mine, &theirs),",
        "                    (Some(base), Some(mine)) if false && mine != base => merge_decks(base, mine, &theirs),",
        [UNSAVED],
    ),
    (
        "a deck another window made does not come in",
        "                self.decks.push(theirs.clone());\n                changed = true;",
        "                changed = true;",
        [HEAR],
    ),
    (
        "a reread does not move on what the next save compares with",
        "            self.base.insert(*id, theirs);\n",
        "",
        [STAYS_DELETED],
    ),
    (
        "a deck another window deleted stays",
        "                (was_kept && !on_disk.contains(&id) && self.base.get(&id) == Some(d)).then_some(id)",
        "                (was_kept && !on_disk.contains(&id) && self.base.get(&id) == Some(d) && false).then_some(id)",
        [DECK_GONE],
    ),
    (
        "a deck changed here goes when another window deletes it",
        "                (was_kept && !on_disk.contains(&id) && self.base.get(&id) == Some(d)).then_some(id)",
        "                (was_kept && !on_disk.contains(&id)).then_some(id)",
        [DECK_GONE],
    ),
    (
        "a first-run window keeps a deck the other deleted before saving",
        "                let was_kept = self.file_stamps.contains_key(&id) || first_sight;",
        "                let was_kept = self.file_stamps.contains_key(&id);",
        [FIRST_SIGHT],
    ),
    (
        "the chosen deck is found again by its number, not its file",
        "            (_, Some(i)) => self.selected_deck = i,",
        "            (_, Some(_)) => {}",
        [CHOSEN_STAYS],
    ),
    (
        "a study session keeps cards another window deleted",
        "            session.retain_cards(|id| deck.find_card(id).is_some());",
        "            let _ = session;",
        [SESSION],
    ),
    (
        "a study session loses its place when a card before it goes",
        "        self.current_pos = self.current_pos.saturating_sub(gone_before);",
        "        let _ = gone_before;",
        [SESSION],
    ),
    (
        "the next card comes up face up",
        "        if self.current_card_id() != shown {\n            self.flipped = false;\n        }",
        "        let _ = shown;",
        [SESSION],
    ),
    (
        "a card deleted elsewhere while changed here is dropped",
        "                deck.cards.push(original);",
        "                let _ = original;",
        [CARD_BACK],
    ),
    (
        "the decks are read in the order of their files' numbers",
        "        decks.sort_by_key(Deck::order);",
        "",
        [ORDER],
    ),
    (
        "a new deck has no time it was made",
        "            .max(1);\n        self.decks.push(deck);",
        "            .max(1);\n        deck.made = 0;\n        self.decks.push(deck);",
        [ORDER],
    ),
    (
        "when a deck was made is not written",
        '            out.push_str(&format!("#@ {}\\n", self.made));',
        "",
        [ORDER],
    ),
    (
        "when a deck was made is not read",
        "                made = when.trim().parse().unwrap_or(0);",
        "                let _ = when;",
        [ORDER],
    ),
]

# Closing while keeping a deck is failing asks first (2026-10-10): the window
# went at once, and the cards and reviews the failing keep held went with it.
CLOSING = "closing_while_a_keep_fails_asks_first"
DISCARD = "closing_without_saving_goes_without_the_change"
NEVER_KEPT = "a_failed_keep_is_reported"
KEEPS_NOTHING = "a_window_that_keeps_nothing_closes_at_once"
NAMES = "closing_names_every_deck_a_keep_is_failing_for"

MUTATIONS += [
    (
        "a close never asks",
        "            return if self.request_close() {\n"
        "                Response::Exit\n"
        "            } else {\n"
        "                Response::KeepOpen\n"
        "            };",
        "            return Response::Exit;",
        [CLOSING, DISCARD, NEVER_KEPT, NAMES],
    ),
    (
        "a close with everything kept asks",
        "        if !self.unkept() {\n            return true;\n        }\n        let names",
        "        let names",
        [CLOSING],
    ),
    (
        "a close does not try the keep again first",
        "        if self.unkept() {\n            self.keep_unkept();\n        }\n",
        "",
        [CLOSING],
    ),
    (
        "keeping what is not kept keeps nothing",
        "        for i in unkept {\n            self.keep(i);\n        }",
        "        let _ = unkept;",
        [CLOSING],
    ),
    (
        "a deck that never had a file is taken for kept",
        "            None => true,\n        })",
        "            None => false,\n        })",
        [NEVER_KEPT],
    ),
    (
        "a deck that differs from its file is taken for kept",
        "            Some(id) => self.base.get(&id) != Some(d),",
        "            Some(id) => self.base.get(&id).is_none(),",
        [CLOSING, DISCARD, NAMES],
    ),
    (
        "a window that keeps nothing asks at the close",
        "        self.persist && (0..self.decks.len()).any(|i| self.deck_unkept(i))",
        "        (0..self.decks.len()).any(|i| self.deck_unkept(i))",
        [KEEPS_NOTHING],
    ),
    (
        "the question does not say why",
        '            &format!("{detail} -- try saving again before closing?"),',
        '            &format!("{} -- try saving again before closing?", detail.len()),',
        [CLOSING, NEVER_KEPT],
    ),
    (
        "a failed keep's reason is not kept for the question",
        "        self.keep_error = Some(why);",
        "        drop(why);",
        [CLOSING, NEVER_KEPT],
    ),
    (
        "a failed keep's reason is forgotten at once",
        "        if !self.unkept() {\n            self.keep_error = None;\n        }",
        "        self.keep_error = None;",
        [CLOSING],
    ),
    (
        "the question names no deck",
        '            [one] => format!("{one} has changes that are not saved."),',
        '            [one] => format!("{} has changes that are not saved.", one.len()),',
        [CLOSING, NEVER_KEPT],
    ),
    (
        "two decks are not named",
        '                "{} decks have changes that are not saved: {} and {last}.",',
        '                "{} decks have changes that are not saved: {}{last}.",',
        [NAMES],
    ),
    (
        "Save at the question goes even when the keep fails",
        "                self.keep_unkept();\n                !self.unkept()",
        "                self.keep_unkept();\n                true",
        [CLOSING],
    ),
    (
        "Don't save stays",
        "            unsaved::Choice::Discard => true,",
        "            unsaved::Choice::Discard => false,",
        [DISCARD],
    ),
    (
        "Cancel goes",
        "            unsaved::Choice::Cancel => false,",
        "            unsaved::Choice::Cancel => true,",
        [CLOSING],
    ),
    (
        "a key under the question reaches the decks",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && matches!(event, Event::Key(_) | Event::Mouse(_))\n"
        "        {",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && matches!(event, Event::Mouse(_))\n"
        "        {",
        [CLOSING, DISCARD],
    ),
    (
        "the question is not drawn",
        "            question.render(&palette, width, height, &mut tree);",
        "            let _ = (question, palette);",
        [CLOSING, NEVER_KEPT, NAMES],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "flashcards", timeout=900, only=only))
