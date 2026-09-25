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
        "                let done = edit_line(self.input(which), key, FIELD_CAPACITY, &clipboard);",
        "                let done = edit_line(&mut TextInput::new(), key, FIELD_CAPACITY, &clipboard);\n"
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
        "                .map_or(key.key == Key::Y, |c| c.eq_ignore_ascii_case(&'y'));",
        "                .map_or(key.key == Key::Y, |_| key.key == Key::Y);",
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
        "        if self.show_help {\n            if matches!(key.key, Key::Escape | Key::Enter) {",
        "        if false {\n            if matches!(key.key, Key::Escape | Key::Enter) {",
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
    (
        "an unreadable file's number is reused",
        "        self.next_file_id = files",
        "        self.next_file_id = decks.len() as u32 + 1;\n        let _ = files",
        ["an_unreadable_deck_file_is_left_alone_and_reported"],
    ),
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
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "flashcards", timeout=900, only=only))
