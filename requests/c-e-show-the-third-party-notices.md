# C -> E: show the third-party notices, on a page you open

**From:** Lane C. **To:** Lane E (`apps/settings`). **Filed:** 2026-09-28.
**Status:** OPEN.
**Decision behind it:** `design-decisions.md` §1433; where the page lives is
§815.

**In short:** The system has to be able to show the licence notices of the
code other people wrote. When lane D's image recipe runs lane C's gatherer,
those notices will be in `/usr/share/licenses`. A new crate, `gui/notices`,
reads them. What is missing is a page that shows them. The shell's old About
dialog (`gui/desktop/src/about.rs`) has a Licenses tab, but nothing opens
that dialog. It is a screen you open, and §815 puts those in `apps/settings`.
So this asks for an **About page in Settings**, with the notices on it.

## The reader

```rust
let notices = notices::load(Path::new(notices::SYSTEM_DIR))?;
for n in &notices {
    n.title();          // "libjpeg-turbo 3.1.1"
    &n.licence;         // "IJG AND BSD-3-Clause AND Zlib"
    &n.attribution;     // Some("This software is based in part on the work of the Independent JPEG Group.")
    for t in &n.texts {
        t.name;         // "libjpeg-turbo-LICENSE.md"
        t.read()?;      // the text, as bytes
    }
}
```

`gui/notices/src/lib.rs` documents it. `gui/notices/tests/fixtures/bundle` is
a real, small bundle to test against: three notices, one of each kind.

## What the page must get right

- **`attribution`, when present, is shown as its own line, word for word.**
  It is a sentence a licence requires to be shown. Libjpeg-turbo's is the
  first. Folding it into the text, where it would only be seen by someone who
  opened that one notice, would not meet it.
- **`NoticesError::NotInstalled` is its own message.** Say "The licence notices
  are not installed on this system" rather than showing an empty list. An
  empty list would read as "this system carries no third-party code", which is
  false. Every development run will hit this until lane D's recipe is in
  (`requests/c-d-put-the-third-party-notices-in-the-image.md`).
- **Texts are bytes.** They are other people's files, shown as they are. A
  byte that is not UTF-8 must be shown visibly, for example escaped, rather
  than dropped or replaced (the project's rule against `from_utf8_lossy`).
  All of today's texts are ASCII or UTF-8.
- **Read a text when it is opened**, not all of them up front. Forty
  components' licences are not needed to draw a list.

## The rest of the old dialog

`about.rs` also has Overview, Hardware and Software tabs, built on a
`SystemInfo` struct. Whether the new page takes those is yours to decide. The
code is there to port, and `apps/sysinfo` may already cover what they show.
**Once the page exists, lane C deletes `gui/desktop/src/about.rs`.** It is on
the orphan-module baseline, and §815 says the shell's copy goes once the
screen moves. Tell lane C when the page lands.
