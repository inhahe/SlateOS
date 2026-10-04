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
  keeps what it does not; Delete asks first.  And the auto-lock (2026-10-03),
  which never fired in a window: no clock was asked for, so no tick came.
  It is asked for at the deadline now; a window runs on the wall clock, a
  test on ticks whose part seconds are carried; and the event that finds the
  lock due is not taken.  And the text boxes (2026-10-03), the toolkit's
  fields: lit under the pointer as settled after every event, marked where
  typing goes -- which follows from what is showing -- and red where a
  refusal was about them.  And the auto-lock slider (2026-10-03), which was
  a number nothing set and a knob drawn for nothing: the toolkit's slider,
  showing and setting the vault's own time, keeping a drag only when let go,
  and dead under a dialog, another panel or the lock screen.
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
LOCKS_ITSELF = "left_alone_the_vault_locks_itself_on_time"
LOCK_DUE = "the_event_that_finds_the_lock_due_is_not_taken"
WALL_CLOCK = "a_window_runs_on_the_wall_clock"
LOCK_BOX = "the_lock_screens_box_is_the_toolkits_field"
SEARCH_BOXES = "the_search_and_the_forms_boxes_are_the_toolkits_fields"
FIRST_RUN_BOXES = "the_first_runs_and_the_restores_boxes_are_the_toolkits_fields"
SLIDER_SETS = "the_auto_lock_slider_shows_and_sets_the_vaults_own_time"
SLIDER_GUARDED = "the_auto_lock_slider_is_not_used_through_a_dialog_or_by_passing_over_it"

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
    (
        "a command's letter is typed into an entry",
        '    if textline::types_into_field(key) {\n        let typed: String = key.typed().collect();',
        '    if key.types_text() {\n        let typed: String = key.typed().collect();',
        ['a_chord_is_neither_a_vault_key_nor_typing_and_altgr_types'],
    ),
    (
        "a chord works the entry form's keys",
        '    if !textline::is_plain(key.modifiers) {\n        return EventResult::Ignored;\n    }\n    match key.key {\n        Key::Escape => {',
        '    match key.key {\n        Key::Escape => {',
        ['a_chord_is_neither_a_vault_key_nor_typing_and_altgr_types'],
    ),
    (
        "a command's letter is typed into a new master password",
        '            Gate::Create(form) if textline::types_into_field(key) => {',
        '            Gate::Create(form) if key.types_text() => {',
        ['a_chord_is_neither_a_vault_key_nor_typing_and_altgr_types'],
    ),
    (
        'a chord works the new-vault form',
        '            Gate::Create(_) if !textline::is_plain(key.modifiers) => {',
        '            Gate::Create(_) if false => {',
        ['a_chord_is_neither_a_vault_key_nor_typing_and_altgr_types'],
    ),
    (
        "a command's letter is typed into the master password",
        '        if textline::types_into_field(key) {\n            state.master_input.extend(key.typed());',
        '        if key.types_text() {\n            state.master_input.extend(key.typed());',
        ['a_chord_is_neither_a_vault_key_nor_typing_and_altgr_types'],
    ),
    (
        'a chord works the lock screen',
        '        if !textline::is_plain(key.modifiers) {\n            return EventResult::Ignored;\n        }\n        match key.key {\n            Key::Enter => attempt_unlock(state),',
        '        match key.key {\n            Key::Enter => attempt_unlock(state),',
        ['a_chord_is_neither_a_vault_key_nor_typing_and_altgr_types'],
    ),
    (
        'AltGr is taken for Ctrl in the vault',
        '    let chord = textline::is_ctrl_chord(key.modifiers);',
        '    let chord = key.modifiers.ctrl;',
        ['a_chord_is_neither_a_vault_key_nor_typing_and_altgr_types'],
    ),
    (
        "a command's letter is typed into the search",
        '        _ if textline::types_into_field(key) => {',
        '        _ if key.types_text() => {',
        ['a_chord_is_neither_a_vault_key_nor_typing_and_altgr_types'],
    ),
    (
        'a chord works the list',
        '        _ if !textline::is_plain(key.modifiers) => EventResult::Ignored,\n',
        '',
        ['a_chord_is_neither_a_vault_key_nor_typing_and_altgr_types'],
    ),
    (
        'AltGr is taken for Ctrl in the generator',
        '    let ctrl = textline::is_ctrl_chord(key.modifiers);',
        '    let ctrl = key.modifiers.ctrl;',
        ['a_chord_is_neither_a_vault_key_nor_typing_and_altgr_types'],
    ),
    (
        "a chord moves the generator's length",
        '    if textline::is_plain(key.modifiers) && matches!(key.key, Key::Left | Key::Right) {',
        '    if !key.modifiers.ctrl && matches!(key.key, Key::Left | Key::Right) {',
        ['a_chord_is_neither_a_vault_key_nor_typing_and_altgr_types'],
    ),
    (
        "a command's letter is typed into a backup's password",
        '            if textline::types_into_field(key) =>',
        '            if key.types_text() =>',
        ['a_chord_is_neither_a_vault_key_nor_typing_and_altgr_types'],
    ),
    (
        'a chord answers a vault dialog',
        '        _ if !textline::is_plain(key.modifiers) => Then::Nothing,\n',
        '',
        ['a_chord_is_neither_a_vault_key_nor_typing_and_altgr_types'],
    ),
    (
        'AltGr+Q closes the window',
        '            && textline::is_ctrl_chord(key.modifiers)',
        '            && key.modifiers.ctrl',
        ['a_chord_is_neither_a_vault_key_nor_typing_and_altgr_types'],
    ),
    # -- the auto-lock, 2026-10-03: no clock was asked for, so it never fired
    (
        'no clock is asked for',
        '        self.vault\n'
        '            .auto_lock_in(self.now)\n'
        '            .map(std::time::Duration::from_secs)\n',
        '        None\n',
        [LOCKS_ITSELF],
    ),
    (
        "the auto-lock falls due at the last use, not a timeout after it",
        '        let due = self.last_access.saturating_add(timeout_seconds);\n',
        '        let due = self.last_access;\n',
        [LOCKS_ITSELF],
    ),
    (
        "a tick's part second is dropped",
        '                    let total = carry_ms.saturating_add(*elapsed_ms);\n',
        '                    let total = *elapsed_ms;\n',
        [LOCKS_ITSELF],
    ),
    (
        'the part second is not carried',
        '                    *carry_ms = total % 1000;\n',
        '                    *carry_ms = 0;\n',
        [LOCKS_ITSELF],
    ),
    (
        "a window's clock is not the wall clock",
        '            Clock::Wall => self.now = unix_now(),\n',
        '            Clock::Wall => {}\n',
        [WALL_CLOCK],
    ),
    (
        'the lock is looked for only at a tick',
        '        if self.vault.should_auto_lock(self.now) {\n'
        '            self.lock_vault();\n'
        '            return true;\n',
        '        if matches!(event, Event::Tick { .. }) && self.vault.should_auto_lock(self.now) {\n'
        '            self.lock_vault();\n'
        '            return true;\n',
        [LOCK_DUE],
    ),
    # -- the text boxes, the toolkit's fields (c-e-a-theme-can-shape-the-controls)
    (
        'a text box is drawn the same wherever the pointer is',
        '            hovered: self.hover == Some(target),\n',
        '            hovered: false,\n',
        [LOCK_BOX, SEARCH_BOXES],
    ),
    (
        'no text box is marked where typing goes',
        '            focused: self.typing_into() == Some(target),\n',
        '            focused: false,\n',
        [LOCK_BOX, SEARCH_BOXES, FIRST_RUN_BOXES],
    ),
    (
        'a wrong text box is not red',
        '            invalid: wrong,\n',
        '            invalid: false,\n',
        [LOCK_BOX, FIRST_RUN_BOXES],
    ),
    (
        'the search box is marked under the file dialog',
        '        if self.picker.is_open() {\n'
        '            return None;\n'
        '        }\n'
        '        if !self.vault.is_unlocked() {\n',
        '        if !self.vault.is_unlocked() {\n',
        [SEARCH_BOXES],
    ),
    (
        'the lock screen box is not marked',
        '                Gate::Unlock => Some(Target::MasterInput),\n',
        '                Gate::Unlock => None,\n',
        [LOCK_BOX],
    ),
    (
        "the first run's keyboard is always in the first box",
        '                Gate::Create(form) if form.confirming => Some(Target::ConfirmPassword),\n',
        '',
        [FIRST_RUN_BOXES],
    ),
    (
        "the restore dialog's box is not marked",
        '            Some(VaultDialog::RestorePassword { .. }) => return Some(Target::RestoreInput),\n',
        '',
        [FIRST_RUN_BOXES],
    ),
    (
        'the search box is marked under a vault dialog',
        '            Some(_) => return None,\n',
        '            Some(_) => {}\n',
        [SEARCH_BOXES],
    ),
    (
        'the search box keeps the keyboard from the new-entry form',
        '            return Some(Target::NewField(form.focused));\n',
        '            let _ = form;\n',
        [SEARCH_BOXES],
    ),
    (
        'the pointer is not followed',
        '                MouseEventKind::Move => self.pointer = Some((mouse.x, mouse.y)),\n',
        '                MouseEventKind::Move => {}\n',
        [LOCK_BOX, SEARCH_BOXES],
    ),
    (
        'leaving the window leaves a box lit',
        '                MouseEventKind::Leave => self.pointer = None,\n',
        '                MouseEventKind::Leave => {}\n',
        [LOCK_BOX],
    ),
    (
        'a change of light asks for no repaint',
        '        match response {\n'
        '            Response::Idle if self.hover != lit => Response::Redraw,\n'
        '            other => other,\n'
        '        }\n',
        '        let _ = lit;\n'
        '        response\n',
        [LOCK_BOX],
    ),
    (
        'the light is settled only when the pointer moves',
        '        self.hover = self.text_box_under_pointer();\n',
        '        if matches!(event, Event::Mouse(_)) {\n'
        '            self.hover = self.text_box_under_pointer();\n'
        '        }\n',
        [SEARCH_BOXES],
    ),
    (
        'a button under the pointer counts as a box',
        '        self.target_at(x, y).filter(|t| t.is_text_box())\n',
        '        self.target_at(x, y)\n',
        [LOCK_BOX],
    ),
    (
        'a short master password is not the box made red',
        '        form.wrong = Some(Target::NewPassword);\n',
        '',
        [FIRST_RUN_BOXES],
    ),
    (
        'a second password that does not match is not the box made red',
        '        form.wrong = Some(Target::ConfirmPassword);\n',
        '',
        [FIRST_RUN_BOXES],
    ),
    (
        'typing leaves the box red',
        '                field.extend(key.typed());\n'
        '                form.error = None;\n'
        '                form.wrong = None;\n',
        '                field.extend(key.typed());\n'
        '                form.error = None;\n',
        [FIRST_RUN_BOXES],
    ),
    (
        'a Backspace leaves the box red',
        '                    field.pop();\n'
        '                    form.error = None;\n'
        '                    form.wrong = None;\n',
        '                    field.pop();\n'
        '                    form.error = None;\n',
        [FIRST_RUN_BOXES],
    ),
    (
        'a refused master password is not shown red',
        '        state.field_state(Target::MasterInput, state.unlock_failed),\n',
        '        state.field_state(Target::MasterInput, false),\n',
        [LOCK_BOX],
    ),
    (
        "a refused backup password is not shown red",
        '            error.is_some(),\n',
        '            false,\n',
        [FIRST_RUN_BOXES],
    ),
    (
        "the text boxes take the toolkit's focus width, not the user's",
        '        self.focus_ring_width = settings.focus_ring_width();\n',
        '        let _ = settings;\n',
        [LOCK_BOX, SEARCH_BOXES, FIRST_RUN_BOXES],
    ),
    # -- the auto-lock slider, 2026-10-03: a number nothing set, a knob for nothing
    (
        "the panel shows the slider's own time, not the vault's",
        '        if !shown.is_dragging() {\n'
        '            shown.set_value(f64::from(self.vault.auto_lock_minutes));\n'
        '        }\n',
        '',
        [SLIDER_SETS],
    ),
    (
        "a drag's minutes are not shown as it goes",
        '        if !shown.is_dragging() {\n',
        '        if true {\n',
        [SLIDER_SETS],
    ),
    (
        'letting a drag go keeps nothing',
        '        if let Some(guitk::slider::SliderEvent::Confirmed(minutes)) = response.event() {\n'
        '            self.set_auto_lock(minutes);\n'
        '        }\n'
        '        (response.is_taken()',
        '        (response.is_taken()',
        [SLIDER_SETS],
    ),
    (
        'a drag keeps every minute it passes',
        '        if let Some(guitk::slider::SliderEvent::Confirmed(minutes)) = response.event() {\n'
        '            self.set_auto_lock(minutes);\n'
        '        }\n'
        '        (response.is_taken()',
        '        if let Some(event) = response.event() {\n'
        '            self.set_auto_lock(event.value());\n'
        '        }\n'
        '        (response.is_taken()',
        [SLIDER_SETS],
    ),
    (
        "the slider's keys keep nothing",
        '        if let Some(guitk::slider::SliderEvent::Confirmed(minutes)) = response.event() {\n'
        '            self.set_auto_lock(minutes);\n'
        '        }\n'
        '        response.is_taken().then_some',
        '        response.is_taken().then_some',
        [SLIDER_SETS],
    ),
    (
        'the slider takes Up and Down from the entry list',
        '        if !its_key && !self.auto_lock.is_dragging() {\n'
        '            return None;\n'
        '        }\n',
        '',
        [SLIDER_SETS],
    ),
    (
        'the slider is moved with another panel up',
        '            && self.detail_view == DetailView::Settings\n',
        '',
        [SLIDER_GUARDED],
    ),
    (
        'the slider is moved through a vault dialog',
        '            && self.dialog.is_none()\n'
        '    }\n',
        '    }\n',
        [SLIDER_GUARDED],
    ),
    (
        'the slider is moved through the lock screen',
        '    fn settings_live(&self) -> bool {\n'
        '        self.vault.is_unlocked()\n'
        '            && self.detail_view',
        '    fn settings_live(&self) -> bool {\n'
        '        self.detail_view',
        [SLIDER_GUARDED],
    ),
    (
        'a press on the slider is not use of the vault',
        '        if matches!(\n'
        '            mouse.kind,\n'
        '            MouseEventKind::Press(_) | MouseEventKind::Release(_)\n'
        '        ) {\n'
        '            state.vault.touch(state.now);\n'
        '        }\n',
        '',
        [SLIDER_GUARDED],
    ),
    (
        'the pointer passing over the slider is use of the vault',
        '            MouseEventKind::Press(_) | MouseEventKind::Release(_)\n',
        '            MouseEventKind::Press(_) | MouseEventKind::Release(_) | MouseEventKind::Move\n',
        [SLIDER_GUARDED],
    ),
    (
        'the slider has no place among the controls',
        '    frame.hit(Target::AutoLock, placement.hit());\n',
        '',
        [SLIDER_SETS],
    ),
    (
        'the event that finds the lock due is taken as well',
        '    if state.advance_clock(event) {\n'
        '        // Locked by the time that has passed. The event is not taken: it was\n'
        '        // meant for a vault that is no longer open -- a key would go into the\n'
        '        // master password, a press land on the lock screen.\n'
        '        return EventResult::Consumed;\n'
        '    }\n',
        '    state.advance_clock(event);\n',
        [LOCK_DUE],
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
