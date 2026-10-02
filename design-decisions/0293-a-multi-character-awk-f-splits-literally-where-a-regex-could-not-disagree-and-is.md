## §293 — A multi-character `awk -F` splits literally where a regex could not disagree, and is refused where it could

**Date:** 2026-08-25
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** `awk -F', '` means "split each line on comma-space". In real awk
a separator longer than one character is a *regular expression* — a pattern
language where `.` means "any character", `[a-z]` means "any of these", and so
on — and the kernel shell has no engine that can evaluate one. Rather than
guess, it now splits on the exact characters you typed when that is provably
the same answer a regex would give, and refuses outright when it is not. So
`-F', '` works; `-F'[,;]'` says it cannot and exits 2.

**Background.** POSIX defines awk's field separator `FS` in two halves. A
**single** character is used literally, always — which is why `-F.` splits on
dots even though `.` is a regex metacharacter. Anything **longer** than one
character is an extended regular expression. `-F', '` is therefore, strictly, a
two-character regex — one that happens to match exactly the two characters
`,` and ` `, because neither is special.

The kernel shell's `awk` cannot evaluate a regex at all: it has no engine
(`TD-A-THE-KERNEL-SHELLS-AWK-MATCHES-REGEXES-WITH-SUBSTRING-SEARCH`), and
adding one would mean a second copy of `userspace/ere` living in the kernel,
which is the outcome that crate exists to prevent.

**What it did before.** Neither of the two readings. `awk_split_fields` ended:

```rust
} else {
    alloc::vec![line]
}
```

— one field holding the whole record, and exit 0. So `awk -F', ' '{print $2}'`
printed a blank line per record, which is exactly what a file whose records
genuinely have no second field looks like.

### The options

| Option | *What changes* |
|---|---|
| Keep returning the whole record as `$1` | nothing; `$2` stays silently empty |
| Refuse every multi-character `-F` | `-F', '` stops working, and it is the common case |
| **Literal where unambiguous, refuse where not** ✓ | `-F', '` works; `-F'[,;]'` prints an error and exits 2 |
| Implement a regex engine in the kernel | everything works, and there are two engines to keep in agreement |

**Why the third.** The test is mechanical and it is *sound*: a separator
containing none of `.[]()*+?{}|^$` matches, as a regex, exactly itself — so the
literal split and the regex split are the same function, and choosing the
literal one is not a guess but a proof. When a metacharacter is present the two
readings genuinely differ, and there is no basis for preferring either; that is
the case that gets refused.

The split matters because the two halves are not equally common. Separators
people actually write — `', '`, `' | '`, `'::'`, `'\t\t'` — are almost all
metacharacter-free. Separators that need a real regex are rare and are, in this
shell, unsupportable. Refusing both would make awk useless for the common case
to be honest about the rare one; accepting both would be the silent guess this
whole series of fixes is about removing.

**Exit 2, not 1.** `awk` already uses 1 for "a file could not be read", which
is a per-operand condition that other operands may survive. A separator that
cannot be evaluated is a property of the *invocation*: nothing will be read at
all. gawk likewise reserves 2 for a fatal error before processing. Keeping them
distinct means `awk -F'[,;]' … || retry-with-another-file` cannot mistake one
for the other.

**What this could cost later.** If `ere` gains a `no_std` build and the shell
gets a real engine, the refusal becomes dead code and the metacharacter test
goes away — `awk_field_sep` returns `AwkFs::Regex(compiled)` instead. Nothing
else has to change: `awk_split_fields` already dispatches on the `AwkFs` enum,
so the third variant slots in beside `Whitespace` and `Literal`. No user-facing
behaviour that works today would change, because everything that works today
works for the reason that its literal and regex readings coincide.

**A smaller call inside this one.** `-F ''` — a zero-length separator — splits
every character into its own field in gawk. It is refused here rather than
supported or silently treated as the default, because the literal splitter
would loop forever on it and nothing in the tree asks for it. That is a
limitation, and it says so.
