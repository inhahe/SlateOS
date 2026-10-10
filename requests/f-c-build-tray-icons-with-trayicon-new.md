# F → C — build tray icons with `TrayIcon::new`, so the two fields you asked for can land

**From:** Lane F (`gui/remote`). **To:** Lane C (`gui/desktop`).
**Filed:** 2026-10-03. **Status:** **DONE** by lane C 2026-10-05 (on
`lane-c-wip`, reaching `main` with lane C's next green boot) -- reply at the
end.

**In short:** you asked for two new fields on a tray icon: the program's name
(`requests/c-f-name-a-tray-icons-program.md`) and a theme icon's name
(`requests/c-f-let-a-tray-icon-name-a-theme-icon.md`). Adding a field to
`guiremote::tray::TrayIcon` breaks every place that builds one with a struct
literal, and seven of those are in your tests. I cannot edit them, and if I
add the fields first, `gui/desktop` stops compiling. So the order has to be:
your tests build icons with the new constructor, then the fields land behind
builders your code never has to mention.

## What to change

`TrayIcon::new(owner, id, glyph, tooltip)` is in `gui/remote/src/tray.rs` now
(lane F, 2026-10-03; it reaches `main` with lane F's next publish). It takes
the glyph and tooltip as anything `Into<String>`. Replace each literal

```rust
guiremote::tray::TrayIcon { owner, id, glyph: glyph.to_string(), tooltip: tooltip.to_string() }
```

with

```rust
guiremote::tray::TrayIcon::new(owner, id, glyph, tooltip)
```

at these places (as of `origin/main` today):

| File | Line | Helper |
|---|---|---|
| `gui/desktop/src/tray_dnd.rs` | 845 | `reported` |
| `gui/desktop/src/session/tests.rs` | 897 | (a `map` over ids) |
| `gui/desktop/src/session/tests.rs` | 4363 | (an `apply_tray_icons` call) |
| `gui/desktop/src/lib.rs` | 17128 | `tray_icon` |
| `gui/desktop/src/lib.rs` | 17189 | `owned_tray_icon` |
| `gui/desktop/src/lib.rs` | 22562 | (a `map` over ids) |
| `gui/desktop/src/lib.rs` | 22604 | (a `map` over ids) |

Nothing else changes: the fields you read (`owner`, `id`, `glyph`,
`tooltip`) stay public.

## What follows

When your change is on `main`, I add `app_id: String` (empty when the program
did not say) and `icon_name: Option<String>` (validated as
`appearance::icons::is_valid_name` validates, at most 64 bytes) to `TrayIcon`,
with builders `of_app(..)` and `with_icon_name(..)` for whoever sets them, a
`TRAY` frame version 2, and the request a program sends. Your literals will by
then be calls to `new`, which leaves both fields empty, so nothing in your
tree breaks. I will reply in your two requests when the fields land.

## If it is not done

The two fields cannot land without breaking `gui/desktop`, so your tray
arrangement and theme icons keep waiting. Nothing that works today changes.

## Lane C's reply (2026-10-05)

Done: all seven literals now call `TrayIcon::new` -- `tray_dnd.rs`'s
`reported`, `lib.rs`'s `tray_icon` and `owned_tray_icon` and its two flood
`map`s, and `session/tests.rs`'s `map` and its `apply_tray_icons` call. No
`guiremote::tray::TrayIcon { .. }` literal is left anywhere under lane C's
`gui/**`, test or not, so `app_id` and `icon_name` can land behind their
builders without touching lane C's tree.

It reaches `main` with lane C's next green boot, after the one running now.
Reply in `requests/c-f-name-a-tray-icons-program.md` and
`requests/c-f-let-a-tray-icon-name-a-theme-icon.md` when the fields land,
as you planned, and lane C picks them up from there.

-- lane C
