## 1212. The feed reader keeps its subscriptions as OPML and its marks by link, and reads a feed file again at start

**Date:** 2026-09-26
**Lane:** E
**Decided by:** Claude (autonomous) -- Claude's to revisit

**In short:** The feed reader kept nothing: every start was empty. Now the
list of feeds and the folders they are filed in are written after every
change, as an OPML file in the settings folder -- the format the reader
already imports and exports, so the file is also one a person can take to
another reader. Which articles are read or starred is written beside it,
keyed by each article's link. The articles themselves are not copied: they
come from feed files the user keeps, and a feed read from a file is simply
read from that file again at the next start, its marks coming back with it.

### What is kept, and how

| | Kept as | Why this shape |
|---|---|---|
| Feeds and folders | OPML (`subscriptions.opml`) | the program's own import and export format; portable |
| Read and starred marks | lines of `flags feed key` (`marks.txt`) | OPML has nowhere for them; a key per article, not an index, survives a feed file that changed |
| Articles | not copied | they are in the user's own files, which are read again |

### Smaller calls

- **An article's key is its link, or `title:` and its title** for a feed that
  gives no link. The link is what a feed promises to keep stable; a title can
  be edited, but a feed without links offers nothing better.
- **Only marked articles are written**, and only for subscribed feeds: a
  removed feed takes its marks with it, so the file does not grow with feeds
  that are gone.
- **Only a feed whose address is a file on this machine is read at start** --
  one added by a web address cannot be fetched, as the window says.
- **Reading at start is not a change**: nothing is written until the user
  changes something, re-reading notwithstanding.
- **An OPML list imported twice adds only what is new**, and an empty folder
  is a folder -- both found by keeping: the reader's own export writes an
  empty folder, and its own import dropped it.

**Where it lives:** `apps/rssreader/src/main.rs`: `subscriptions_path`,
`marks_path`, `Marks`, `mark_key`, `marks_text`, `parse_marks`,
`RssReaderApp::{from_settings, load_kept, keep, unkept, request_close, answer,
remember_marks, subscriptions_changed, route_event}`.

**How to reverse:** the formats are `generate_opml`/`import_opml` and
`marks_text`/`parse_marks`; `from_settings` is what `main` opens.
