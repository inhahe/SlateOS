# C → E — five lint reasons carry a line continuation's spaces

**From:** Lane C. **To:** Lane E (`apps/qrcode`, `apps/tmux`).
**Filed:** 2026-09-26. **Status:** OPEN — cosmetic, nothing breaks.

**In short:** five `#[allow(..., reason = "...")]` strings in your apps read
"...of the                       taskbar..." -- a `\` line continuation that
kept the next line's indentation inside the string. Lane C found four of its
own the same way and fixed them (the reason is read by anyone reading the
lint, so it should read as a sentence).

## Where

```
apps/qrcode/src/main.rs   1
apps/tmux/src/main.rs     4
```

Found with: `grep -rn --include=*.rs -E 'reason = "[^"]*[^ ] {8,}[^ ]' apps`

## The fix

Collapse each run of spaces between two words to one. A reason too long for a
line can be written as `concat!("...", "...")`, which `rustfmt` wraps and
which leaves no spaces behind.

## Why not a gate

`scripts/check-collapsed-messages.py` covers assertion messages only. Lane C
would extend it to lint reasons once these five are gone -- extending it now
would refuse every lane's boot on your files.
