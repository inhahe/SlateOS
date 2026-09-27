# Lane E -> lane F: a file named on an application's command line may be any bytes

**Filed:** 2026-09-26 by lane E. **For:** lane F (`gui/window/src/app.rs`, `Args`).
**Status:** OPEN.

**In short:** an application that is opened on a file -- the file manager's
"open with", which runs `program /path/to/file` -- reads the path through
`oswindow::app::Args::from_env`. That calls `std::env::args()`, which
**panics** on an argument that is not UTF-8, and a SlateOS path may hold any
byte but `/` and NUL. Double-clicking a file whose name is not UTF-8 crashes
the program it opens before its window appears. `Args::rest` is also a
`Vec<String>`, so even without the panic there would be no way to carry such a
path through.

## Who it bites

Every application that takes a file. As of today that is `editor`,
`imageviewer`, `hexeditor`, `pdfviewer`, `archivemanager`, `explorer` and
`musicplayer` (lane E moved the last five onto `Args` today: they called
`app::launch`, which refused every file argument with "exit 2, unexpected
argument" before the window opened -- the file manager's "open with" opened
nothing in six of its eight targets). `app::launch` itself calls
`Args::from_env`, so an application that takes no file panics on such an
argument too, instead of saying it takes none.

`apps/match3/src/main.rs` has a comment saying "`Args::from_env` has already
refused an argument that is not UTF-8" -- it has not; `std::env::args()`
panics while iterating, before `parse` sees anything.

## What would do it

```rust
pub struct Args {
    /// The address given with `--display`, if any.
    pub display: Option<String>,
    /// Everything that was not a display option, in order -- file names,
    /// usually, as the bytes they are.
    pub rest: Vec<OsString>,
}

impl Args {
    /// Over `std::env::args_os()`. `--display`'s value must be text (an
    /// address), and one that is not is an `Err`, not a panic.
    pub fn from_env() -> Result<Self, String>;
    /// For tests: the same over any `OsString`s.
    pub fn parse_os<I: IntoIterator<Item = OsString>>(args: I) -> Result<Self, String>;
}
```

`leftover_complaint` would print the first leftover with `.display()` (or
`to_string_lossy()` -- it is a message, not a path to open).

Lane E would then take `rest` as `&[OsString]` in its seven callers: each
turns an argument into a `Path` and nothing else, so the change there is the
type in one function signature per application.

## If it is never done

A file whose name is not UTF-8 cannot be opened from the file manager: the
program it is sent to panics. Everything else works.
