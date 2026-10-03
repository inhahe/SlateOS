## 541. Arrow keys move the caret by what is on the screen, not by position in the sentence

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Operator (Claude recommended this option) — `open-questions.md` → C-Q2, answered `b` (visual)

**In short:** Hebrew and Arabic are written right to left, and one line can mix them with English — *"I said שלום to him"*. On such a line the order the characters are stored in is not the order they are drawn in, so the Right arrow key has two possible meanings that disagree. It now means **one step to the right on the screen**. Previously it meant "the next character in reading order", which made the caret occasionally jump a whole word sideways between two presses of the same key. The cost of the new rule is that the caret's *position in the sentence* can move backwards while it moves rightwards on screen.

### The worked example

The line is `I said <SHALOM> to him`, where `<SHALOM>` is one Hebrew word of five letters, drawn right-to-left inside a sentence drawn left-to-right. Put the caret just before the Hebrew word and press Right five times:

| | What the caret does | Where it ends up |
|---|---|---|
| **Logical** (the old rule) | Steps through the letters in reading order, so it jumps to the word's right-hand end, walks leftwards, then jumps back | after the last Hebrew letter, at the word's **left** edge |
| **Visual** (the new rule) | Moves right by one letter each press, never jumping | at the word's **right** edge, having passed through the whole word |

Both arrive "after the whole word" in some sense. They disagree about which end of the word that is, and about everything in between.

### Why visual

Neither convention is wrong, and the major systems are split — macOS, GTK and Qt are logical; Windows edit controls are visual, and ICU's `ubidi` API is built to support the visual walk.

The argument that decided it is what the key is *called*. A user presses "right arrow" while looking at the screen. A right arrow that sometimes moves the caret leftwards is surprising in a way no correctness argument repairs. The logical convention's advantage — that the caret's text offset advances monotonically — is real but **invisible**; the visual convention's advantage is the thing the user is actually looking at.

Home/End and word-motion stay logical under this decision, and that is not an inconsistency: those name *positions in the sentence* ("start of line", "next word"), not *directions on the screen*. Only the two keys that name a screen direction follow the screen.

### The measured caveat, which is the reason this was asked rather than guessed

The caret has to carry one extra bit alongside its offset: **which side of a direction boundary it is on.** That turned out not to be a nicety. A text box that stores only the caret's position in the string between keypresses, and recomputes the rest each time, does not merely land on the wrong side of a boundary — it **skips the entire right-to-left word in a single press**, which is worse than the behaviour being replaced.

So a half-implemented "visual" — switching the arrow keys without also making each widget remember that bit — is a regression, not a partial improvement. This is the specific thing to check when wiring each widget, and it is why "one line per widget" is true only because the groundwork was already built.

### What was already built

`caret_left` / `caret_right` were written and tested on 2026-08-17, before the answer arrived, covering a mixed-direction line, an Arabic ligature crossed as a single unit, and the pixel round-trip. They were written with nothing calling them precisely because the answer was outstanding — and they are not wasted under either answer, since mouse selection and any future screen-order feature need the same primitive. That is why the question could be left open at no cost.

### Rejected

- **A — keep logical.** The status quo, self-consistent, and what most of Linux does. Rejected because its one advantage is not observable by the person pressing the key, while its cost — a caret that jumps a word's width sideways between two presses of the same key — is.
- **C — a user setting, defaulting to one of the two.** Rejected on two grounds: it asks the user a question they have no basis to answer, and it doubles the number of behaviours every future text widget must be correct in — for a rule that is subtle enough that the *single* correct behaviour already has a documented way to get it wrong.

### Where this lands

- `gui/font/src/shape.rs` — `ShapedRun::caret_left` / `caret_right`, the primitive; already correct.
- `gui/toolkit/src/text.rs` — `TextCursor` and its wrappers.
- `guitk::widget::TextInput` and `guitk::modal::InputDialog` — the arrow-key handling; each carries a comment marked `C-Q2` naming the exact line.
- `apps/editor` is **not** covered by this decision either way. It draws its caret and scrolls horizontally on the assumption that screen order equals reading order, so it needs its own larger fix first (`known-issues.md` → `TD-EDITOR-IS-NOT-BIDIRECTIONAL`).
