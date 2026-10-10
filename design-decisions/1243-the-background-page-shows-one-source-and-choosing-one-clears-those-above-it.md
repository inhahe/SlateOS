## 1243. The Background page shows one source, and choosing one clears those above it

**Date:** 2026-10-10 · **Decided by:** Claude (operator-approved scope: lane
C's request `requests/c-e-a-themes-wallpapers-on-the-background-page.md`
asked for a theme as a wallpaper source and for the sources to be "one
choice", and left how to Lane E) · **Lane:** E

**In short:** The desktop's background can come from five places -- one
picture, a theme's pictures, pictures by time of day, a folder that rotates,
or nothing (the plain background) -- and when more than one is set, the
desktop shows only the highest of them in a fixed order. The Settings page
showed each as a section of its own, so a user could set a picture, see it
written, and never see it on the desktop, because a folder set months ago
outranked it. Now the page asks one question first -- "Show" -- and shows
only the controls of the answer. Choosing an answer clears the sources that
would outrank it, so what is chosen is what is shown; sources below it are
kept, unseen, as what shows if it is cleared.

### The order, and the rule

The desktop's order (`gui/desktop/src/session.rs`, `sync_wallpaper`), highest
first: **pictures by time of day**, then **a rotating folder**, then **a
theme's picture** for the mode the desktop is drawn in, then **a picture**,
then nothing.

| Chosen | Cleared | Kept, unseen |
|---|---|---|
| Pictures by time of day | -- | the folder, the theme, the picture |
| A rotating folder | the schedule | the theme, the picture |
| A theme's pictures | the schedule, the folder | the picture |
| A picture | the schedule, the folder, the theme | -- |
| Plain background | everything | -- |

The page's answer to "Show" is the highest source that is set -- the one the
desktop shows -- unless the user has just chosen one that is not set up yet
(a folder not yet picked): then that one, with its controls, until it is set
up or another is chosen. Nothing is cleared until the choice is made, so
looking at a source costs nothing.

### Why kept below rather than cleared everywhere

A theme whose pictures cannot be used -- uninstalled, say -- shows the user's
own picture in their place (lane C's `WallpaperTheme::problem` says so). If
choosing a theme cleared the picture, that fallback would be the plain
background, and choosing "A picture" again would find it gone. Keeping what
is below means each source remembers its settings while another is shown.

### Pictures, shown as pictures

A theme is chosen from its recommended picture for the mode the desktop is
in now, drawn small with the theme's name -- the request's "each shown by its
recommended picture" -- and under "A picture", the pictures installed themes
bundle (`ThemeInfo::wallpapers`) are offered the same way, beside Choose...
Each is decoded on a thread of the page's own (`apps/settings/src/thumbs.rs`),
scaled as it is decoded (`imagecodec::decode_scaled`), and drawn when it is
ready: a full-screen photograph takes long enough to decode that the page
would stall on opening while it did.

### Alternatives

| Alternative | Why not |
|---|---|
| Choosing a source clears every other one | A theme that cannot be used would leave the plain background rather than the user's picture, and switching back would find the picture gone. |
| Keep every section, and say under each whether it is the one shown | Four interacting sources and a sentence each; the user still has to work out the order to get what they want. |
| A `wallpaper.source` field in `appearance.yaml` that the desktop obeys | The same answer with a second copy of it to disagree with the first, and a change to the desktop and the file format, lane C's, for no difference a user would see. |

**Reversal:** `WallpaperSource` and `SettingsState::choose_wallpaper_source`
in `apps/settings/src/main.rs`.
