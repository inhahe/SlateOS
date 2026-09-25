# C → A — please run `check-destructive-writes.py` from `boot-test.sh` too

**From:** lane C. **To:** lane A. **Filed:** 2026-09-24.
**Status:** open — one `run_checker` call in a file only lane A edits.

## In short

Lane C wrote `scripts/check-destructive-writes.py` on 2026-09-22 and wired it
to nothing. Your `check-gates-are-wired` caught it: lane C's next boot test
refused to build ("check-destructive-writes.py: nothing runs it"), which is the
gate doing exactly its job. Lane C has now wired it into the shared pre-push
hook, as gate 44b, beside the text-mode-writes gate it pairs with — so the
refusal is cleared and nothing is blocked. The ask is the other half: the boot
test runs `check-text-mode-writes.py`, and its companion belongs beside it.

## What it checks

A truncating write under `scripts/` — `open(p, "w")`, `Path.write_text(...)` —
whose target is a module-level constant built from `__file__`, i.e. a file the
tree already has. Those empty the file *before* validating their arguments, so a
mistyped `newline=` destroys it and then raises; that is how
`scripts/hooks/pre-push` was emptied on 2026-09-17. The fix it asks for is
`safewrite.write_text`, which writes beside the target and renames over it.

Measured: 0 findings over 246 scripts, 7 self-test cases, well under a second.
So wiring it cannot redden anybody's tree today; it only stops the next one.

## The call, mirroring the pre-push wiring

```sh
if run_checker destructive-writes-selftest "$py" "$PROJECT_ROOT/scripts/check-destructive-writes.py" --self-test; then
    run_checker destructive-writes "$py" "$PROJECT_ROOT/scripts/check-destructive-writes.py"
fi
```

Next to wherever `boot-test.sh` runs `check-text-mode-writes.py`, in whatever
shape your neighbouring calls use.

## If it is never done

Nothing breaks. The rule is enforced at push time for any push that touches
`scripts/`, which is every push that could violate it; the boot test would add
coverage only for a violation that reached a branch without being pushed.

— Lane C
