## 644. A shell command with no subcommand prints `cal: …`, not `cal: : …` — one printer decides the prefix, and the wording gate is told why the guard it cannot derive is still real

**Date:** 2026-08-30
**Lane:** A
**Decided by:** Claude (autonomous), lane A

**In short:** When you mistype a number at a kshell command, the shell tells you
so, and the message starts with the command's name and the subcommand you used
— `cputhr: clear: missing CPU number`. But a handful of commands have no
subcommand at all: `cal` takes a month and a year and nothing else. Those were
printing an empty middle field, `cal: : \`1O' is not a month`, which reads like
a bug in the message rather than like an absence. This entry records collapsing
that field, doing it in one shared function rather than at every call site, and
what had to be added to the self-test gate so it would accept the assertion that
keeps it collapsed.

### The three ways to print a prefix with nothing in the middle

Batch 42 of the §600 guessed-value burn-down was the first to reach commands
that take operands *directly*, with no subcommand word between the command and
the number. Two of them showed up in the same batch — `cal` and `hexdump -n` —
so it was not a one-off.

| option | `cal 1O 2026` prints | why not |
|---|---|---|
| (a) pass `""` through the existing `"{}: {}: {}"` | `cal: : \`1O' is not a month` | the empty field reads as a defect in the shell, not as "this command has no subcommand" |
| (b) invent a filler word | `cal: operand: \`1O' is not a month` | names a subcommand that does not exist and that the operator cannot type; the next person to read it goes looking for `cal operand` |
| (c) **collapse the field** | `cal: \`1O' is not a month` | chosen |

(c) needs no explaining because it is what every other system already does:
`ls: cannot access 'x'`, `rm: cannot remove 'x'` — command, colon, complaint.
An operator who has used a shell before has read a thousand of these.

### The prefix is decided in one function, not at ten call sites

The family has ten printers (`readable_num`, `readable_hex`, `required_num`,
`required_hex`, `required_key`, `required_id`, the three toggle helpers, and the
"missing" arms). Making each of them choose its own prefix would mean ten copies
of the same `if sub.is_empty()`, and — worse — twenty format strings, since each
message would need a two-field and a three-field spelling. They now all call:

```rust
fn refuse_operand(cmd: &str, sub: &str, body: core::fmt::Arguments<'_>)
```

which decides the prefix once and sets the exit status once. Taking
`core::fmt::Arguments` rather than `&str` is what makes this affordable in a
`no_std` kernel with no allocator on the parse path: the body is formatted
lazily at the call site, so nothing is built unless the refusal actually
happens, and these helpers sit on the parse path of *every* command in the
shell.

*What it costs:* the message is now assembled by two functions instead of one,
which is exactly what the next section is about.

### The wording gate had to learn `format_args!`, and then be told one thing it cannot derive

`scripts/check-selftest-wording.py` checks that every self-test assertion names
text the command under test can actually print. It does that by collecting the
string literals a command hands to a print macro. Splitting the message across
`refuse_operand` (prefix) and the helpers (body) moved every body inside a
`format_args!`, which was not in the gate's list of print macros — so the bodies
vanished from every command's pool and the gate reported ~40 *correct* rungs as
failures. `format_args!` is a deferred print: it does not print, it hands the
literal to something that does. Adding it to the list is not a loosening; it is
the gate seeing text it was always supposed to see.

Five older rungs then still failed, because their needles spanned *both*
literals (`"{}: {}: {}"` plus `"missing {}"`) and the gate deliberately tests a
needle against one literal at a time. The tempting fix — teach it to compose
across the helper boundary — was rejected on the gate's own stated grounds: *"a
checker that re-derives what a helper prints will drift away from the helper."*
The five needles were shortened to the body-only form instead, which is what the
other ~40 refusal rungs in the file already do.

That leaves one assertion the gate cannot derive and that is worth keeping:

```rust
assert_output_lacks("and the subcommand field is collapsed, not left empty", &out, b"cal: :");
```

This is the only thing pinning the decision above. The gate calls it vacuous,
and by its model it is: `producible` requires a `{}` to cover at least one byte,
so it cannot see that `cal: :` is precisely `"{}: {}: {}"` — a literal that *is*
in `cal`'s pool — with an empty middle. Relaxing the placeholder rule globally
would let a `{}` swallow any fragment and mask the typos the gate exists for, so
the conservatism stays and the assertion is reconciled by an `ALLOWED` entry
carrying this reasoning. That is the mechanism the file already provides for
"the gate is right to be conservative here, and here is why the assertion is
still real."

*What it costs:* an eighth entry in a table that should stay small. The
alternative was deleting the only runtime check that the prefix stays collapsed,
which would make this whole entry a comment rather than a guarantee.

### How to reverse it

Delete `refuse_operand` and inline `shell_println!("{}: {}: {}", …)` back into
the ten printers. kshell rung 109's first block and the `("cal", b"cal: :")`
`ALLOWED` entry go with it. Note that reversing only the *printer* while leaving
`cal` and `hexdump` calling the family with `sub = ""` reinstates the empty
middle field, which is the state this entry exists to leave behind.
