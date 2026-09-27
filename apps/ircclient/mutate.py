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
