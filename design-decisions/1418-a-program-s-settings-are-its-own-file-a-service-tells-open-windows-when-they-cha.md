## 1418. A program's settings are its own file; a service tells open windows when they change

**Date:** 2026-09-27 &middot; **Decided by:** Operator (Claude recommended A; the operator chose A, with C added beside it) &middot; **Lane:** C, with E

**In short:** Each program keeps its settings in a file of its own under the
user's settings folder -- `~/.config/<program>.yaml`, in YAML as the design
requires -- which the program writes itself. In addition, a settings service
tells every open window when a setting changes, so a change shows at once
without reopening anything. The service is *beside* the saving, not in front of
it: saving a setting is the program writing its file, and the service being
down or slow cannot lose one.

**The question:** `open-questions.md` C-Q26 (now resolved).

| Part | What | Whose |
|---|---|---|
| The file | one YAML file per program, written through `gui/settingsfile` (which already does this, comments preserved) | lane C (`settingsfile`, exists) |
| The four programs that asked | the lock screen's clock and date, the markdown editor's autosave, the password generator's rules, the explorer's copy-onto-an-existing-name choice, kept across restarts | lane E |
| Live changes | a service that watches the settings files and tells open programs what changed -- separate from the function that saves, as the operator specified | lane C |

**How "not as the same function that saves" was read:** the service does not
own the write path. A program saving its settings writes its own file whether
or not the service is running; the service's job is to notice and tell others.
If the operator meant something else, this is the entry to correct.
