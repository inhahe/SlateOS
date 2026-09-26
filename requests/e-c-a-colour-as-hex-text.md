# Lane E -> lane C: a colour as `#RRGGBB` text, both ways, on `guitk::color::Color`

**Filed:** 2026-09-25 by lane E. **For:** lane C (`gui/toolkit/src/color.rs`).
**Status:** OPEN.

**In short:** three of lane E's files that keep a user's colours -- the
spreadsheet's workbook, the address book, the kanban boards -- each carry the
same two functions: a `Color` written as `#RRGGBB` (`#RRGGBBAA` when it is not
opaque), and read back. `Color` has `from_hex(u32)` and nothing for text. A
fourth copy is the next app that keeps a colour.

## What would do it

```rust
impl Color {
    /// `#RRGGBB`, or `#RRGGBBAA` when `a` is not 255 -- every bit, so it
    /// reads back as the same colour.
    pub fn hex_text(self) -> String;
    /// `#RRGGBB` or `#RRGGBBAA`, or `None` for anything else.
    pub fn from_hex_text(text: &str) -> Option<Self>;
}
```

Lane E's copies to replace once it lands, all identical: `colour_hex` and
`parse_colour` in `apps/spreadsheet/src/main.rs`, `apps/contacts/src/main.rs`
and `apps/kanban/src/main.rs`. Their tests (the files' round trips) cover the
behaviour either way.

## If it is never done

Nothing breaks; three copies stay three, and the next app that keeps a colour
makes four.
