## TD-C-THE-WALLPAPER-SUBSYSTEM-CARRIES-PATHS-AS-TEXT-THROUGHOUT -- 2026-09-16 -- FIXED 2026-09-16

**Status:** FIXED 2026-09-16 (`9ce32d61f`) for the live chain -- the setting,
the shell and the settings app all carry a `PathBuf`, percent-encoded on disk
behind a version marker -- and the slideshow playlist followed on 2026-09-17
when rotation gained a consumer. Stamped 2026-09-24 by lane C, which found the
entry still reading as open a week later, and a comment in `wallpaper.rs`
still describing the playlist as text.

**In short:** choose a wallpaper whose filename is not text and the setting
that gets saved names a different file: the wallpaper silently does not appear
and the settings page shows a path nobody picked. The fix is not one line at
the point of saving -- the whole wallpaper subsystem, including the slideshow
playlist and the recent-picture history, holds paths as text.

**Date:** 2026-09-16. **Lane:** C.

**This is the one real defect** out of the 74 sites the lossy-decode checker
reports for `gui/` and `apps/` -- see
`TD-C-THE-LOSSY-DECODE-CHECKER-NEVER-LOOKED-AT-TWO-THIRDS-OF-THE-TREE`, where
six of the seven worst-looking turned out to be correct as written.

**How it is stored is no longer an open question.** It was raised as C-Q24 and
withdrawn the same evening: design-decisions §426 decided it in August --
percent-encode the bytes, behind a version marker -- and `gui/pathcodec` now
holds that encoding where every crate involved can reach it. What is left is
work, not a decision.

**Measured scope, so the next session does not have to guess and does not have
to state it twice:**

| Where | Sites | What changes |
|---|---|---|
| `gui/appearance/src/lib.rs` | 2 | `Settings.wallpaper: Option<String>` becomes `Option<PathBuf>`; encode on write, decode on read |
| `gui/desktop/src/session.rs` | 11 | passes a `&Path` to the widget instead of a `&str` |
| `gui/desktop/src/session/tests.rs` | 7 | fixtures |
| `apps/settings/src/main.rs` | 15 | **the actual bug**: line 912 stores `path.to_string_lossy()`; it should store the path |
| `gui/desktop/src/wallpaper.rs` | **3 items**: `WallpaperConfig.image_path`, `set_image`, `current_image_path` | the live chain, and only it -- see below |

**The last row is one third of what this entry first claimed, and the
correction is the point.** An hour after filing it I wrote: *"27 typed sites
across 41 public functions, 2,823 lines, 92 tests ... the fifth is a subsystem
whose playlist and history are lists of paths held as text, and converting it
is the actual work."* That measured the **file**. The work is the part of the
file something calls, and most of this one is not called at all:

| Mechanism | Writers | Readers |
|---|---|---|
| `WallpaperHistory` | **9 production sites** push to it | **none in production.** `go_back`, `go_forward` and `current` all exist and work; every caller of them is a test. |
| `SlideshowState` / `set_slideshow` | — | already recorded in `roadmap-detailed.md` §3.4: *"exists, takes a directory, an interval and a shuffle flag, and nothing outside the shell's tests calls it"* |
| `save_config` / `load_config` | — | a whole `key=value` persistence format whose only callers are its own tests. The real persistence is `appearance.yaml`, through `gui/appearance`. |

So converting the playlist and the history would be making **dead code
byte-correct**, which is worse than leaving it: a careful-looking conversion is
exactly what persuades the next reader that a thing is load-bearing. It is the
same trap as the two dead installer fields in
`TD-C-THE-INSTALLER-RECORDS-A-GRUB-PATH-NOTHING-EVER-READS`, met from the other
direction -- there the byte question was unanswerable because there was no
consumer; here it is answerable and *not worth answering*.

**The live chain, which is the whole task:** `appearance.yaml` ->
`Settings.wallpaper` -> `session.rs` -> `set_image()` -> `config.image_path`,
and back out through `current_image_path()`. Four files, and in `wallpaper.rs`
three items rather than a subsystem.

**A separate finding, not to be folded into the fix:** a 2,823-line module
whose live surface is three functions is worth a look in its own right. The
history is the sharpest case -- nine production sites faithfully recording into
a structure nothing in production ever reads.

**Corrected 2026-09-17, because the first version of this line was wrong and
wrong in the expensive direction.** It said there was "no `back` or `forward`
method in the type" and that the navigation "was never built". Both false:
`WallpaperHistory::go_back` and `go_forward` are there, they are implemented,
and they are tested. I had grepped for `pub fn back` and `pub fn forward`; they
are called `go_back` and `go_forward`.

That mistake is worse than having filed nothing, because it points the next
reader at the wrong work -- writing methods that already exist -- and they
would find that out only after starting. The true gap is narrower and
different: **nothing invokes them.** No keybinding, no menu item, no shell
command reaches `go_back`.

Two consequences for whoever picks this up:

* The work is a *caller*, not an implementation. The type is complete.
* ~~A caller needs one thing that genuinely does not exist: the entries are
  tagged strings and nothing parses them back.~~ **Done 2026-09-17:** the
  entries are a `WallpaperChoice` enum -- `Solid(Color)`, `SolidTheme`,
  `Image(PathBuf)`, `Slideshow(PathBuf)`, `Dynamic` -- so there is nothing to
  parse, and what `image:` should do with a path containing a colon cannot
  arise.
* **§858 removed the other blocker.** The history was recording every automatic
  slideshow advance, so at the default interval a twenty-entry buffer turned
  over in ten minutes and a user's own choice was evicted by a slideshow they
  left running. Only deliberate acts are recorded now.
* **What remains is a caller, and it is deliberately not built.**
  `roadmap-detailed.md`'s Desktop Background section has **no bullet for
  history navigation**, so adding a menu item would be inventing a feature
  rather than implementing one. The nearest thing to a specification is the
  kernel-side `wallpaper` kshell command's `history` subcommand
  (`roadmap.md` §2761, lane A), which is not this shell. If it is wanted, the
  desktop's context menu is the obvious home -- with the wrinkle that the menu
  is built once and reused, so items whose enabled state depends on there being
  something to go back to need it rebuilt when shown.

**A version marker is required, not optional.** Existing files hold the path
raw under `["wallpaper", "image"]`. Writing encoded text into the same key
would misread any existing path containing a literal `%` -- §426 met exactly
this and answered it with a marker whose absence means version 1. The same
shape fits here: write `["wallpaper", "image_encoding"] = "percent"` alongside,
and treat its absence as a raw path.

**Do not start at the producer.** The one-line change at
`apps/settings/src/main.rs:912` is the tempting entry point and it is the wrong
one: it would hand a correct `PathBuf` to a chain that flattens it two layers
down, so the bug would survive with its cause moved somewhere less obvious.
Start at `wallpaper.rs`, which is where the model actually lives.
