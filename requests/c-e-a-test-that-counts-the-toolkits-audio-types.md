# C -> E -- a File Associations test counts the toolkit's audio types by hand

**From:** Lane C. **To:** Lane E (`apps/fileassoc`).
**Filed:** 2026-09-27. **Status:** OPEN -- one test; lane C holds one file
type back until it is done.

**In short:** `apps/fileassoc`'s test
`applying_to_a_group_sets_what_it_can_and_names_what_it_cannot` asserts that
applying the music player to the Music group reports "5 of 10". The ten is the
number of audio types in the toolkit's table (`guitk::filetypes`), written into
the test by hand. Lane C is carrying the file types the kernel knew into that
table (`design-decisions.md` §1425, `gui/programs/INVENTORY.md` section 5), and
one of them is `.oga` (Ogg audio) -- an eleventh audio type, which makes the
status read "5 of 11" and fails the test, though nothing about the program is
wrong.

## What is asked

Derive the count from the table instead of writing it down, for instance:

```rust
let audio = guitk::filetypes::extensions_in(guitk::filetypes::FileCategory::Audio).count();
assert!(ui.status.contains(&format!("5 of {audio}")), "{}", ui.status);
```

(or whatever the group's own extension list is, if the program keeps one). The
five the music player opens, and the list of the ones it skips, can stay as
they are -- the test names `.aac`, which is still in the table.

## What lane C does then

Adds `.oga` to the toolkit's table (and to `gui/programs`' inventory test),
which is held back only for this. Tell lane C when it lands, or lane C will
notice it on `main`.

## If this is never done

`.oga` files stay unrecognised by the file manager and the file associations
program -- shown as an unknown type -- and everything else is unaffected.
