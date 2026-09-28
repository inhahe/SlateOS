# C -> E -- the terminal draws in the theme's terminal colours

**From:** Lane C (`gui/toolkit`, `gui/appearance`). **To:** Lane E (`apps/terminal`).
**Filed:** 2026-09-27. **Status:** DONE (lane E, 2026-09-28) --
`ColorScheme::from_palette` takes the foreground, background, cursor and the
sixteen from `palette.terminal`, and the selection from the accent, exactly as
asked; its comment says why the slots keep their meanings while the theme
picks the shades. tmux's panes are drawn by the same code and follow.

**In short:** the palette every application is handed (`App::theme_changed`,
design-decisions §822) now carries a terminal's colours: `Palette::terminal`,
a `guitk::palette::TerminalColors` of `foreground`, `background`, `cursor` and
`ansi: [Color; 16]` (design-decisions §1410). By default they are the theme's
own hues in the slots every terminal gives them -- red in red's, green in
green's, pink as magenta, teal as cyan -- and a theme's `terminal` section can
set any of them. The terminal application still draws its sixteen from fixed
xterm-like constants (`apps/terminal/src/main.rs`, `ColorScheme::default`).

## What is asked

`ColorScheme::from_palette` takes all four from the palette:

```rust
Self {
    foreground: palette.terminal.foreground,
    background: palette.terminal.background,
    cursor: palette.terminal.cursor,
    selection_bg: palette.accent,          // as now: every selection is the accent (839)
    ansi: palette.terminal.ansi,
}
```

## On the comment it replaces

`from_palette`'s comment says the sixteen are not furniture -- "colour 1 is red
because the escape sequence says so ... Retinting those would ... corrupt what
programs print". The *meaning* of each slot is kept: slot 1 is the theme's red,
slot 2 its green, as every terminal's themes do (Catppuccin, Solarized,
Gruvbox). What a theme chooses is the shade, which is the thing a theme is for.
The foreground is held to the text contrast floor on the background; the
sixteen are left as the theme states them, a program being entitled to mean
black on black.

## If this is never done

Nothing breaks: the terminal keeps its fixed colours and ignores the theme for
the sixteen, as today.
