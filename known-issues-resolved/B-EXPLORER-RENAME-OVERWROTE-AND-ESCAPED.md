## B-EXPLORER-RENAME-OVERWROTE-AND-ESCAPED (lane C, 2026-08-16) — FIXED

**In short:** Renaming a file in the explorer could delete a different file.
If you renamed `draft.txt` to `notes.txt` while a `notes.txt` was already
sitting in the folder, the old `notes.txt` was destroyed — no warning, no
recycle bin, and the status bar said "Renamed to: notes.txt" as though
everything was fine. Typing a name like `../notes.txt` was worse: it moved the
file out of the folder you were looking at and clobbered whatever it landed
on. Fixed: rename now refuses a name that is already taken, and refuses names
that point outside the folder.

**Where:** `apps/explorer/src/main.rs`, `ExplorerState::rename_entry` and
`ExplorerState::create_folder`.

**Three defects, one root cause: the typed name was trusted.**

1. **Overwrite.** `fs::rename` overwrites its destination silently. The call
   was unguarded, so any rename onto an existing name destroyed that file.
2. **Directory escape.** `old_path.with_file_name(new_name)` does not
   constrain the result to the same directory. `../taken.txt` produced
   `dir/../taken.txt`, which resolves outside — so the rename both left the
   folder and overwrote a file the user had not selected and could not see.
   `create_folder` had the same hole through `current_path.join(name)`.
3. **Non-names.** `""` makes `with_file_name` return the *parent directory*,
   so an empty rename box operated on the directory itself. `.` and `..`
   likewise.

**The fix.** `validate_entry_name` rejects `""`, `.`, `..`, anything with `/`
or `\`, and anything with a NUL. It deliberately does **not** impose Windows'
character rules: `design.txt` specifies "all characters except `/` and null",
and an explorer that refuses names the OS accepts is its own bug. `\` is
rejected anyway because the app is developed and tested on Windows, where it
is also a separator — a name that escapes only on the dev host is a bug found
late. Then `rename_entry` refuses a destination that already exists.

**Case-only renames still work.** `notes.txt` → `Notes.txt` is a legitimate
rename, but on a case-insensitive filesystem the destination "exists" — it is
the source. `is_same_file` canonicalises both sides so that case is allowed
through. SlateOS's own filesystem is case-sensitive, where the two really are
distinct and `canonicalize` reports them as such, so the same code is correct
on both.

**Known race, accepted deliberately.** Between `new_path.exists()` and
`fs::rename` another process could create that file, and `std` has no portable
"rename only if the destination is free" (`RENAME_NOREPLACE` is Linux-only;
Windows needs `MoveFileEx` / `SetFileInformationByHandle`). The check is still
right: the realistic trigger is a user typing a name they can *see* in the
listing, not another process winning a microsecond. The `fileops` engine makes
the same tradeoff. If SlateOS's VFS ever exposes an atomic no-replace rename,
both call sites should move to it — noted here rather than in `todo.txt`
because the fix is one call, not a project.

**Reachability.** `rename_entry` and `create_folder` currently have no callers
— they are the public API the UI layer is meant to wire to. So this was not
yet user-reachable; fixing it now is what makes wiring them safe, and the
alternative was a data-loss bug waiting behind a keybinding.

**Tests:** `a_rename_onto_an_existing_file_does_not_destroy_it`,
`a_rename_cannot_escape_the_current_directory`,
`a_new_folder_cannot_escape_the_current_directory`,
`a_rename_rejects_names_that_are_not_names`,
`a_rename_to_the_same_name_is_harmless`, `an_ordinary_rename_still_renames`.
The first three were written before the fix and observed failing, which is
stronger evidence than injection after the fact.

**Sweep note.** Fifth find, and it sharpens the fourth's lesson. The recycle
bin bug and this one are the same shape — *an API permissive where the caller
needed it strict* — but this one adds the more useful signal: **the codebase
already knew.** `paste`'s doc comment says in as many words that it was
rewritten because "it overwrote an existing destination file without asking",
and `rename_entry` sat eleven lines away still doing exactly that. When a bug
is fixed on one path, grep for the other callers of the same primitive before
closing it out; the comment explaining the fix is itself the best search term.
