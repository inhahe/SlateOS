## `C-DEFRAG-HAS-NO-WAY-TO-SCAN-A-DRIVE` (lane C, 2026-08-26) — **open**, missing backend

**In short:** The disk defragmenter now opens as a real window and every control
in it works, but it has nothing to defragment. Nothing in SlateOS can yet list
the volumes on a machine or read where a file's pieces physically sit on one, so
the app opens on an empty drive list and stays there. It is a complete, tested
front end waiting for a back end.

**What is missing, precisely.** Two things, neither of which exists anywhere in
the tree:

1. **Volume enumeration.** `DefragUI::set_drives` takes a `Vec<DriveInfo>` and has
   no caller outside tests. There is no service, syscall or library that can
   answer "what drives are attached and how full are they?"
2. **A block map.** `analyze_drive(&BlockMap, &[FileFragInfo])` — the function
   that produces everything the app displays — needs to be told which of a
   volume's blocks are free, which are used, and which file owns each one. That
   is filesystem-internal data; there is no `fs/` tree in lane C and no interface
   that exposes it.

**How the gap is held open rather than papered over.** `DefragUI` carries a
`scan: Option<ScanFn>` seam, where `ScanFn = fn(&DriveInfo) -> Option<(BlockMap,
Vec<FileFragInfo>)>`. In the shipping binary `main` leaves it `None`. The
predicate `DefragUI::action_available` gates **both** the Analyze button's hit box
and its greying off the same condition, so with no scanner the button is *absent*
rather than present-and-dead — a control that cannot work does not get drawn.
Every test that exercises the Analyze path has to install a scanner itself
(`always_scans` in the test module), which makes the missing backend impossible to
forget while working on this file.

**Two alternatives were rejected.** Inventing plausible fragmentation numbers so
the app has something to draw would be a lie in the UI — a defragmenter that
reports 34% fragmentation on a drive it never read is worse than one that reports
nothing. Shipping the Analyze button wired to a function that always fails would
be a control that looks live and does nothing, which is the exact failure this
lane has been removing from app after app (see
`C-RENDERER-AND-HIT-TEST-DERIVE-THE-SAME-LAYOUT-SEPARATELY`).

**Proper fix.** When a filesystem service exists that can enumerate volumes and
report block allocation, write the real `ScanFn` against it and set
`ui.scan = Some(...)` in `main`, plus a drive-enumeration call feeding
`set_drives`. Nothing in the UI needs to change: the seam was chosen so the
window is already correct on the day the data arrives. Until then the app is
honest about having no data.

**Where it bites:** `apps/defrag/src/main.rs` — `ScanFn`, `DefragUI::scan`,
`DefragUI::analyze_selected_drive`, `DefragUI::action_available`, and `main`.
