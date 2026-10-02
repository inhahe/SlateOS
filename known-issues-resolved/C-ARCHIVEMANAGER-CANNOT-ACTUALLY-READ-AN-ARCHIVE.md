## C-ARCHIVEMANAGER-CANNOT-ACTUALLY-READ-AN-ARCHIVE (lane C, 2026-08-26) — FIXED 2026-09-02

**Status, 2026-09-02: all six toolbar buttons now work against real archive
bytes.** The entry below is the original 2026-08-26 report and is kept for the
history; it describes a program that no longer exists. Read the "How it was
actually fixed" section at the end of this entry, not the numbered plan in the
middle — three of that plan's five steps landed differently from how it
guessed.

**In short (the fix):** the Archive Manager reads, verifies, extracts, creates
and modifies real ZIP files. Open reads a path off the disk through a file
picker; Test inflates every member and checks it against the CRC-32 the archive
declares; Extract All and Extract Selected write the members out with a
traversal defence; New writes a fresh empty archive; Add puts a file into the
open one; and Delete now removes members **from the file**, not merely from the
list on screen.

**In short (the original bug):** the Archive Manager can now be clicked, but it has nothing to click
*on* except a hard-coded sample listing. Pressing Open, Extract All, Extract
Selected, Add or Test does not read or write a single byte of any file on disk —
each one just writes "not yet implemented — no archive back end" into the status
bar. The window, the tree, the sorting and the selection are all real; the
archive behind them is a fiction.

**Where it lives:** `apps/archivemanager/src/main.rs`, `AppState::run_toolbar`
(the `other =>` arm) and `AppState::open_entry` (the non-directory branch).
`create_sample_archive()` is what populates the list at startup.

**What a user sees:** the program opens showing a listing of files that are not
on their computer, and every button that would touch a real archive politely
declines. Nothing lies about having succeeded — that was deliberate — but
nothing works either.

**What the proper fix looks like:** a ZIP reader built on the two crates already
in the tree, `deflate/` (lane A's inflate implementation, already used by
`gui/imagecodec`) and `crc32/`. Concretely:

1. Parse the end-of-central-directory record, then the central directory, into
   `ArchiveEntry` values — the struct already has every field the format
   carries (`crc32`, `compressed_size`, `method`, `modified`).
2. `Open` reads a real path and replaces `create_sample_archive()`. This needs a
   file-picker; `guitk::dialog::FileDialog` exists but has no `handle_click` and
   no `read_dir`, so embedding it is its own piece of work (see
   `TD-C-FILEDIALOG-PATHS-ARE-STRINGS…`).
3. `Test` inflates each entry and compares the CRC-32 it computes against the
   one in the directory. `ArchiveTestResults` and `TestResult::CrcMismatch`
   already exist to receive the answer, and `OperationProgress` already exists
   to report the sweep.
4. `Extract All` / `Extract Selected` write the inflated bytes out. Paths must
   be validated against `..` traversal before anything is written — a ZIP is an
   untrusted input and an entry named `../../etc/passwd` is the oldest bug in
   the format.
5. Writing (`Add`, `Delete` against the file rather than the list) needs a
   deflate *compressor*, which the `deflate/` crate may not have; storing
   uncompressed (method 0) is a correct first cut if not.

**Filed with lane A:** `requests/c-a-zip-is-trapped-in-the-kernel-binary.md`.
`kernel/src/fs/zip.rs` is already a complete 799-line ZIP reader *and* writer
with ZIP64 and a boot self-test -- but it is a module of a binary crate, so
nothing outside the kernel can depend on it. Writing a second ZIP parser here
would be the same mistake `requests/c-a-two-inflates.md` documents, with more
force: a ZIP parser reads untrusted input, so a second copy is a second attack
surface for `..` traversal, the backwards end-of-central-directory scan and the
ZIP64 shadow fields. **This entry stays open until that crate exists**, and the
"proper fix" above is then a matter of calling it rather than writing it.

**Why the window was wired first rather than waiting:** the window and the back end
fail independently and are tested completely differently — one against rendered
geometry, the other against archive bytes. This mirrors what was done for
`apps/diskcleanup`, which was wired first and given a real back end in the
following commit.

### How it was actually fixed

The blocker cleared first: lane A lifted `kernel/src/fs/zip.rs` out of the
kernel binary into the **`ziparchive/` crate**, which is what
`requests/c-a-zip-is-trapped-in-the-kernel-binary.md` asked for. No second ZIP
parser was written, so the `..`-traversal, backwards-EOCD-scan and ZIP64
handling all still live in exactly one place.

