# E → C — icon names lane E's programs need that the built-in set does not draw

**From:** lane E (`apps/**`). **To:** lane C (`gui/appearance`'s built-in icons).
**Filed:** 2026-10-10. **Status:** OPEN.
**About:** `requests/c-e-draw-pictures-from-the-icon-theme.md` (lane E's reply).

**In short:** lane E's programs are moving their pictures from emoji, which
no font here draws, to icons from the theme (`apps/themeicons`). Settings
has moved. Some names they need are not in the built-in set. For Settings'
seven a stand-in the set has is drawn meanwhile -- right, just not the best
picture. For the rest there is no stand-in at all: the file manager's Cut
and Paste buttons and a transfer's cancel button would be blank squares, and
the world clock's night mark would draw the *sun*, since
`weather-clear-night` shortens to the built-in `weather-clear`. Those
programs keep their emoji until these exist.

## No stand-in -- the programs wait on these

| Name | Where | What it shows |
|---|---|---|
| `edit-cut` | file manager toolbar | scissors |
| `edit-copy` | file manager toolbar, when Copy gets its button | two sheets |
| `edit-paste` | file manager toolbar | a clipboard |
| `folder-new` | file manager toolbar ("New Folder") | a folder with a plus; it shortens to `folder` today, which loses the plus |
| `window-close` | a transfer's cancel, a world clock's remove | a cross |
| `weather-clear-night` | the world clock's night mark | a moon |
| `view-pin` | the world clock's pinned city | a pin |

## A stand-in is drawn meanwhile -- the picture would be better

| Name asked for | Where | Drawn meanwhile |
|---|---|---|
| `network-workgroup` | Settings: the Network category (GNOME's Network panel icon) | `network-idle` |
| `preferences-desktop-theme` | Settings: Personalization | `image-x-generic` |
| `applications-other` | Settings: Apps | `application-x-executable` |
| `system-users` | Settings: Accounts | `avatar-default` |
| `security-high` | Settings: Privacy & Security (a shield) | `system-lock-screen` |
| `system-software-update` | Settings: Update & Recovery | `system-reboot` |
| `object-select` | Settings: the check beside an open list's chosen item | `emblem-ok` (a check in a ring) |
| `video-x-generic` | file manager: a video's kind | `media-playback-start` |
| `package-x-generic` | file manager: an archive's kind | `application-x-executable` |
| `text-x-script` | file manager: source code's kind | `text-x-generic` |

**One constraint**, which each program tests for its own names: a name must
not shorten (dropping a last `-part`) to a *different* built-in icon before
the program's stand-in is tried. `weather-clear-night` is the live case:
until it exists, it draws `weather-clear`'s sun.

## When it is done

A notice to lane E; the file manager's toolbar and transfers and the world
clock move to icons that day.

## If it is never done

Settings keeps its stand-ins; the file manager's toolbar and the world
clock keep their emoji, drawn as boxes.
