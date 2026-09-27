"""Mutation test for the renamer's pointer layer, its rule editor and the
engine fixes they exposed.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The renamer drew a toolbar, sidebar tabs, a rule list, a checkbox on every file
and a conflicts filter, and handled no pointer event.  Worse, every rule that
needed a string typed or a choice made -- find and replace, insert, remove,
regex, date stamp, template, most of the extension rules -- could not be added
at all, because the app had no text box and no chooser (known-issues,
TD-C-RENAMER-CAN-ONLY-ADD-THE-RULES-THAT-NEED-NO-TYPING).  Making them
reachable exposed what they would have done: the regex rule was a literal
replace, the date stamp was always 2026-05-18, numbering counted files that
were not being renamed, and an empty search inserted its replacement between
every character.

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
    # -- the engine -----------------------------------------------------------
    (
        "the regex rule is a literal replace again",
        "                regex_replace(pattern, replacement, !*case_sensitive, name)\n"
        "                    .unwrap_or_else(|_| name.to_string())",
        "                name.replace(pattern.as_str(), replacement.as_str())",
        ["the_regex_rule_is_a_regular_expression"],
    ),
    (
        "a regex replacement's & is not the match",
        "                '&' => out.extend_from_slice(subject.get(start..end).unwrap_or(&[])),",
        "                '&' => out.push(b'&'),",
        ["the_regex_rule_is_a_regular_expression"],
    ),
    (
        "a regex replacement's groups are ignored",
        "                    Some(d @ '0'..='9') => {",
        "                    Some(d @ '0'..='9') if false => {",
        ["the_regex_rule_is_a_regular_expression"],
    ),
    (
        "an empty search puts the replacement between every character",
        "                // a rule the user has only half written must change nothing.\n"
        "                if find.is_empty() {\n                    return name.to_string();\n                }\n",
        "",
        ["an_empty_search_changes_nothing"],
    ),
    (
        "the date stamp is one day in May again",
        "                let date_str = stamp_for(*format, ctx.modified_ms);",
        "                let date_str = format.format(2026, 5, 18, 14, 30, 0);",
        ["a_date_stamp_is_the_files_own_date"],
    ),
    (
        "numbering counts files that are not being renamed",
        "            if !(file.selected && file.renameable) {\n"
        "                file.new_name.clone_from(&file.original_name);\n"
        "                continue;\n            }",
        "            if !file.renameable {\n"
        "                file.new_name.clone_from(&file.original_name);\n"
        "                continue;\n            }",
        ["numbering_counts_only_the_ticked_files"],
    ),
    (
        "a tick does not re-run the preview",
        "        file.selected = !file.selected;\n"
        "        // The preview and the numbering follow the ticks, and so do the\n"
        "        // conflicts: a collision with a file that is no longer being renamed\n"
        "        // is no collision, and one with a file that now is, is.\n"
        "        self.apply_operations();\n",
        "        file.selected = !file.selected;\n",
        [
            "numbering_counts_only_the_ticked_files",
            "unticking_a_file_updates_the_conflicts",
        ],
    ),
    (
        "opening a folder throws the rules away again",
        "        self.files.clear();\n        // The rules are kept:",
        "        self.files.clear();\n        self.operations.clear();\n        // The rules are kept:",
        ["the_rules_survive_opening_another_folder"],
    ),
    # -- the pointer ------------------------------------------------------------
    (
        "a rule row records no hit box",
        "            f.hit(Target::Rule(i), row);\n",
        "",
        ["every_rule_can_be_selected_moved_and_removed"],
    ),
    (
        "a file's checkbox is not its own target",
        "            f.hit(Target::Check(*file_idx), check);\n",
        "",
        [
            "a_row_moves_the_cursor_and_its_box_ticks",
            "numbering_counts_only_the_ticked_files",
        ],
    ),
    (
        "the file list does not follow the cursor",
        "            self.file_scroll = pos.saturating_sub(rows.saturating_sub(1));",
        "            let _ = rows;",
        ["the_file_list_scrolls"],
    ),
    (
        "the operations panel does not scroll",
        "                    && l.panel.contains(x, y) =>",
        "                    && l.panel.contains(x, y)\n                    && false =>",
        ["a_tall_editor_scrolls"],
    ),
    (
        "the layout is the size the window opened at",
        "                    self.last_width = *width as f32;",
        "                    self.last_width = *width as f32 * 0.0 + 1100.0;",
        ["a_resize_moves_where_presses_land"],
    ),
    # -- the rule editor ----------------------------------------------------------
    (
        "a new rule does not take the keyboard",
        "        let first = rule_slots(&op).first().copied();",
        "        let first: Option<Slot> = None;",
        [
            "the_add_rule_menu_adds_every_kind",
            "a_find_and_replace_rule_is_written_in_its_editor",
        ],
    ),
    (
        "what is typed does not reach the rule",
        "                    .is_some_and(|op| set_field(op, slot, &typed));",
        "                    .is_some_and(|op| set_field(op, slot, \"\") && false);",
        [
            "a_find_and_replace_rule_is_written_in_its_editor",
            "a_counting_box_refuses_what_is_not_a_number",
            "f2_edits_the_selected_rule",
        ],
    ),
    (
        "a refused count is not shown as refused",
        "                self.draft_invalid = !taken;",
        "                self.draft_invalid = false;",
        ["a_counting_box_refuses_what_is_not_a_number"],
    ),
    (
        "F2 does not edit",
        "            Key::F2 => {",
        "            Key::F2 if false => {",
        ["f2_edits_the_selected_rule", "every_advertised_key_does_something"],
    ),
    (
        "a rename is stamped with no time",
        "                .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),",
        "                .map_or(0, |_| 0),",
        ["the_history_says_when"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "renamer", timeout=600, only=only))