`apps/archivemanager/src/backend.rs` is that crate plus the app's own policy.
The reading half landed on 2026-08-26; the writing half on 2026-09-02.

| Button | What it does now |
|---|---|
| Open | `backend::open` → `ziparchive::parse`. A failure keeps the archive already open and says why, rather than clearing the window. |
| Test | Inflates every member, checks size and CRC-32, and reports an encrypted member as needing a password rather than as damage. |
| Extract All / Selected | `safe_destination` splits on **both** `/` and `\`, requires every component to be `Component::Normal`, and refuses a Windows drive prefix (`PathBuf::push("C:")` *replaces* rather than extends, which is a traversal all by itself). |
| New | `backend::create_empty` writes an empty archive and then **opens it**, so every later Add and Delete rewrites that file rather than a model built in memory. |
| Add | `backend::read_for_add` reads the file, then the same rewrite Delete uses. An added file with an existing member's name displaces it: a ZIP with two members of one name is legal to write and ambiguous to read, so writing one is a way of not deciding. |
| Delete | Rewrites the archive without the removed members. It used to say "Removed N entries **from the list**" — true about the wrong noun. |

**Three deviations from the plan above**, recorded because the plan is what a
later reader would otherwise trust:

- Step 2's file-picker worry was **half** right, and the half that survived is
  still open. The directory-listing gap closed — the app hands the dialog a
  listing through `list_directory`/`set_entries` — so a picker is now four
  `DialogPurpose` variants over one dialog. But `FileDialog` **still records no
  hit targets and has no `handle_click`**: every picker in this program, the two
  new ones included, is keyboard-only. Clicking a filename in the Open, New or
  Add dialog does nothing. Tracked at
  `C-FILEDIALOG-IS-KEYBOARD-ONLY-SO-EVERY-PICKER-IN-THE-OS-IGNORES-CLICKS`.
- Step 5 guessed that `deflate/` might have no compressor and that storing
  uncompressed would be "a correct first cut". It does have one;
  `ziparchive::create` deflates, and members come back out smaller than they
  went in.
- The plan did not mention the property that turned out to matter most, below.

### The property the writer is built around: never lose a member

A rewrite drops everything it cannot reproduce. That makes a *partial* success
the worst possible outcome — silent data loss inside a file the user still
believes is intact — so `backend::save` reproduces every member first and only
then writes anything. If any member will not come back (encrypted, or it fails
the same `extract_entry` the Test button uses), the whole save is refused and
the file is untouched. Four consequences, each with its own regression test:

- **Refused whole, not partly.** `a_member_that_cannot_be_reproduced_stops_the_whole_save_and_the_file_is_untouched`
  asserts the archive is byte-identical afterwards and that no temporary is
  left beside it.
- **Written beside, then renamed over.** `fs::write` straight onto the target
  would truncate the user's archive as its first action, so a full disk halfway
  through would destroy the original and leave a fragment wearing its name. The
  temporary is a sibling, so the rename stays inside one filesystem and is
  atomic on both Windows and Unix.
- **The raw member name, never the displayed one.** `ArchiveEntry::path` is a
  lossy rendering built for a column. Writing it back would rename any member
  whose name has no UTF-8 reading to one containing U+FFFD, and would collapse
  two members onto one name if they differed only in those bytes.
  `a_name_with_no_utf8_reading_survives_a_rewrite_unchanged` covers it, and
  `read_for_add` takes the added file's name from `OsStr::as_encoded_bytes` for
  the same reason.
- **A refused save puts the rows back.** Delete edits the list before saving, so
  a refusal has to undo that edit or the window ends up describing an archive
  that still contains the rows. The undo is a snapshot restored from memory, not
  a re-read: the likeliest reason a write failed (a removed drive, a vanished
  file) is a reason the read would fail too, which would leave the edit standing
  with nothing to correct it. `a_delete_the_archive_will_not_accept_puts_the_rows_back`
  drives that path by replacing the archive with a *directory* of the same name,
  which no filesystem will rename a file over.

**Add and Delete save immediately** rather than accumulating into a Save button,
matching 7-Zip and WinZip. The two buttons that can destroy something are
disabled — not merely failed at the end — when the open model has no bytes
behind it (`source: None`), since a rewrite from such a model would replace the
user's archive with an empty one.

**Still open, separately:** `TD-C-ARCHIVEMANAGER-HOLDS-THE-WHOLE-ARCHIVE-IN-MEMORY`.
The rewrite holds the old archive, every member's plaintext and the new archive
at once, so `MAX_ARCHIVE_BYTES` bounds it rather than streaming doing so.
