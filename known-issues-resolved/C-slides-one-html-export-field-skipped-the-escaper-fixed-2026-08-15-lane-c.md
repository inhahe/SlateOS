## slides: one HTML export field skipped the escaper (fixed 2026-08-15, lane C)

`apps/slides`'s `export_html` escapes the presentation title, text-box bodies
and bullet items, and does so correctly. It did not escape the placeholder
label of an `Image` element, which is user-typed and is written straight into
the exported document. A label of `<script>…</script>` — or, more cheaply, a
`"` closing the `style` attribute early — is therefore reproduced as markup by
any browser opening the export.

### Why this one and not the other three

The three fields that were escaped each sit in a statement of their own:

```rust
push_html_escaped(&mut html, &slide.title);
```

The one that was not was a `{}` inside a larger `format!`, in the company of
five geometry values that genuinely cannot need escaping:

```rust
html.push_str(&format!(
    "  <div class=\"img-placeholder\" style=\"left:{x}px;top:{y}px;\
     width:{width}px;height:{height}px;\">{placeholder_label}</div>\n",
));
```

Reading that line, the eye is doing arithmetic, not taxonomy. Every other name
in the interpolation is an `f32`, and `placeholder_label` inherits their
apparent harmlessness by proximity. This is the recurring shape of the whole
audit: the dangerous interpolation is rarely the one on a line by itself — it
is the one *embedded among values that are obviously safe*, where the reader's
attention has already been spent. A grep for `format!` finds it; a reading of
the function does not.

The fix splits the statement so the label goes through `push_html_escaped` like
its three siblings, which also makes the asymmetry impossible to reintroduce
without deleting a call.

### Test

`no_text_field_can_inject_a_tag_into_the_export` drives *one* payload through
all four text fields at once — title, text box, image label, bullet item — and
counts tags rather than substring-matching, since escaped output legitimately
contains the payload text. Driving every field from a single payload is what
makes the test grow with the exporter: a fifth text field added later either
routes through the escaper or fails this test. A second test checks the
attribute case specifically, since a bare `"` escapes the `style` value without
needing a `<` at all.
