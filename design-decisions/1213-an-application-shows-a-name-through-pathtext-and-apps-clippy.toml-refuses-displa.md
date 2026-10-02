## 1213. An application shows a name through `pathtext`, and `apps/clippy.toml` refuses `display()`

**Date:** 2026-09-26
**Lane:** E
**Decided by:** Claude (autonomous) -- Claude's to revisit

**In short:** A file's name on this system can hold bytes that are not text.
Rust's usual way to show a path, `display()`, draws every such byte as the
same symbol (U+FFFD), so two different files can look identical on screen and
a message about one reads as a message about the other. Lane E's programs now
show every path and name through one method, `pathtext`'s `.shown()`, which
writes such a byte as its code instead (`caf\351.txt`) -- the rendering they
already used for names in the forty-seven places fixed earlier the same day.
A configuration file that only lane E's crates read makes `display()` a build
error there, so a new one cannot come back unnoticed.

### The choices

| Question | Chosen | Alternative | Why |
|---|---|---|---|
| Rendering | `quoting::escape_unprintable`: a byte that is not text, or a control character, as three octal digits; everything else, backslash included, as itself | `quoting::escape` (the `ls`/`tar` style, which also doubles `\`) | one rendering across lane E: the earlier lossy-decode fixes use it. `escape` is unambiguous where this is not (a name that really contains the four characters `\351` looks like one holding byte 0xE9), but doubles every backslash in a name a person typed: a rare confusion against a daily oddity. Revisit if the two are ever confused in practice. |
| Where it lives | a lane E crate, `apps/pathtext` | a function in `quoting` | `quoting` is lane B's, and a method that stands in for `display()` is an application convenience, not a quoting style |
| Shape | a trait method, `.shown()`, returning a `Display` | a free function returning a `String` | it replaces `.display()` where it stands, so no call site's receiver had to be re-read; it borrows, and renders only when formatted |
| Ratchet | `disallowed-methods` in `apps/clippy.toml` | a text-scanning gate in the pre-push hook | the compiler resolves which `display` is meant, so the fourteen application types with a `display()` of their own are not flagged, which no text scan can tell apart; and the workspace already denies `clippy::all`, so the boot test's clippy gate enforces it with no change to a shared file |

### The sweep

The call sites were found by clippy itself -- the new configuration, the lint
capped to a warning, every crate under `apps/` on both the host target and
`x86_64-unknown-linux-gnu` so `cfg(unix)` code was seen -- and each reported
span was rewritten in place; nothing was found by pattern.

**Then every site was read, because `display()` was not always a display.**
A shown name and a used name need opposite things: shown, a control
character must be escaped so it cannot break or disguise a line; used, the
name must stay exactly itself or it names another file. About a dozen of the
368 were used:

| Where | What the name was | Now |
|---|---|---|
| `musicplayer` M3U export | a line of the playlist file | the path exactly, or the track left out and named -- a line break or a name that is not text has no M3U form (it wrote U+FFFD, pointing the entry at no file) |
| `email` flag key | a persisted key | `pathcodec::encode_path` (§426): shown forms can coincide |
| `rssreader` feed address | read again as a path at start | the exact text; a name that is not text is kept by its shown form and the window says it will not be read again |
| `diskanalyzer` path box | edited, then scanned | unedited, it means the folder it was filled from, exactly |
| `installer/build.rs` | a linker argument | exact, or the build stops |
| save-name suggestions, an attachment's name from the user's own file, the two editors' document names | used as file names | `text_or_shown`: exact whenever the name is text |
| an attachment's name from a *stranger's* message | a suggested file name | escaped on purpose: a bidirectional override cannot hide the true extension |
| test arguments and a test's device node | opened | exact text |

`pathtext` gained `text_or_shown` for the used-but-not-a-key case, and ten
per-application helpers that each rendered names their own way (one wrote
`\xNN`) now call it or `shown`.

### The cost

`apps/clippy.toml` replaces the workspace's `clippy.toml` for every crate under
`apps/`: clippy reads the nearest one and merges nothing. So it repeats the
workspace's settings. `pathtext`'s test
`the_apps_clippy_config_repeats_the_workspaces` fails if one is missing, and the
workspace file says so at its top.

**Where it lives:** `apps/pathtext/src/lib.rs`, `apps/clippy.toml`, and every
`.shown()` under `apps/`.

**How to reverse:** delete the two `disallowed-methods` entries; `.shown()` and
`.display()` are interchangeable at every call site.
