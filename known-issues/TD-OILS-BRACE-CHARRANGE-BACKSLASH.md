### TD-OILS-BRACE-CHARRANGE-BACKSLASH. MSYS reference bash mishandles `{A..z}`-style char ranges crossing ASCII `\` (92); `osh` follows real (Linux) bash — INTENTIONAL, `osh` is correct 2026-07-20

**Where:** `userspace/oils/src/brace.rs` (single-char range expansion).

**What:** For a `{c..d}` range whose endpoints straddle the punctuation gap
between `Z` (90) and `a` (97) — which contains `[ \ ] ^ _ `` — `osh`
expands the **full inclusive ASCII range**, e.g. `echo {A..z}` →
`… X Y Z [ \ ] ^ _ ` a b …` and `echo {Z..^}` → `Z [ \ ] ^`. This matches
real Linux bash's documented sequence-expression semantics (each character
between the two endpoints, inclusive). The **MSYS reference bash on this
dev box is inconsistent/buggy** here: `echo {A..z}` expands but silently
**drops the `\`** (emits an empty slot at position 92), while
`echo {Z..^}` and `echo {[..]}` are left **unexpanded** (literal). Only the
MSYS build shows this; osh's output is the correct one.

**Why not "fixed":** matching the MSYS quirk would mean *removing* correct
behavior to reproduce a host-libc bug on invalid-ish input. Documented as a
reference-artifact divergence; osh keeps the correct inclusive char range.
