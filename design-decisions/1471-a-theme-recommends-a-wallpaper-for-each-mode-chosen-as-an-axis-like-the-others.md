## 1471. A theme recommends a wallpaper for each mode, chosen as an axis like the others

**Date:** 2026-10-05 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** a theme can now come with its own desktop pictures. Its author
puts them in a `wallpapers` folder beside `theme.yaml`, and names one for
dark mode and one for light in a `wallpapers` section. Choosing that theme
for the wallpaper -- `theme.wallpaper: <name>` in `appearance.yaml`, as each
other part of a theme is chosen -- shows its picture for the mode the
desktop is in: a day picture in light mode and a night one in dark, changing
with the automatic mode. A time-of-day schedule or a rotating folder, when
the user has set one, still wins; the theme's picture takes the place of the
single picture. The theme checker checks the pictures.

**Where:** `gui/appearance/src/themes/wallpaper.rs` (`WallpaperTheme`,
`WallpaperNames`, `bundled`), `gui/appearance/src/themes.rs`
(`ThemeFile::wallpapers`, `ThemeInfo::wallpapers`, `provides_wallpapers`),
`gui/appearance/src/lib.rs` (`AppearanceSettings::wallpaper_theme`,
`theme_wallpaper`), `gui/desktop/src/session.rs` (`sync_wallpaper`),
`gui/appearance/src/themecheck.rs` (the `wallpapers` axis); asked for by
`roadmap-detailed.md` (*Tier 3 -- Wallpaper Integration*).

### The choices, and what they cost

| Choice | Instead of | For | Against |
|---|---|---|---|
| **An axis, `theme.wallpaper`** | a switch, "use the colour theme's wallpaper" | The model every other part of a theme has: one theme's colours can be worn with another's pictures, and a list of themes that give wallpapers is made as the list of those that give icons is (`ThemeInfo::provides_wallpapers`). | One more key in the settings file. |
| **A picture for each mode -- the other mode's when only one is named** | one picture; or a list rotated | A day and a night picture are what a theme pairs with its colours, and the automatic mode already changes at its edges, so the picture follows with nothing more. Rotation is the folder setting's job. A picture is not made for one mode as colours are, so one named for dark is shown in light too rather than nothing. | A theme recommends at most two of the pictures it bundles; the rest are offered by the list, as pictures to choose. |
| **Below a schedule and a rotation folder, above the fixed picture** | above everything | A schedule and a rotation are choices the user goes on making; the theme's picture stands in for the one picture. | A user with a folder set and a wallpaper theme chosen sees the folder: the Settings page should make where the wallpaper comes from one choice. |
| **Inside the theme's folder only** | any path | A theme cannot reach outside its folder, as for its screenshots and icons. | A theme cannot recommend a picture the system ships elsewhere. |
| **The built-in theme recommends none** | an Aero picture | The tree has no Aero picture to recommend; choosing the built-in theme for the axis is choosing your own picture. | -- |

A recommended picture that is not in the theme's folder is dropped with a
warning; a theme left recommending none says so in its `problem()`, and the
user's own picture is shown. The checker decodes every picture the folder
bundles and each recommended one as the desktop does, up to 64 MiB (a limit
of its own: the desktop has none), and the `wallpapers` axis is no longer
one it calls planned.

The settings watcher (`appearance::watcher`) counts which recommended
pictures are there as well as the theme's file, so a picture copied in after
the file is found the next time the settings are re-read -- after a save in
Settings, or at the automatic mode's edge -- and not only once the file
itself changes. A watcher looks only when told to
(`TD-C-AN-EDITED-THEME-FILE-IS-NOT-NOTICED-UNTIL-THE-SETTINGS-CHANGE`), and a
picture replaced under the same name is not read again, a theme's or the
user's own (`TD-C-A-WALLPAPER-REPLACED-UNDER-THE-SAME-NAME-IS-NOT-READ-AGAIN`).

**What is not done.** The Settings page that chooses it -- the themes that
give wallpapers, and the pictures each bundles -- is lane E's
(`requests/c-e-a-themes-wallpapers-on-the-background-page.md`). And, as for
every axis but the colours, the desktop does not say on screen that a chosen
wallpaper theme cannot be used; the page can show `problem()`.
