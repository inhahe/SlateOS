"""Mutation test for the mail client: its window over mail kept in files, the
codings it reads and writes mail in, and the store of folders, drafts and
marks.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

It drew a compose form it never showed, typed every key into an invisible
draft, showed a one-line preview as the message, drew no reading pane at all
"below", and had no mail it could ever show (known-issues,
TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED).

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"
DECODE_SRC = Path(__file__).parent / "src" / "decode.rs"
STORE_SRC = Path(__file__).parent / "src" / "store.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a message opened is not marked read",
        "        self.mark_read(id);\n"
        "        self.remember(id);\n"
        "        self.active_panel = Panel::Reading;",
        "        self.remember(id);\n"
        "        self.active_panel = Panel::Reading;",
        ["a_mail_folder_is_read_and_its_marks_are_kept"],
    ),
    (
        "a mark is not kept",
        "        if let Some(path) = &self.flags_path {",
        "        if let Some(path) = None::<&PathBuf> {",
        ["a_mail_folder_is_read_and_its_marks_are_kept"],
    ),
    (
        "a Status header is not a read mark",
        "            seen: marks.seen || seen_in_file || own,",
        "            seen: marks.seen || own,",
        ["a_mail_folder_is_read_and_its_marks_are_kept"],
    ),
    (
        "mail read from files is deleted",
        "            Some(origin) if summary.flags.draft => {",
        "            Some(origin) if true => {",
        ["mail_read_from_files_is_never_deleted"],
    ),
    (
        "one Delete deletes a draft",
        "                if self.doomed != Some(id) {",
        "                if false {",
        ["a_draft_is_saved_edited_and_deleted"],
    ),
    (
        "saving a draft again makes a second file",
        "        let result = match &compose.saved_as {",
        "        let result = match &None::<PathBuf> {",
        ["a_draft_is_saved_edited_and_deleted"],
    ),
    (
        "a draft opens to be read, not written",
        "        if own && let Some(stored) = self.stored.get(&id) {",
        "        if false && let Some(stored) = self.stored.get(&id) {",
        ["a_draft_is_saved_edited_and_deleted"],
    ),
    (
        "closing unsaved work discards it at once",
        "        if compose.unsaved() && !compose.confirm_close {",
        "        if false {",
        ["closing_unsaved_work_asks_first"],
    ),
    (
        "a reply quotes the preview",
        "        if let Some(stored) = self.stored.get(&id) {\n"
        "            return Some(stored.message.readable_body().text);\n"
        "        }",
        "",
        ["a_message_is_read_whole_and_its_attachment_saved"],
    ),
    (
        "a forward leaves its attachments behind",
        "            if let Some(stored) = self.stored.get(&message_id) {\n"
        "                draft.attachments = stored",
        "            if let Some(stored) = None::<&store::Stored> {\n"
        "                draft.attachments = stored",
        ["every_control_answers_the_pointer"],
    ),
    (
        "the pane below draws nothing",
        "                    Some(Rect::new(x, top + list_h, w, (h - list_h).max(0.0))),",
        "                    None,",
        ["the_reading_pane_is_drawn_wherever_it_is"],
    ),
    (
        "Save as writes nothing",
        "                    self.status_message = match safeio::write_str_atomically(path, &built.text) {",
        "                    self.status_message = match Ok::<(), std::io::Error>(()) {",
        ["save_as_writes_a_message_that_reads_back"],
    ),
    (
        "an attachment is not saved",
        "                    Some(bytes) => match safeio::write_atomically(path, &bytes) {",
        "                    Some(bytes) => match Ok::<(), std::io::Error>(drop(bytes)) {",
        ["a_message_is_read_whole_and_its_attachment_saved"],
    ),
    (
        "a file opened is not listed",
        "                if !self.opened.iter().any(|p| p == path) {\n"
        "                    self.opened.push(path.to_path_buf());\n"
        "                }",
        "",
        ["a_file_is_opened_from_anywhere"],
    ),
    (
        "the list does not follow the keys",
        "                if self.selected_message != before {\n"
        "                    self.follow_selection();\n"
        "                }",
        "",
        ["the_message_list_scrolls_and_follows_the_keys"],
    ),
    (
        "the wheel does not scroll the message",
        "                self.read_scroll = next;",
        "                let _ = next;",
        ["a_message_is_read_whole_and_its_attachment_saved"],
    ),
    (
        "the search box cannot be pressed",
        "        f.hit(Target::SearchBox, search);",
        "",
        ["every_control_answers_the_pointer"],
    ),
    (
        "the notice does not say where mail comes from",
        "    \"An empty folder here is not \\u{201C}no new mail\\u{201D}. It reads mail kept in files",
        "    \"An empty folder here is empty. It reads mail kept in files",
        ["the_window_says_it_cannot_fetch_mail"],
    ),
]

DECODE_MUTATIONS = [
    (
        "Windows-1252's high table is ignored",
        "    if (0x80..=0x9F).contains(&b) {",
        "    if false {",
        ["character_sets_are_decoded_and_a_wrong_label_is_said"],
    ),
    (
        "the space between encoded words is kept",
        "            if !(after_word && before.chars().all(char::is_whitespace)) {",
        "            if true {",
        ["encoded_words_are_decoded_and_joined"],
    ),
    (
        "Q's underscore is not a space",
        "            b'_' => out.push(b' '),",
        "            b'_' => out.push(b'_'),",
        ["encoded_words_are_decoded_and_joined"],
    ),
    (
        "a semicolon in quotes splits a parameter",
        "        } else if c == ';' && !quoted {",
        "        } else if c == ';' {",
        ["parameters_are_unquoted_and_rfc_2231_is_put_together"],
    ),
    (
        "RFC 2231 pieces are taken in the order they come",
        "        parts.sort_by_key(|(index, _, _)| *index);",
        "",
        ["parameters_are_unquoted_and_rfc_2231_is_put_together"],
    ),
    (
        "a script is read as text",
        "                if !closing && (name == \"script\" || name == \"style\") {",
        "                if false {",
        ["html_reads_as_text"],
    ),
    (
        "an envelope line needs no date",
        "    has_sender && dated",
        "    has_sender",
        ["an_mbox_splits_into_its_messages"],
    ),
    (
        "a quoted From is not unquoted",
        "            if quoted > 0 && line.get(quoted..).is_some_and(|l| l.starts_with(b\"From \")) {",
        "            if false {",
        ["an_mbox_splits_into_its_messages"],
    ),
    (
        "a date's zone is ignored",
        "            .saturating_sub(offset_minutes.saturating_mul(60)),",
        "            .saturating_sub(0),",
        ["dates_are_read_to_seconds"],
    ),
    (
        "quoted-printable leaves = as it is",
        "                ((b == b' ' || b == b'\\t') && !last) || ((33..=126).contains(&b) && b != b'=');",
        "                ((b == b' ' || b == b'\\t') && !last) || (33..=126).contains(&b);",
        ["quoted_printable_round_trips"],
    ),
    (
        "quoted-printable lines run long",
        "            if width.saturating_add(piece_len) > limit {",
        "            if false {",
        ["quoted_printable_round_trips"],
    ),
    (
        "an encoded word runs long",
        "        if chunk.len().saturating_add(c.len_utf8()) > 45 {",
        "        if false {",
        ["header_words_round_trip"],
    ),
]

STORE_MUTATIONS = [
    (
        "a folder with no extension is not an mbox",
        "        decode::is_envelope(first)",
        "        false",
        ["the_mail_directory_lists_its_folders"],
    ),
    (
        "a mark is not kept under the Message-ID",
        "            |id| format!(\"id:{id}\"),",
        "            |_| String::from(\"id:\"),",
        ["a_folder_is_read_message_by_message"],
    ),
    (
        "any file is taken for a marks file",
        "        if lines.next() != Some(FLAGS_HEADER) {",
        "        if false {",
        ["flags_are_kept_in_a_file_of_their_own"],
    ),
    (
        "a tab in a key breaks the marks file",
        "            '\\t' => out.push_str(\"\\\\t\"),",
        "            '\\t' => out.push('\\t'),",
        ["flags_are_kept_in_a_file_of_their_own"],
    ),
    (
        "a draft's name keeps a slash",
        "            if c.is_control() || matches!(c, '/' | '\\\\' | ':' | '*' | '?' | '\"' | '<' | '>' | '|') {",
        "            if c.is_control() {",
        ["a_draft_is_named_for_its_subject"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    results = [
        sweep(SRC, MUTATIONS, "email", timeout=900, only=only),
        sweep(DECODE_SRC, DECODE_MUTATIONS, "email", timeout=900, only=only),
        sweep(STORE_SRC, STORE_MUTATIONS, "email", timeout=900, only=only),
    ]
    raise SystemExit(max(results))
