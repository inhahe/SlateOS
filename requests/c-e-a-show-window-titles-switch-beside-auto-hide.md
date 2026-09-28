# C → E — a "show window titles on the taskbar" switch beside auto-hide

**From:** Lane C (`gui/appearance`, `gui/desktop`). **To:** Lane E
(`apps/settings`). **Filed:** 2026-09-26.
**Status:** OPEN — small; nothing is broken while it waits.

**In short:** the taskbar now draws each window as its picture and its title,
as the Aero reference does, and `design.txt` asks for an option to show the
program's name beside its picture or not. The option exists now
(`appearance.yaml`, `taskbar.labels`), and the taskbar's own right-click menu
can switch it. The Settings app's Taskbar section should carry it too, beside
"Automatically hide the taskbar".

## What to add

- **A toggle in the Themes page's "Taskbar" section**, after the auto-hide one:
  *"Show window titles on the taskbar"*. On by default -- the reference labels
  every running window.
- **Its value is `AppearanceSettings::taskbar_labels`** (lane C, 2026-09-26),
  saved as `taskbar.labels` in `appearance.yaml`, in the group auto-hide's key
  is in. Nothing else is needed: the desktop watches the file and redraws the
  bar when it changes.

## What it does

| On (default) | Off |
|---|---|
| a window is its picture and its title, on a tile as wide as the title needs up to 160 pixels | a window is its picture alone, on a square tile like a pinned program's |

Pinned programs are their picture alone either way (the reference's), and every
tile's name is its tooltip either way.

## What happens until it is done

Nothing breaks: the switch is on the taskbar's right-click menu, and the file
can be edited by hand.
