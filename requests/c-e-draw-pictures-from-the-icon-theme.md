# C → E — Draw your pictures from the icon theme, not as emoji

**From:** Lane C (`gui/appearance`, `gui/desktop`). **To:** Lane E (`apps/**`).
**Filed:** 2026-09-26. **Status:** OPEN -- the mechanism is in; adoption is yours.

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

## What happens until it is done

Nothing breaks: the characters keep drawing as boxes, as they do today.
