## §294 — The kernel shell's `awk` refuses a program it cannot run whole, rather than running the parts it recognises

**Date**: 2026-08-25
**Decided by**: Claude (autonomous)
**Lane:** A

**In short:** The kernel's built-in `awk` only knows how to do one thing —
print. Faced with anything else it has been quietly skipping that part of the
program and carrying on, so `awk '{ n = n + 1 } END { print n }'` printed a
blank line and reported success: the counting step was dropped and nothing
said so. The choice was between teaching it the rest of the awk language and
making it say plainly that it cannot. It now says so: a program containing
anything outside the supported set is refused before it runs, with the exact
fragment quoted, and exits 2.

**The state before.** `awk_exec_action` split the action on `;` and handled
`print`, `print $0` and `print <list>`. Its `else` arm was a bare comment
reading "Unknown statement — ignore." `awk_pattern_matches` had the same shape
one level up: after the `NR`/`NF` comparisons it fell through to
"treat as a literal substring match", so `awk '$1 > 5 { print }'` searched each
record for the seven characters `$1 > 5`, found none, and printed nothing.

Both are the silent-guess shape this series exists to remove: not a refusal, an
answer, and the wrong one. They are worse than most, because the *shape* of the
output is right. A blank line where a count was expected, or no lines where a
filter was expected, both look like a legitimate result on data that happened
not to match.

| written | before | now |
|---|---|---|
| `awk '{ n = n + 1 } END { print n }'` | prints an empty line, exit 0 | `awk: unsupported statement: 'n = n + 1'`, exit 2 |
| `awk '{ if ($1 > 5) print }'` | prints nothing, ever, exit 0 | `awk: unsupported statement: 'if ($1 > 5) print'`, exit 2 |
| `awk '$1 > 5 { print }'` | prints nothing, exit 0 | `awk: unsupported pattern: '$1 > 5'`, exit 2 |
| `awk '{ printf "%s\n", $1 }'` | prints nothing, exit 0 | `awk: unsupported statement: 'printf "%s\n", $1'`, exit 2 |

**The alternative was to implement the language.** What is missing is
assignment, arithmetic, user variables, `if`, `printf`, `sub`/`gsub`,
`substr`/`length`, `next`, `exit` and `getline` — which is to say, most of awk.
Two reasons not to:

- **Userspace already has one.** A second awk in the kernel would have to agree
  with the userspace one forever, and would not, for the same reason a second
  regex engine would not. That is the divergence the `ere` crate exists to
  prevent, and the argument does not change because the duplicate is an
  interpreter rather than a matcher.
- **`kshell` is a pre-userspace debugging tool.** Its job is to be correct about
  a small set of things, early, with no dependencies. A small correct subset
  serves that; a large approximate one does not.

**Why refusal is not merely the cheaper option.** A refusal is checkable. The
validator runs over the parsed rules *before any rule executes*, so the verdict
depends only on the program text and not on the data — a program that would
have been mis-run on some inputs is refused on all of them, including the
inputs where the wrong answer would have looked right. That is the property
that makes the refusal worth more than a partial implementation: there is no
input for which the shell quietly does the wrong thing.

**Exit 2, matching `-F`.** As in §293: 1 is `awk`'s per-operand status for a
file it could not read, and other operands may still succeed. A program it
cannot run is a property of the invocation — nothing will be read at all — so
it takes the invocation-fatal 2, which is also where gawk puts a fatal error
raised before processing begins.

**The refusal is a floor, not a ceiling.** Every statement listed above can be
added later, and each addition is then a deliberate widening of a stated subset
rather than another silent hole. The order this suggests, by how often each
appears in the shell's own scripts: `printf`, `exit`, `next`, assignment with
integer arithmetic, `if`. `getline` and the string functions are further off
and may never be worth it.

**One thing this deliberately does not refuse.** A `/pattern/` is still matched
as a substring rather than as a regular expression. That is a wrong answer of
exactly the kind this entry removes elsewhere, and it is left in place only
because the fix is a real regex engine, which is filed as
`requests/a-b-ere-is-std-only-so-the-kernel-shell-still-matches-regexes-with-contains.md`
and tracked as `TD-A-THE-KERNEL-SHELLS-AWK-MATCHES-REGEXES-WITH-SUBSTRING-SEARCH`.
Refusing every `/…/` with a metacharacter would be consistent, and is the
documented fallback if that request is declined; it is not done pre-emptively
because it would break working invocations while the request is still open.
