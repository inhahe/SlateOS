### TD-C-EVERY-SECTION-HEADING-IS-WRITTEN-OUT-BY-HAND — 2026-08-23 — OPEN

**In short.** A settings panel is divided into sections, and each section has a
heading — "Date Format", "Measurement", "Notifications". Every one of those
headings is eight lines of copied-out drawing code that differ only in the
words. There are fourteen copies across three files today, and there will be
one per section in every settings panel still to be converted. Nothing is
*wrong* on screen; the cost is that the rule "a section heading looks like
*this*" is recorded fourteen times instead of once, so the fifteenth copy is
free to disagree with the other fourteen and nothing will notice.

**Where:** `gui/desktop/src/language_settings.rs` (7 copies),
`datetime_settings.rs` (4), `notification_settings.rs` (3). The shape is
identical in all fourteen:

```rust
cmds.push(RenderCommand::Text {
    x,
    y: cy,
    text: "Date Format".into(),
    font_size: 15.0,
    color: p.lavender,
    font_weight: FontWeightHint::Bold,
    max_width: Some(width),
    overflow: TextOverflow::Ellipsis,
});
cy += 26.0;
```

**Proper fix:** one `fn section_heading(cmds, p, x, cy, width, text) -> f32`
returning the advanced `cy`, somewhere all three modules can reach — the
obvious home is beside `palette_check` in `gui/desktop/src`. The same argument
applies to the sub-heading rung (`p.subtext1` at 13pt Bold) and to the
label/value row, which is already a private helper duplicated per module.

**Why it is not done inside a per-module palette conversion:** the
reintroduction harness (`scripts/reintro-palette.py`) pins defects by *exact
source text*. Rewriting these sites collapses the very strings that twenty-odd
already-proved defects match on, in modules whose proofs are finished and
merged. Folding the extraction into one module's conversion would silently
invalidate earlier modules' evidence; doing it as its own change, across all
modules at once, lets the harness be re-pointed in one deliberate edit and
re-run to confirm the proofs still hold. That is a sequencing reason, not a
cost one.

**Trigger:** do it once the last settings panel is converted (part 2 of
`TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`), so the
helper is extracted against the final set of call sites rather than a moving
one.

**If never fixed:** nothing breaks, but the heading convention stays
unenforceable. The symptom to expect is drift — a panel written next month at
14pt, or in `p.text` instead of `p.lavender`, sitting beside one written today,
with no test able to state that they should match because there is no single
place where the rule lives.
