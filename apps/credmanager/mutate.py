"""Mutation test for the credential manager.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Two tables:

* **main.rs** -- Copy (2026-09-27): it said "Copied Password -- clears in 30s"
  over a clipboard only this program could read. It now copies nothing and
  says why. And the vault's lifecycle (2026-09-27): a first run makes the
  vault from a master password typed twice, locking forgets the key and every
  entry, every change is sealed under a new nonce and written, a save without
  secure randomness is refused, a file that is not a vault is never written
  over, and a close waits once for a save that failed.
  And its doors out (2026-09-27, C-Q25): Export only after the warning says
  what the file is, and every field quoted; a backup is the vault sealed,
  restored only with its master password and only after asking. And the
  entry's own controls (2026-09-27): Edit changes what the form shows and
  keeps what it does not; Delete asks first.
* **vaultfile.rs** -- the file itself: a header that asks for too much work is
  refused before any is done, and contents are read whole or not at all.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

REFUSAL = "a_copy_press_says_nothing_was_copied_and_why"
PAST_END = "a_copy_target_past_the_last_field_does_nothing"
FIRST_RUN = "a_first_run_makes_the_vault_from_a_master_password_typed_twice"
MISMATCH = "a_short_or_mismatched_master_password_makes_no_vault"
CLEAR = "nothing_in_the_vault_file_is_in_the_clear"
REOPEN = "a_vault_opens_again_only_with_its_master_password"
NONCE = "every_change_is_saved_as_it_is_made_under_a_new_nonce"
JUNK = "a_file_that_is_not_a_vault_is_left_alone"
FORGETS = "locking_forgets_the_key_and_every_entry"
ENTROPY = "without_secure_randomness_nothing_is_sealed_and_the_window_says_so"
CLOSE = "a_close_that_could_not_save_says_so_once"
REFUSED_WHOLE = "contents_that_are_not_understood_are_refused_whole"
COSTLY = "a_file_asking_too_much_work_is_refused_before_any_is_done"
EXPORT_CHARS = "an_export_keeps_every_character_of_every_password"
EXPORT_WARNING = "export_comes_only_after_the_warning_says_what_it_is"
RESTORE = "a_restore_needs_the_backups_password_and_asks_before_replacing"
MODAL = "a_vault_dialog_is_modal"
BUTTONS = "the_settings_buttons_can_be_pressed"
PICKER_KEYS = "the_file_dialog_takes_the_keys_while_it_is_up"
EDIT_KEEPS = "editing_a_login_changes_what_was_typed_and_keeps_the_rest"
EDIT_KIND = "an_entry_being_edited_keeps_its_kind"
CARD_KEPT = "a_card_number_left_alone_stays_as_it_was_kept"
DELETE_ASKS = "delete_asks_first_and_then_takes_the_entry_out_of_the_vault"
BUTTONS_ON_ENTRY = "edit_and_delete_can_be_pressed_on_an_entry"

MAIN = [
    (
        "a Copy press is not answered",
        "    state.copy_refused = Some((*label).to_string());",
        "    let _ = label;",
        [REFUSAL],
    ),
    (
        "the refusal is not drawn",
        "    if let Some(label) = state.copy_refused.as_deref() {",
        "    if let Some(label) = None::<&str> {",
        [REFUSAL],
    ),
    (
        "a press past the fields is answered",
        "    let Some((label, value)) = fields.get(index) else {\n        return false;\n    };",
        "    let Some((label, value)) = fields.get(index).or(fields.first()) else {\n        return false;\n    };",
        [PAST_END],
    ),
    (
        "locking keeps the entries",
        "        self.key = None;\n        self.entries.clear();\n",
        "        self.key = None;\n",
        [FORGETS],
    ),
    (
        "locking keeps the key",
        "        self.key = None;\n        self.entries.clear();\n",
        "        self.entries.clear();\n",
        [FORGETS],
    ),
    (
        "a change is not saved",
        "    let result = dispatch_event(state, event);\n    state.keep_if_changed();\n",
        "    let result = dispatch_event(state, event);\n",
        [NONCE, REOPEN],
    ),
    (
        "every save reuses one nonce",
        "        if !(self.entropy)(&mut nonce) {\n            self.save_error = Some(",
        "        if !(self.entropy)(&mut [0u8; seal::NONCE_LEN]) {\n            self.save_error = Some(",
        [NONCE],
    ),
    (
        "a save without secure randomness goes ahead",
        "        if !(self.entropy)(&mut nonce) {\n            self.save_error = Some(",
        "        if false && !(self.entropy)(&mut nonce) {\n            self.save_error = Some(",
        [ENTROPY],
    ),
    (
        "a vault is made without secure randomness",
        "    if !(state.entropy)(&mut salt) {",
        "    if false && !(state.entropy)(&mut salt) {",
        [ENTROPY],
    ),
    (
        "the two passwords need not match",
        "    if form.password != form.confirm {",
        "    if false {",
        [MISMATCH],
    ),
    (
        "a file that is not a vault is offered to be made over",
        "                Err(e) => Gate::Unreadable(e.to_string()),",
        "                Err(_) => Gate::Create(NewVault::default()),",
        [JUNK],
    ),
    (
        "a close does not wait for a failed save",
        "            if self.save_error.is_some() && self.vault.is_unlocked() && !self.close_anyway {",
        "            if false {",
        [CLOSE],
    ),
    (
        "export skips the warning",
        "        Target::ExportCsv => {\n            state.dialog = Some(VaultDialog::ExportWarning);",
        "        Target::ExportCsv => {\n            state.open_picker(PickFor::Export);",
        [EXPORT_WARNING],
    ),
    (
        "a quote in an exported field is not doubled",
        "            if c == '\"' {\n                out.push('\"');\n            }\n",
        "",
        [EXPORT_CHARS],
    ),
    (
        "a restore replaces without asking",
        "                Ok(()) => self.dialog = Some(VaultDialog::RestoreConfirm { path, backup }),",
        "                Ok(()) => {\n"
        "                    self.dialog = Some(VaultDialog::RestoreConfirm { path, backup });\n"
        "                    self.restore_replace();\n"
        "                }",
        [RESTORE],
    ),
    (
        "a restore opens with any password",
        "            }) => match backup.open(&input, self.now) {",
        "            }) => match Ok::<(), vaultfile::OpenError>(()) {",
        [RESTORE],
    ),
    (
        "a vault dialog lets keys through",
        "    if state.dialog.is_some() {\n        return dialog_key(state, key);\n    }\n",
        "",
        [MODAL],
    ),
    (
        "the settings buttons are decoration again",
        "        frame.hit(target, Rect::new(bx, y, width, 32.0));\n",
        "",
        [BUTTONS],
    ),
    (
        "the file dialog lets keys through",
        "            Picked::Handled => return Response::Redraw,",
        "            Picked::Handled => {}",
        [PICKER_KEYS],
    ),
    (
        "an edit adds a new entry instead",
        "    let id = if let Some(id) = form.editing {",
        "    let id = if let Some(id) = None::<u64> {",
        [EDIT_KEEPS, CARD_KEPT],
    ),
    (
        "an edit loses the one-time-code secret",
        "                    n.totp_secret.clone_from(&o.totp_secret);\n",
        "",
        [EDIT_KEEPS],
    ),
    (
        "an unchanged card number is masked again",
        "                    n.number_masked.clone_from(&o.number_masked);\n",
        "",
        [CARD_KEPT],
    ),
    (
        "an entry being edited can change kind",
        "        if self.kind == kind || self.editing.is_some() {",
        "        if self.kind == kind {",
        [EDIT_KIND],
    ),
    (
        "delete does not ask",
        "        state.dialog = Some(VaultDialog::DeleteConfirm { id });",
        "        delete_entry(state, id);",
        [DELETE_ASKS],
    ),
    (
        "a confirmed delete deletes nothing",
        "    if state.vault.remove_entry(id) {",
        "    if false {",
        [DELETE_ASKS],
    ),
    (
        "edit and delete are decoration",
        "        frame.hit(target, Rect::new(bx, y - 4.0, width, 26.0));\n",
        "",
        [BUTTONS_ON_ENTRY],
    ),
]

VAULTFILE = [
    (
        "a file asking any amount of memory is opened",
        "        if kdf.memory_kib > MAX_MEMORY_KIB\n",
        "        if false\n",
        [COSTLY],
    ),
    (
        "a record not understood is skipped",
        '            _ => return Err(bad("a record this program does not know")),',
        "            _ => {}",
        [REFUSED_WHOLE],
    ),
    (
        "an escape never written is read anyway",
        '            textfmt::tsv::unescape(raw).ok_or_else(|| bad("a field is not escaped as written"))',
        "            Ok((*raw).to_string())",
        [REFUSED_WHOLE],
    ),
    (
        "the next id may be one in use",
        "    if contents.next_id <= highest {",
        "    if false {",
        [REFUSED_WHOLE],
    ),
    (
        "an entry may be in a folder that is not there",
        "            if let Some(f) = folder_id\n                && !c.folders.iter().any(|folder| folder.id == f)\n",
        "            if let Some(f) = folder_id\n                && false\n",
        [REFUSED_WHOLE],
    ),
]

TABLES = {
    "main.rs": MAIN,
    "vaultfile.rs": VAULTFILE,
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
        worst = max(worst, sweep(SRC / file, rows, "credmanager", timeout=900, only=mine or None))
    raise SystemExit(worst)
