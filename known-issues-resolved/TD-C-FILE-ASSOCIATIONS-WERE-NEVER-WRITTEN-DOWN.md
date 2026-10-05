## TD-C-FILE-ASSOCIATIONS-WERE-NEVER-WRITTEN-DOWN -- FIXED 2026-09-14

**Date:** 2026-09-14. **Lane:** C. **Fixed the same day.**

**In short:** the File Associations program let you choose which application
opens each kind of file, and then threw the answer away when you closed the
window. It never read or wrote a file of any kind -- 4,826 lines, no
`std::fs`, no `PathBuf`, nothing. Every setting reverted to the built-in
default at the next start.

**What made it hard to see.** The program had a complete save format sitting
right there: `export_config` and `import_config`, a matching pair with eight
tests including a round-trip over deliberately hostile extension names. All of
it worked. None of it went anywhere -- `export_config` was reachable only as
text in a panel the user could read, and `import_config` had **no caller at
all** outside its own tests. So every signal a reader would use said the
feature was present: a format, tests, a round-trip, even a hardened escaping
scheme with a comment about a bug it had already fixed. The one thing missing
was the call that touches a disk.

That is the same shape as `apps/automator`'s mutation table and `guitk::svg`:
the work was done, carefully and with tests, on a component whose output
nobody collected.

**What was done.**

* Associations are a YAML document now, under `settingsfile`, like every other
  settings surface here -- `design.txt` says configuration is YAML "processed
  with a library that preserves comments and formatting", and this program had
  a hand-rolled `ext=app` grammar instead. The document is kept and *edited*
  rather than rebuilt, so a hand-written comment survives being saved over.
* `read_from` / `write_into` on the registry, and `FileAssocUI::load` /
  `from_document` / `persist` on the UI. The `load`/`from_document` split is
  the one `inputsettings` already has, and it exists for a concrete reason:
  48 tests build a `FileAssocUI`, and a constructor that read the config
  directory would make all 48 of them read the developer's own.
* Saved after every change rather than behind a Save button. There is no Save
  button and there should not be one: this program is a list of choices, and a
  choice that has to be confirmed elsewhere is a choice a user can lose.
* The old `ext=app` codec, its `CONFIG_META`, and the `AssocError::ParseError`
  variant are gone with it. `ParseError` carried the line number of a bad line;
  `yamldoc` repairs what it can and reports no line, and inventing one to keep
  the variant alive is exactly what the comment beside it warned against.

**Two things the tests caught that reading had not.**

1. **A cleared association came back.** `load` started from the built-in
   defaults and applied the file on top, so an association the user had
   *cleared* -- an absence from the file -- was indistinguishable from one
   never set, and the default under it won. Once a file exists it is now the
   whole truth about associations: the defaults are cleared before it is
   applied. The catalogue of file types and applications is untouched, because
   that is not a choice the user made.
2. **A cleared entry has to be removed from the document**, not merely omitted
   from what is written. Writing only what is present leaves the old key in the
   file, and it reads back as though the clear never happened.

**Verified by reintroduction**: with the `store` call disabled, and again with
the defaults left to override the file, `an_association_survives_a_restart`
fails and nothing else does.
