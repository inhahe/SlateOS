## Five copies of two escapers, at three levels of correctness (lane C) — FIXED

Following the GRUB finding above, the same question — *can this value change
the meaning of the document it is written into?* — was put to every generator
in `apps/`. It found five near-copies of a JSON escaper and two of an XML one,
which had drifted apart:

| Copy | JSON escaper | Verdict |
|---|---|---|
| `apps/jsonviewer` | `"` `\` `\n` `\r` `\t` `\b` `\f`, `\u00XX` fallback | correct |
| `apps/kanban` | as above (fixed in an earlier sweep) | correct |
| `apps/snippets` | `\u00XX` fallback present | correct |
| `apps/diagram` | five cases only, **no fallback** | emits invalid JSON |
| `apps/reminders` | five cases only, via `str::replace` | emits invalid JSON **and** corrupts on read |

**`apps/reminders` was the serious one.** Its `unescape_json` was a chain of
`str::replace` calls in the wrong order — `\n` decoded before `\\`:

```rust
s.replace("\n", "\n").replace("\r", "\r").replace("\t", "\t")
 .replace("\\\"", "\"").replace("\\\\", "\\")
```

So the two-character text `\n` (a literal backslash, then the letter n) was
escaped to `\n` on save and read back as a **newline**. A Windows path in a
note, `C:\temp`, came back as `C:\<TAB>emp`. The damage was then re-saved, so
the note decayed a little further every time the app was opened. The existing
test `test_json_escape_special_chars` covered this function and passed,
because its sample text — `"Hello \"world\"\nnew line"` — contains a real
newline and real quotes but not one literal backslash, the single input that
tells a correct decoder from a broken one.

**`apps/whiteboard` had an unescaped XML export**: `page.name`, `layer.name`
and both `TextLabel` and `StickyNote` content went straight into the markup, so
a sticky note reading `</sticky><rect/>` closed its own element and injected a
sibling, and any `&` made the export unparseable. Same class as the GRUB bug,
found by the audit that bug prompted.

**Fixed** by adding `gui/toolkit/src/escape.rs` (`guitk::escape`) with one
correct implementation of each — `xml`, `json_string`, and a
`unescape_json_string` that is a single left-to-right pass and so structurally
cannot make the replace-chain mistake — and routing `reminders`, `whiteboard`,
`diagram`, `snippets` and `markdowneditor` through it. Non-vacuity verified by
breaking each of the five defences alone; each failed only its own tests.

**Not converged, deliberately:** `apps/kanban` and `apps/jsonviewer` decode
inside full tokenising JSON parsers (`parse_string(data, start) -> (String,
usize)`), a different shape from a standalone `unescape`. Both are already
correct, so rewriting them onto the shared helper would risk regressing working
code for no correctness gain. If a third parser of that shape appears, extract
a shared *parser* rather than bending these two into the wrong signature.

**The generalisation, now twice-confirmed:** a value can be preserved
byte-for-byte and still be a bug. The lossy-path sweep asked *is this
preserved?*; this one asks *can this re-punctuate its document?* Every
`format!` into a file that something else later parses needs an escaper chosen
for that grammar. Remaining unaudited generators of this kind: the YAML and
`.desktop`-style writers, if any, and `pkg/`'s manifest output.
