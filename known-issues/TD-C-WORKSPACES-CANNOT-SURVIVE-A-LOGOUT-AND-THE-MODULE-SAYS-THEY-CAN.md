### TD-C-WORKSPACES-CANNOT-SURVIVE-A-LOGOUT-AND-THE-MODULE-SAYS-THEY-CAN — 2026-08-24 — OPEN (the module no longer says they can, and the format round-trips as of 2026-09-03; the caller is still missing)

**In short.** A *workspace* here is a saved window layout — "Development" with
an editor, a terminal and a browser at particular positions — that the user
names once and can then restore with one keystroke. The module that implements
them opens by promising "session persistence across logouts/reboots". It does
not do that. Nothing in the module ever reads or writes a file, so every
workspace the user creates exists only in the running shell's memory and is
gone the moment the shell exits. There *is* an `export_workspaces()` that turns
the list into a string, but nobody calls it, nothing can read the string back,
and the string leaves out most of what a workspace is.

**Where:** `gui/desktop/src/session_mgr.rs`.

- The module doc (lines 1–8) claims persistence across logouts/reboots.
- `SessionManager::export_workspaces` (line 418) is the only serialiser. It is
  called from exactly one place — its own unit test, `export_workspaces` at
  line 1081. `grep -rn 'session_mgr\|SessionManager\|WorkspacePicker' gui apps`
  finds a single hit outside the file itself: `pub mod session_mgr;` in
  `gui/desktop/src/lib.rs:131`. The type is declared and never constructed by
  the shell.
- There is no counterpart reader. No `import`, no `parse`, no `load`, no
  `from_str`, no `serde` derive anywhere in the module.

**What the format drops.** `Workspace` has eleven fields. The exported line
`workspace:{id}:{name}:{icon}:{auto_launch}` carries four of them. Missing:
`description`, `created_at`, `last_used` (which is what `sort_by_recent`
orders on), `shortcut` (the "Super+1" the picker draws), `pinned_desktop`, and
`color` (the user's colour tag). The whole `SessionState` — the live snapshot
`save_session`/`restore_session` operate on — is not exported at all. So even
if a reader were written against this format, restoring would silently discard
the user's shortcut and colour tag and reset every workspace's recency, which
would reorder the picker on every reboot.

**The format is also ambiguous, which is why the reader cannot simply be
written.** Fields are joined with `:` and no field is escaped or quoted, but
`name`, `icon`, `app_id` and `title_hint` are all free-form user strings. A
workspace named `Build: nightly` produces a line with six colon-separated
fields where the parser expects four, and a window whose title hint contains a
colon does the same. `state` is written with `{:?}`, i.e. the Rust `Debug`
spelling of the enum, which is a debugging aid rather than a format — renaming
a variant would silently change the file format. Writing a reader against this
format would be writing a parser for a format that cannot round-trip its own
inputs.

**What the proper fix looks like.** Not "add an importer". The format has to be
replaced first, and by the project's own configuration rule that means YAML
(`design.txt`: YAML for configuration, with a library that preserves comments
and formatting), covering all eleven `Workspace` fields plus `SessionState`,
with the enum spelled explicitly rather than via `Debug`. Then a reader, then
the shell actually calling both at the right moments — which needs the shell
event loop, since something has to notice "the user is logging out" and "the
shell just started". Until the shell can be driven
(`TD-C-THE-SHELL-CAN-DRAW-ITSELF-AND-NOBODY-CAN-ASK-IT-TO`), there is no moment
to hang the save and load calls on. The honest interim step, which costs
nothing and is not deferred, is the doc comment: it should say what the module
does today rather than what it is intended to do.

### Half done 2026-09-03 — the format exists and round-trips; the caller does not

Both steps this entry named as doable were done. What is left is the shell event
loop, which is another entry's problem.

**The doc comment no longer claims persistence.** It says what the module does
today, names what is missing, and points at the entry that owns the missing
half. The old line ("Also handles session persistence across logouts/reboots")
is removed rather than softened: a doc comment describing an intention in the
present tense is how a missing feature stops being noticed, which is exactly
what happened here for however long it stood.

**The format is replaced, and it round-trips.** `export_workspaces` is gone;
`SessionManager::to_yaml` and `load_yaml` are new. YAML through `yamldoc`,
because `design.txt` says configuration is YAML and `yamldoc` preserves the
comments and formatting of a file a user has edited.

| the old format | now |
|---|---|
| 4 of `Workspace`'s 11 fields | all 11, plus every field of `SessionState` |
| fields joined with `:`, nothing quoted | `yamldoc` quotes; a colon in a name is a non-event |
| window mode via `{:?}` | `SavedWindowMode::as_yaml`, an explicit `match` |
| no reader, and none writable | `load_yaml`, with round-trip tests |

The `{:?}` point is worth keeping: it made the *Rust identifier* the file
format, so renaming a variant would silently change what every saved file meant
and the compiler would say nothing, because `Debug` is still implemented. The
`match` in `as_yaml` turns that into a non-exhaustive-match error.

Windows are an index-keyed map rather than a sequence because `yamldoc`'s
sequences hold strings, not maps; keys are zero-padded so a plain sort keeps
them in order, which `workspaces_and_windows_come_back_in_the_order_they_went_out`
pins — the picker's list depends on it.

**Six tests.** Field-by-field round-trip (asserted per field, because a
serialiser that drops a field and a reader that ignores it agree with each other
perfectly); the colon case this entry named, exercising a name, a description
and a title hint at once; `None` versus `Some("")` for the three optional
fields, which a format writing both as empty would have collapsed on the first
save; ordering; every window mode by its explicit spelling, plus `"Maximized"`
and an unknown spelling both reading as `None`; and colour including alpha.

**Deliberately not defaulted:** a window whose mode this build does not
recognise is *skipped*, not read as `Normal`. A file written by a newer build
may hold a mode this one lacks, and un-maximising a window the user had
maximised looks like the layout being restored wrong rather than not being
restored. `an_unreadable_window_is_skipped_rather_than_defaulted` pins it.

**Still open, and it is the whole remaining point:** nothing calls either
function. There is no moment to hang a save on until the shell can notice a
logout, and none to hang a load on until it can notice a start — see
`TD-C-THE-SHELL-CAN-DRAW-ITSELF-AND-NOBODY-CAN-ASK-IT-TO`. A user's workspaces
still do not survive a logout. What changed is that the reason is now solely the
missing caller, rather than the missing caller *and* a format that could not
have been read back anyway.

**Trigger:** the format and the reader can be done at any time; the save/load
calls are sequenced after the shell event loop.

**If never fixed:** the feature is a demo. A user who spends ten minutes
arranging and naming three workspaces loses all three at logout, with no
warning that they were never saved — which is worse than the feature being
absent, because the naming UI implies durability. The module doc actively
misleads the next person to read it into thinking the persistence exists and
merely needs wiring.
