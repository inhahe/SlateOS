## 1210. The reminders list is kept like the calendar, and a repeating reminder done comes round again

**Date:** 2026-09-26
**Lane:** E
**Decided by:** Claude (autonomous) -- Claude's to revisit

**In short:** A reminders app that could not add a reminder: the list filled
only from a JSON file, nothing in it could be changed or deleted, a reminder's
steps could not be reached, and everything was gone when the window closed.
Now N adds a reminder, E changes the one selected, Delete asks and deletes, and
the steps are written, ticked and removed in the same form -- all from the
keyboard, which is how the program has always been driven. Every change is
written at once to `reminders/tasks.txt` in the settings folder, the calendar's
kind of file (§1209). And a repeating reminder, done, comes back for its next
time instead of being finished for good.

### A repeating reminder, done

| | Comes round again (chosen) | Marked done, like any other (before) |
|---|---|---|
| "Take the pills", daily, done today | due again tomorrow at the same time | gone from Today, Upcoming and All, forever |
| What its repeat is for | when it is next due | nothing: a label |
| Missed three times, then done | due next at the first time still to come | -- |

The next time is stepped from the due date, not from when it was done, so a
reminder due at 09:00 stays at 09:00; past times are skipped, so doing a
weekly chore late does not leave it due three weeks ago.

### Smaller calls

- **Keyboard only.** The program binds every key it uses and takes no pointer
  by construction (known-issues, the no-pointer entry: "keyboard-driven, no
  pointer layer needed"); the form keeps to that: Tab between fields, Left and
  Right to choose, Enter to save.
- **Steps are edited in the form**: a Steps row where Up and Down choose, Space
  ticks and Delete removes, and an "Add a step" field where Enter adds -- plus
  Shift+1-9 to tick one from the list without opening the form.
- **A new reminder is due at the next whole hour, today** -- clearing the date
  makes it undated -- and a reminder saved where the current view would hide
  it switches the view to All, so it does not vanish the moment it is made.
- **A change is counted by the store** (`TaskStore::revision`), as the
  calendar's is.

**Where it lives:** `apps/reminders/src/main.rs`: `tasks_text`, `parse_tasks`,
`next_due_after`, `TaskStore::{revision, from_tasks, complete_task}`,
`TaskForm`, `RemindersApp::{from_settings, load_tasks, keep, request_close,
answer, open_new_task, open_edit_task, save_form, delete_task, toggle_step,
notice_lines, handle_form_key, handle_confirm_key}`.

**How to reverse:** the format is `tasks_text`/`parse_tasks`; the repeat is
`complete_task`'s first branch.
