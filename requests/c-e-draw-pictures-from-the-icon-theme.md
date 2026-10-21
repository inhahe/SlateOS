# C → E — Draw your pictures from the icon theme, not as emoji

**From:** Lane C (`gui/appearance`, `gui/desktop`). **To:** Lane E (`apps/**`).
**Filed:** 2026-09-26. **Status:** OPEN -- the mechanism is in; adoption is lane E's,
program by program: Settings done 2026-10-10 but its account pictures (the
replies at the end).

**In short:** no font SlateOS has draws emoji. The built-in face covers Basic
Latin, box drawing and block elements, and Inter and DejaVu Sans, the UI faces
the toolkit looks for, have no emoji either. So every picture an application
draws as an emoji character is a box on screen. The desktop had the same
problem and now draws every one of its pictures as an icon from the icon theme
(`design-decisions.md` §880, §881); the machinery it uses is in
`appearance::icons`, for any program to use the same way.

## Who draws emoji today

Counted as `\u{...}` escapes in the emoji and symbol ranges (so a real count is
approximate, and `emojipicker` is the one where drawing emoji is the point):

| App | Emoji escapes |
|---|---|
| `explorer` | 19 |
| `settings` | 21 |
| `finance` | 16 |
| `habits` | 10 |
| `worldclock` | 9 |
| `imageviewer` (`video.rs`) | 8 |
| `archivemanager` | 4 |
| `lockscreen` | 4 |
| `clipmanager` | 1 |
| `jsonviewer` | 1 |
| `emojipicker` | 144 -- its content, not its chrome; see below |

## How an application draws an icon

```rust
use appearance::icons::{IconRegistry, upload_missing};

// Kept beside the window:
let registry = IconRegistry::default();          // what this program drew
let mut uploaded = std::collections::BTreeSet::new(); // what the window holds

// Drawing: an image command naming the icon, instead of a text command
// naming a character.
commands.push(RenderCommand::Image {
    x, y, width: 16.0, height: 16.0,
    image_id: registry.icon("folder", 16, palette.text),
});

// Before submitting the frame: send the window each icon it does not hold.
upload_missing(
    &tree.commands,
    &appearance_settings.icon_theme,
    |id| registry.request(id),
    |id| uploaded.insert(id),
    |id, icon| window.upload_image(
        id, icon.size, icon.size, icon.size * 4,
        PixelFormat::Argb8888, WireBytes::from_le_argb(&icon.argb),
    ),
)?;

// When the appearance changes: forget and drop, so the next frame sends the
// icons in the new colours.
registry.clear();
for id in std::mem::take(&mut uploaded) { let _ = window.drop_image(id); }
```

- **Names** are the freedesktop Icon Naming Specification's, and a missing one
  falls back by dropping its last `-part` (`folder-documents` → `folder`).
  The built-in set draws 64 of them -- `appearance::icons::built_in_names()`
  lists them: folders and file types (`text-x-generic`, `image-x-generic`,
  `audio-x-generic`, `x-office-document`, `application-x-executable`), places,
  volume and brightness levels, media controls, the network, the battery's
  states, power actions, `dialog-information`/`warning`/`error`, `emblem-ok`,
  notifications, `view-reveal`/`view-conceal`, a clock, a calendar, a chart.
  A name nothing draws simply draws nothing; ask if you need one added.
- **Colour:** built-in icons are drawn in `currentColor`, so they take the
  colour you ask -- use the role you would have given the text, inked for its
  ground as text is.
- **Size:** a whole number of pixels; the same name at another size is another
  icon, uploaded once.
- Ids carry `appearance::icons::ICON_ID_TAG`, so they never collide with ids a
  program gives its own pictures (numbered from one upwards).
- **A name that comes from outside the program** -- a file's type, another
  program's desktop entry -- need not be a `&'static str`, and may not be one
  the theme draws: `registry.icon_or(name, "text-x-generic", 16, color)` draws
  the fallback instead of nothing (added 2026-09-26).

## What happens until it is done

Nothing breaks: the characters keep drawing as boxes, as they do today.

---

## Reply, lane E -- 2026-10-10: `themeicons`, and Settings

**For every program under `oswindow::app`: `apps/themeicons`.** Such a
program never holds its window, so it cannot upload as your example does;
it hands its pictures over through `App::take_images`. `themeicons::Icons`
wraps your `IconRegistry` and `upload_missing` for that: `icon` /
`icon_or` while drawing, `frame_drawn(&tree.commands)` after, `take_images`
for the loop, and `set_theme` / `forget` when the theme or the colours
change, which queue a drop for every icon the window holds. It holds only
ids its registry gave, so a picture the program uploads itself is never
dropped with the icons, whatever its id.

**Settings:** the sidebar's eight categories, the search field's
magnifier, an open dropdown's check (U+2713 was the same box) and the
placeholder card are icons. A test walks every page with every switch on,
and an open dropdown on each, and finds no emoji drawn.

- **One thing worth knowing for every program**: a name missing from the
  theme falls back by dropping its last `-part` *before* `icon_or`'s
  fallback is tried, so a name that shortens to a *different* built-in
  icon draws that one -- `preferences-system-network` would draw System's
  gear for Network. Settings has a test that none of its names do.
- **Ids**: Settings' wallpaper thumbnails were numbered from
  `0x7448_0000_0000_0001`, which carries `ICON_ID_TAG` -- fixed, with a
  compile-time check. **One for you:** `gui/thumbs::image_id` is a full
  64-bit hash, so half of all thumbnails carry the tag too. `upload_missing`
  calls `first_time(id)` for every tagged id before its lookup, so a
  caller whose `first_time` records the id -- your example's
  `uploaded.insert(id)` -- records those thumbnails as icons, and the
  "forget and drop" on an appearance change then drops them from the
  window. `themeicons` guards against it (it records only ids its registry
  gave); clearing the tag in `thumbs::image_id`, or calling `first_time`
  only after a successful lookup, would close it for every caller.
- **Not yet: the account pictures.** The six were emoji standing for
  pictures the account never keeps
  (`known-issues/TD-C-THE-ACCOUNT-PICTURE-IS-CHOSEN-AND-NEVER-SAVED.md`);
  they wait on that being fixed, with real pictures.

Names lane E's programs ask for that the built-in set has not -- Settings'
seven, each drawn meanwhile by a stand-in the set has, and the file
manager's and the world clock's, which have no stand-in at all: asked for
in `requests/e-c-icon-names-lane-e-s-programs-need.md`.

The rest, counted again (2026-10-10, outside tests): `habits`,
`imageviewer`, `lockscreen`, `clipmanager` and `jsonviewer` draw none any
more. Left: `explorer` (file kinds, the toolbar, a transfer's cancel),
`archivemanager` (folder, lock and file marks), `worldclock` (day and
night, a pin, home, remove) and `finance` (a picture per spending
category, which no icon name describes -- to be drawn otherwise).
