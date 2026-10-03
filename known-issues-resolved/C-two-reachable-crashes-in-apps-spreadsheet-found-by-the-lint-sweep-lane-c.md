## Two reachable crashes in `apps/spreadsheet`, found by the lint sweep (lane C)

**Status: FIXED 2026-08-16** (lane C). Both verified by reverting the fix and
watching the new test fail with the exact failure described, then restoring.
Recorded because the *shapes* recur, not because these two are still open.

### 1. Any formula with ~50,000 nested parentheses aborts the process

`FormulaEvaluator` counted recursion depth in exactly one place —
`resolve_cell`, which is followed when a formula names another cell — and
checked it against 100. Nothing at all counted the grammar's own recursion:
`parse_primary`'s `LeftParen` arm calls `parse_comparison`, and so does every
function argument, so `=((((((…1…))))))` recursed once per parenthesis until the
stack ran out.

Reproduction (this is the regression test, with the guard removed):

```rust
let formula = format!("={}1{}", "(".repeat(50_000), ")".repeat(50_000));
evaluate_formula(&formula, &sheet);
```

```
thread 'tests::a_deeply_nested_formula_reports_an_error_instead_of_overflowing' has overflowed its stack
process didn't exit successfully: … (exit code: 0xc00000fd, STATUS_STACK_OVERFLOW)
```

`0xc00000fd` is the same code `rssreader`'s XML nesting produced, and the same
non-recovery: it is not a `Result` the caller can render in a cell, it takes the
whole application — and the unsaved workbook — with it.

**The fix is where the counting happens, not how much of it there is.** Every
recursive step in the evaluator re-enters the grammar at `parse_comparison`, so
that is now the single place depth is charged, and the budget is *shared*
between parenthesis nesting and chains of referring cells because it stands for
one thing: the depth of the Rust stack, which does not care which production put
a frame there. `resolve_cell` no longer counts separately; it seeds the
sub-evaluator with the depth already spent.

Four tests pin it: 50,000 parens → `#DEPTH!`, 20,000 nested `ABS(` → `#DEPTH!`,
20 parens still evaluates (a guard that fires on real formulas is a guard that
gets raised until it is useless), and a 999-cell reference chain reports
`#DEPTH!` while a genuine two-cell cycle still reports `#CIRC!`.

### 2. Typing a non-ASCII character into a cell, then Delete, aborts the process

`InteractionMode::Editing { text: String, cursor_pos: usize }` — and the code
around it did not agree what `cursor_pos` counted. Insertion converted it with
`text.char_indices().nth(*cursor_pos)`, which is **characters**. `Backspace`,
`Delete`, `Right` and `End` used it directly as a byte offset into the same
string. For ASCII those are the same number, which is why it worked at all.

`String::remove` takes a byte offset and panics if it is not on a character
boundary. So:

```
type "é", type "a", press Home, press Right, press Delete
```

```
panicked at apps\spreadsheet\src\main.rs:2164:35:
start byte index 1 is not a char boundary; it is inside 'é' (bytes 0..2) of `éa`
```

Backspace reaches the same panic from the other end (`End`, `Backspace`,
`Backspace`). The two entry points into editing did not even agree with each
other: `begin_editing` seeded the caret with `text.len()` — bytes — while typing
a character from `Normal` mode seeded it with `1` — characters.

**Fixed by giving the caret a type.** `mod edit_buffer` holds
`EditBuffer { text, caret }` with the caret documented and enforced as a
character count in `0..=chars().count()`; `byte_of` is the one function that
converts, so it is the one place the conversion can be got wrong. The enum
variant is now `Editing { buffer: EditBuffer }`, which is why no caller can
reintroduce the ambiguity. Five tests cover it, including the two exact
keystroke sequences above.

This is the third crate in the sweep where a "character position" was a byte
offset (`rssreader`'s `wrap_text`, its content-window measurement, and now
this). It is the single most common defect the sweep has found, and it is
invisible to every test written in English.
