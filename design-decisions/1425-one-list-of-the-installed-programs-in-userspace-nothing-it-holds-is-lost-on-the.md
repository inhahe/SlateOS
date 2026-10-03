## 1425. One list of the installed programs, in userspace; nothing it holds is lost on the way

**Date:** 2026-09-27 &middot; **Decided by:** Operator (Claude recommended A or B; the operator chose B, with a condition) &middot; **Lane:** C (the library), A (the kernel's registry), E (the programs that read it)

**In short:** Four parts of the system each kept a list of which programs are
installed, and none could read another's. The one list will be a library in
userspace, which the start menu, Settings, the file manager and the file
associations program all read; the kernel's registry goes. Before anything is
deleted, every program, category, file type, MIME type and per-role default
that any of the four held is gathered and carried into the new place, so the
merge loses nothing.

**The question:** `open-questions.md` C-Q20 (now resolved). **Verbatim:** "B,
but I think before deleting anything else you should collect all of the
built-in apps, categories, MIME types, apps, file types, per-role defaults, etc.
and make sure they survive in the new place or wherever they belong."

**Order of the work:** (1) inventory all four lists -- the kernel's
`fs::appregistry`, the shell's `launcher.rs` database, `apps/fileassoc`, and the
per-role defaults that were in `default_apps.rs` (git history) -- into one table;
(2) the library under `gui/`, holding all of it, read by the shell first; (3) the
other readers move to it; (4) only then does lane A remove `fs::appregistry` and
decide what `fs::startmenu` and `/proc/startmenu` become.

**The operator's second point in the same answer**, about how the lanes
communicate: "whenever you take it upon yourself to do a task, add it to some
text file that other agents read ... or maybe send that you're starting the task
to all other agents ... only when the roadmap file doesn't clearly say that the
program or feature belongs to your own lane." The mechanism is being built as a
claim every lane can see at once (the same shared store the halts use); the
change to `CLAUDE.md` that would make it a rule is put to the operator directly,
because `CLAUDE.md` changes on the operator's own word, not on a relay.

**As built (lane C, 2026-09-27): steps 1 and 2, and the shell's half of 3.**

- **The inventory** is `gui/programs/INVENTORY.md`. There were fourteen sources,
  not four: besides the four the question named, the kernel had eight more
  modules with program or file-type data (`fs::defaultapps`,
  `fs::associations`, `fs::mime`, `fs::filetype`, `fs::pinnedapps`,
  `fs::startmenu`, `fs::applaunch`, and the empty `fs::openwith`/`fs::appstore`),
  and the toolkit's file types and `gui/associations` hold the rest. Every item
  is listed with where it now lives or why it does not; `tests/inventory.rs`
  holds that to the code.
- **The library** is `gui/programs` (the format is §1429): fifteen programs as
  desktop entries, fifty type defaults, fourteen roles.
- **Carried elsewhere:** 23 file extensions and nine kinds of content signature
  into `guitk::filetypes`; the kernel's default pins into the shell's first
  start. **Held back:** `.oga`, until lane E's File Associations test counts
  audio types from the table (their 3275adc99), and nine file types the toolkit
  had already decided to wait on.
- **Read by the shell:** `launcher::builtin_app_database` is the library's list.
  Found on the way: the shell's System Info row started `userspace/sysinfo`, the
  command-line tool, rather than the window (`sysinfo-app`).
- **Still to do:** lane E's programs read the library
  (`requests/c-e-read-the-one-list-of-programs.md`), then lane A removes the
  kernel's lists (`requests/c-a-the-kernels-app-registry-and-the-first-screen-hint.md`
  item 1).
