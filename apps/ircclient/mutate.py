"""Mutation test for the IRC client's connection.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-26, when the client stopped having
no socket: the connection (`net.rs`) and how the window drives it
(`main.rs`) -- registration, the server's echoes, what is sent and what is
refused.

Deliberately absent: `auto_join` on the welcome (the default list is empty,
by design, so nothing joins), and `CONNECT_TIMEOUT` (no test can wait twenty
seconds for a connection that never answers). Both are read by eye.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

# net.rs
BOTH_WAYS = "lines_go_both_ways_and_a_ping_is_answered"
BREAK = "a_line_with_a_break_is_refused"
LATIN1 = "a_latin1_line_loses_no_byte"
PING_SHAPES = "every_shape_of_ping_is_recognised"
# main.rs
CONVERSATION = "a_connection_registers_and_carries_a_conversation"
NICK_TAKEN = "a_nickname_in_use_is_tried_again"
VERSION = "a_ctcp_version_is_answered_not_shown"
HANG_UP = "the_server_hanging_up_is_said"
RENAME = "a_rename_is_reported_only_where_the_user_is"
TAGS = "message_tags_are_skipped"
NOTHING_PRETENDED = "without_a_connection_a_command_says_nothing_was_sent"
JOIN = "slash_join_opens_the_channel"
CLOCK = "a_chat_client_asks_for_a_clock_only_while_connected"
EDITS = "the_message_line_edits_at_a_caret"
RECALLED = "a_recalled_line_is_edited_from_its_end"
NOTHING = "a_key_that_changes_nothing_in_the_line_is_not_a_redraw"
SHOWN = "the_message_line_edits_the_text_it_shows"

NET = [
    (
        "a line break is sent",
        "        if line.contains(['\\r', '\\n', '\\0']) {",
        "        if line.contains('\\0') {",
        [BREAK],
    ),
    (
        "a PING reaches the window unanswered",
        "        if let Some(token) = ping_token(&line) {",
        "        if let Some(token) = ping_token(&line).filter(|_| false) {",
        [BOTH_WAYS],
    ),
    (
        "the PONG answers another token",
        '            let pong = format!("PONG :{token}\\r\\n");',
        '            let pong = format!("PONG :x{token}\\r\\n");',
        [BOTH_WAYS],
    ),
    (
        "a PING with a colon keeps it",
        "    Some(token.strip_prefix(':').unwrap_or(token))",
        "    Some(token)",
        [PING_SHAPES, BOTH_WAYS],
    ),
    (
        "a line that is not UTF-8 is replaced",
        "    String::from_utf8(raw).unwrap_or_else(|e| e.into_bytes().into_iter().map(char::from).collect())",
        "    String::from_utf8(raw).unwrap_or_default()",
        [LATIN1],
    ),
]

MAIN = [
    (
        "message tags are read as the command",
        "        let line = if line.starts_with('@') {",
        "        let line = if line.starts_with('\\0') {",
        [TAGS],
    ),
    (
        "registration sends no NICK",
        "        self.send_line(&cmd_nick(&nick));",
        "        let _ = nick;",
        [CONVERSATION, NICK_TAKEN],
    ),
    (
        "a taken nickname is not tried again",
        "                self.my_nick.push('_');",
        "",
        [NICK_TAKEN],
    ),
    (
        "a join does not switch to the channel",
        "            && let Some(wanted) = self.pending_switch.clone()",
        "            && let Some(wanted) = self.pending_switch.clone().filter(|_| false)",
        [CONVERSATION, JOIN],
    ),
    (
        "a CTCP VERSION is drawn as a message",
        "            Some(CtcpMessage::Version) => {",
        "            Some(CtcpMessage::Version) if false => {",
        [VERSION],
    ),
    (
        "a rename is told to every channel",
        "            if ch.find_user(&old_nick).is_none() {\n                continue;\n            }",
        "",
        [RENAME],
    ),
    (
        "one's own line is shown though it was not sent",
        "            && !self.send_line(&cmd_privmsg(target, text))",
        "            && (self.send_line(&cmd_privmsg(target, text)) && false)",
        [NOTHING_PRETENDED],
    ),
    (
        "a command not sent is applied anyway",
        "        if !self.send_line(&wire) {",
        "        if !self.send_line(&wire) && false {",
        [NOTHING_PRETENDED],
    ),
    (
        "the clock runs with no connection",
        "        self.link.is_some().then_some(Duration::from_millis(500))",
        "        Some(Duration::from_millis(500))",
        [CLOCK, HANG_UP],
    ),
    (
        "the banner is drawn over a connection",
        "        if self.link.is_some() {\n            return;\n        }",
        "",
        [CONVERSATION],
    ),
    (
        "the server password is dropped",
        "                self.server_config.password = words.next().map(str::to_string);",
        "                let _ = words.next();",
        ["a_server_password_is_sent_first_and_not_kept_in_history"],
    ),
    (
        "the history keeps the password",
        "        self.input_history.push(without_password(&line));",
        "        self.input_history.push(line.clone());",
        ["a_server_password_is_sent_first_and_not_kept_in_history"],
    ),
    (
        "an old password is kept for the next server",
        "                self.server_config.password = words.next().map(str::to_string);",
        "                if let Some(p) = words.next() {\n                    self.server_config.password = Some(p.to_string());\n                }",
        ["a_connect_without_a_password_clears_the_last_one"],
    ),
    # -- the shortcut card is modal, for the keys and the pointer
    (
        "a key that is not the card's types or sends behind it",
        "            if closes {\n"
        "                self.show_help = false;\n"
        "            }\n"
        "            return closes;\n",
        "            if closes {\n"
        "                self.show_help = false;\n"
        "                return true;\n"
        "            }\n",
        ["the_shortcut_card_takes_every_key_and_press_while_it_is_up"],
    ),
    (
        "Escape does not put the card away",
        "                textline::is_plain(event.modifiers) && matches!(event.key, Key::F1 | Key::Escape);\n",
        "                textline::is_plain(event.modifiers) && matches!(event.key, Key::F1);\n",
        ["the_shortcut_card_takes_every_key_and_press_while_it_is_up"],
    ),
    # -- a chord is neither typing nor one of the line's keys (2026-10-04)
    (
        "a chord puts the card away",
        "                textline::is_plain(event.modifiers) && matches!(event.key, Key::F1 | Key::Escape);\n",
        "                matches!(event.key, Key::F1 | Key::Escape);\n",
        ["a_chord_is_neither_typed_into_the_line_nor_one_of_its_keys"],
    ),
    # No rows for "a command's letter is typed into the line" or "AltGr's
    # characters are not typed": since 2026-10-04 the line's typing is
    # textline::apply_key's, which makes both distinctions itself, in its own
    # crate and with its own tests;
    # a_chord_is_neither_typed_into_the_line_nor_one_of_its_keys still holds
    # the line to them.
    (
        "a chord is one of the line's keys",
        "        if !textline::is_plain(event.modifiers) {\n            return self.input_key(event);\n        }\n",
        "",
        ["a_chord_is_neither_typed_into_the_line_nor_one_of_its_keys"],
    ),
    # -- the message line is the toolkit's field (2026-10-04; lane C,
    #    c-e-a-theme-can-shape-the-controls)
    (
        "the message line never has the keyboard",
        "            focused: !self.show_help,\n",
        "            focused: false,\n",
        [
            "the_message_line_is_the_toolkits_field",
            "the_caret_follows_the_typing_and_a_long_line_scrolls",
        ],
    ),
    (
        "the message line keeps its mark under the key list",
        "            focused: !self.show_help,\n",
        "            focused: true,\n",
        ["the_message_line_is_the_toolkits_field"],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        ["the_message_line_is_the_toolkits_field"],
    ),
    (
        "the caret is at the start of the line",
        "                cursor: self.input_cursor(),\n",
        "                cursor: TextCursor::from(0),\n",
        ["the_caret_follows_the_typing_and_a_long_line_scrolls", EDITS],
    ),
    (
        "a press goes through the card",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                    self.show_help = false;\n"
        "                    return true;\n"
        "                }\n",
        "",
        ["the_shortcut_card_takes_every_key_and_press_while_it_is_up"],
    ),
    (
        "only the left button puts the card away",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n",
        "                MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n",
        ["the_shortcut_card_takes_every_key_and_press_while_it_is_up"],
    ),
    (
        "the wheel scrolls the chat the card covers",
        "                MouseEventKind::Scroll { .. } => return false,\n",
        "",
        ["the_wheel_scrolls_no_chat_under_the_card"],
    ),
]

# The message line edits at a caret (2026-10-04,
# known-issues/E-twenty-nine-applications-type-only-at-the-end-of-a-box): it
# took typing at its end and Backspace from it, and nothing else.
MAIN += [
    (
        "a cut or a copy takes nothing to the clipboard",
        "            self.input_clipboard = copied;\n",
        "            let _ = copied;\n",
        [EDITS],
    ),
    (
        "an edit keeps the line in the history",
        "            self.input_text = self.input_editor.text().to_owned();\n            // Editing leaves the history: the line being edited is the\n            // user's own, not the recalled one it started from.\n            self.input_history_idx = None;\n",
        "            self.input_text = self.input_editor.text().to_owned();\n",
        [RECALLED],
    ),
    (
        "a caret moved is not drawn",
        "        before\n            != (\n                self.input_editor.cursor(),\n                self.input_editor.selection_anchor(),\n            )\n    }\n",
        "        false\n    }\n",
        [NOTHING],
    ),
    (
        "a key that changes nothing is a redraw",
        "        before\n            != (\n                self.input_editor.cursor(),\n                self.input_editor.selection_anchor(),\n            )\n    }\n",
        "        edit.handled\n    }\n",
        [NOTHING],
    ),
    (
        "a key finds the editor holding another line",
        "    fn input_key(&mut self, event: &KeyEvent) -> bool {\n        if self.input_editor.text() != self.input_text {\n",
        "    fn input_key(&mut self, event: &KeyEvent) -> bool {\n        if false {\n",
        [SHOWN, RECALLED],
    ),
    (
        "a press finds the editor holding another line",
        "        let drawn = self.input_cursor();\n        if self.input_editor.text() != self.input_text {\n",
        "        let drawn = self.input_cursor();\n        if false {\n",
        [SHOWN],
    ),
    (
        "a press puts the caret at the start",
        "            x - rect.x - INPUT_TEXT_INSET,\n",
        "            0.0,\n",
        [EDITS],
    ),
    (
        "a press in the line does nothing",
        "                    self.press_input(x);\n                    return true;\n",
        "",
        [EDITS, SHOWN],
    ),
    (
        "the selection is not drawn",
        "                selection_anchor: if self.input_editor.text() == self.input_text {\n",
        "                selection_anchor: if false {\n",
        [EDITS],
    ),
]

TABLES = {
    "net.rs": NET,
    "main.rs": MAIN,
}

if __name__ == "__main__":
    only = sys.argv[1:]
    names = [name for rows in TABLES.values() for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    worst = 0
    for file, rows in TABLES.items():
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        print(f"\n######## {file} ########")
        worst = max(worst, sweep(SRC / file, rows, "ircclient", timeout=900, only=mine))
    raise SystemExit(worst)
