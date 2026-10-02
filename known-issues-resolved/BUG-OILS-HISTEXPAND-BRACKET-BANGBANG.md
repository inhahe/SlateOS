### BUG-OILS-HISTEXPAND-BRACKET-BANGBANG. the glob-bracket inhibition swallowed `[!!]`, which bash expands — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/histexpand.rs` — `inhibited()`.

**What.** A `!` directly after a `[` that has a later `]` is a negated glob
bracket, and history expansion stands aside for it. That rule has one exception,
found while probing the `%` designator: bash checks the character *after* the `!`
against `history_expansion_char` first, so a *doubled* `!` gets through where
every other designator is turned away.

```
$ echo one two
$ echo [!!]
bash → [echo one two]     osh → [!!]        (inhibited, left literal)
$ echo [!!!]
bash → !]: event not found osh → [!echo one two]
```

The `[!!!]` case shows the shape of the bug: osh inhibited the first `!` and then
expanded the `!!` that followed it, where bash expands the leading `!!` and fails
on the leftover `!]`.

**Fixed (2026-07-30).** One condition in `inhibited()`:

```rust
if prev == Some('[') && next != Some('!') && rest.contains(&']') { return true; }
```

Measured against bash 5.2: `[!!]`, `[!!x]`, `[!!$]`, `[!!:1]`, `x[!!]y` and
`[[!!]]` all expand; `[!^]`, `[!$]`, `[!:0]`, `[!-1]`, `[!*]`, `[!%]` and
`[a[!$]]` are all left alone. Inhibition is per-`!` and does not extend over the
rest of the bracket, so in `[!]!!]` — where the `]` is a literal member rather
than the close — the first `!` is inhibited and the `!!` after it still expands.

**Tests.** `shell_syntax_inhibits_expansion` (both lists, plus the `[!!!]`
`"!]: event not found"` assertion) and a section of
`tests/corpus/histexpand-inhibit.sh`.
