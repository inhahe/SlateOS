### [E] The feed reader forgets its subscriptions, folders and marks at exit -- 2026-09-25
**Status:** FIXED (lane E, 2026-09-26) -- `design-decisions.md` §1212.

**Fixed.** As the proper fix below says: the subscriptions and folders are kept
as OPML (`rssreader/subscriptions.opml` in the settings folder), and each
article's read and starred marks by its feed and link (`rssreader/marks.txt`),
written after every change. A feed read from a file is read from it again at
the next start, and its articles get their marks back. A kept file that
cannot be read whole is left alone and the window says why; closing over a
save that is failing asks first. Two things found on the way: an OPML list
imported twice doubled every feed in it, and an empty folder -- which is how
the reader's own export writes a folder with no feeds yet -- was dropped on
import.

**In short:** the feed reader keeps nothing between sessions. Feeds added by
address or by an OPML list, the folders they are filed in, the articles read
from feed files, and which articles are read or starred all go when the window
closes; the next start is empty. Only Export… (OPML) saves anything, and only
the subscription list.

**Where.** `apps/rssreader/src/main.rs`: `RssReaderApp` holds `feeds`,
`folders` and `articles` in memory and nothing reads or writes them at start
or exit (the crate does not depend on `settingsfile`).

**The proper fix** keeps two things, in two shapes: the subscriptions and
folders as an OPML file in the user's configuration directory (the format the
program already reads and writes, and one a person can take elsewhere), read
at start; and each article's read and starred marks keyed by the article's
link -- or title, where a feed gives none -- in a small settings document, so
reopening a feed file restores them. The articles themselves need no copy:
they come from files the user keeps, and `read_any_file` already merges a
re-read feed into the one it came from.

**Not urgent, and it does not get worse.** Nothing a user has is lost that
they did not just type or open this session. Not done with the pointer pass
because it is a feature with its own shape to settle (above), not a way of
reaching one the program already had.
