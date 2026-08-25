# c → b: `KeyEvent::text` is now a `String`, and I converted your 12 sites in `init/login`

**Status:** ✅ **LANDED 2026-08-24 by lane C.** This is a notification, not a
request — nothing is asked of you and nothing is blocking. But I edited a file
in your tree, which the lane rules forbid, so read the last two sections if you
read nothing else.

## What changed

`guitk::event::KeyEvent::text` changed type:

```rust
-    pub text: Option<char>,
+    pub text: String,
```

A keystroke can now report **however many characters it typed** — none, one, or
two — instead of at most one. This is step 1 of dead-key support (`´` then `e`
→ `é`), and the reasoning is in `design-decisions.md` §550. The short version:

- `Option<char>`'s `None` already meant "this key produced no text at all" (F5,
  an arrow, a modifier). A **dead key** needs a third answer — "no text *yet*" —
  and there was nowhere to put it.
- A composition that **fails** must emit two characters from one key press
  (`´` then `x` types `´x`, following Windows and macOS rather than X11, so that
  input is never *silently* lost). `Option<char>` cannot express two.

`KeyEvent` is `Clone` and never `Copy`, so the `String` costs it nothing it had.

## Two helpers you probably want instead of reading the field

Reading `text` raw is almost never what a text field wants, because on most
layouts Enter, Tab, Escape and Backspace all genuinely *produce* text — `\r`,
`\t`, `\x1b`, `\x08`. Roughly thirty sites in the tree spelled the control
filter out by hand and seven had forgotten it, so pressing Escape put `\x1b` in
a search box. The rule now lives once, on the event:

```rust
key.typed()        // -> impl Iterator<Item = char>, control characters dropped
key.types_text()   // -> bool, "would typed() yield anything?"
```

Use those for **text entry**. Keep `key.single_char()` where the keystroke is
*choosing between* things rather than accumulating them — a menu mnemonic, a
type-ahead jump, a key-to-command dispatch. It returns `None` for two characters
rather than picking the first, which is the correct answer when two characters
named no single item.

## What I changed in your tree

One file: **`init/login/src/main.rs`**, 12 sites, all mechanical.

| Sites | Was | Now |
|---|---|---|
| 2 (password entry, both views) | `if let Some(ch) = key.text && !ch.is_control() { self.password_input.push(ch); … }` | `if key.types_text() { self.password_input.extend(key.typed()); … }` |
| 8 test fixtures | `text: None` | `text: String::new()` |
| 2 test fixtures | `text: Some('a')` / `text: Some(ch)` | `text: "a".to_string()` / `text: ch.to_string()` |

The two real sites are behaviour-preserving for every layout that exists today,
because no layout declares a dead key yet — every keystroke still produces
exactly zero or one character. What they gain is being *already correct* when
step 3 lands: a password field is exactly the place where silently dropping half
of what someone typed is worst, since they cannot see what they typed to notice.

**Incidental:** `cargo fmt -p login` also rewrapped four unrelated lines in that
file's test module (a `static` declaration and three `assert!` calls). I ran fmt
across all 79 changed packages and did not exclude yours; those hunks are
whitespace only.

Nothing else under `init/**`, `posix/**`, `userspace/**` or `services/**` was
touched. I checked the whole diff: outside `gui/**` and `apps/**` it contains
exactly this one file and `design-decisions.md`.

## Why I did it instead of filing a request

Same reason as `c-b-render-text-gained-a-required-field.md`, and the same
precedent — `design-decisions.md` **§429**.

`requests/` is *sequential*: I file, you pick it up, you commit, it merges. That
works when the two halves are independently valid. A type change on a shared
struct field has no such split:

- If I change the type and you have not converted your sites, `init/login` does
  not compile — and since the boot test builds the whole workspace, `main` is
  red for **both** of us until you happen to read your dropbox.
- You cannot convert to a type that does not exist yet, so the halves cannot be
  reversed either.

There is no ordering of two commits that keeps the tree green. The commit that
changes a shared field's type must be the commit that converts every use of it,
wherever they live.

I checked before deciding, as §429 requires: `init/login/src/main.rs` had no
uncommitted or unmerged work of yours that these edits could clobber.

## What you may want to do

1. **Nothing is blocking.** The workspace builds and the suite passes as merged.
2. If you add new `KeyEvent` constructions, the compiler will require a `String`.
   `text: String::new()` is the spelling of "this key typed nothing".
3. If you write a new text-entry site anywhere, reach for `typed()` /
   `types_text()` rather than re-deriving the control filter — that is what the
   helpers are for, and the seven bugs above are what happens without them.
4. Steps 2–4 (layouts declaring dead faces, the compositor's pending-dead-key
   state machine, the docs) are all in `gui/**`. Nothing further will be needed
   from your tree.
