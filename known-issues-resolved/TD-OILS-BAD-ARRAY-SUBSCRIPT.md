### TD-OILS-BAD-ARRAY-SUBSCRIPT. `osh` "bad array subscript" underflow name/fatality — 2026-07-20 — ✅ RESOLVED 2026-07-20

**Status:** All four forms now match bash exactly, including the exact
diagnostic text (which uses the **raw subscript source**, post word-expansion
but pre-arithmetic — `a[1+2-20]`, `a[i]` — not the evaluated index):

- **Write underflow** (`x[-9]=z`, `a[1+2-20]=z`, `a[i]=z`): names the **full**
  reference with the raw source (`a[1+2-20]: bad array subscript`) and is
  **fatal** in command position (aborts, status 1). Inside `declare "…"` it is
  demoted to **non-fatal** (status 1, `declare` continues). Implemented at
  `apply_assignment` (~3521): the subscript is evaluated with the arith tag
  cleared (bash never tags a bad *subscript* expr, even under `-i` — only a bad
  `-i` *value* is tagged `declare:`), a syntax error returns early, and an
  underflow emits `{name}[{raw_src}]` + sets `unbound_error = Some(1)`. The
  `declare` branch passes the raw subscript source straight through (no
  pre-evaluation), so the message names the source and the value keeps its
  `declare:` tag; a `had_fatal` snapshot demotes the underflow to status 1.
- **Read underflow, value form** (`echo ${a[-5]}`): non-fatal, names the
  **base** (`a: bad array subscript`), value expands empty — matches bash.
- **Read underflow, length form** (`echo ${#a[-9]}`, `${#a[1+2-20]}`): the
  obscure **fatal** subcase naming the raw source + `]`
  (`1+2-20]: bad array subscript`), aborting the command. Implemented at
  `expand_array_ref` (~4590).
- **Positive out-of-range read** (`${a[9]}`): empty, no error — matches bash.

Covered by `bad_array_subscript_underflow` and the updated `array_negative_index`.
