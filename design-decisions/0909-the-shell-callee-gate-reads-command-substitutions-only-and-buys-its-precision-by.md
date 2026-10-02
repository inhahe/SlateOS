## §909 — The shell-callee gate reads command substitutions only, and buys its precision by narrowing the question

**Date:** 2026-09-04. **Decided by:** Claude (autonomous). **Lane:** A.

**In short:** A safety check in the boot test called a helper that had never
been written. Because of how it was written, the failure looked exactly like
success, so that check sat there doing nothing from the day it was added. The
obvious repair — have something verify that every command a script calls
actually exists — cannot be done in general, because shell scripts routinely
build command names out of variables at the last moment. So the new gate
answers a smaller question it *can* answer, and refuses the rest by design. The
decision is to accept a check that knowingly ignores most of the tree in
exchange for one that is never wrong about the part it does read.

### The problem

`scripts/boot-test.sh`'s `check_libc_shape` opened with
`py="$(find_python)" || return 0`. Nothing defines `find_python`, so the
substitution exited 127, the `|| return 0` converted that into a pass, and the
gate plus its 24-case self-test had never executed on any host. `set -e` does
not fire (`||` is an explicit handler, which is its purpose), `bash -n` accepts
it (the syntax is valid; the failure is at run time), and shellcheck is looking
for undefined *variables*. See `known-issues.md` →
`A-A-THE-LIBC-SHAPE-GATE-WAS-BORN-DEAD-AND-THE-WIRING-GATE-CALLS-IT-WIRED`.

### The options

**A. Grade every command word.** Every word in command position — line start,
after `;`, `|`, `&&`, `(`, `then`, `do` — must resolve.

*What changes:* the gate reads roughly ten times as many words, and covers
plain calls (`frobnicate --help`) as well as substitutions.

**B. Grade the first word of a command substitution only** — `$(word …)` and
the backquote form. *(chosen)*

*What changes:* `frobnicate --help` on its own line is not read at all; only
`x="$(frobnicate --help)"` is.

### Why B

**It is the only one of the two that is decidable.** Command words come from
variables, `eval`, arrays and `"$@"`. After `$(` the shell is unambiguously
parsing a fresh command, and if the first token is a bare identifier rather
than a `"` or a `$`, it is a literal command name and there is no other reading
of it. Option A has to guess at the same construct in contexts where the guess
is often wrong.

**The failure it catches is the worse of the two.** A bare failing command
trips `set -e` and stops the script — loudly, at the right moment. A failing
*substitution* yields the empty string plus a status that the surrounding `||`,
`if`, or `local` swallows, so the caller proceeds with an empty value believing
it succeeded. Option A's extra coverage is concentrated on the cases that
already fail safely.

**It was tested, and A lost on the actual bug.** Option A was implemented
first. It returned 42 findings, every one a false positive — arithmetic inside
`$(( ))`, awk program variables, C declarations in heredoc'd source (`size_t`,
`pid_t`) — and it **did not find `find_python`**, because the pattern wanted
whitespace after the name and the real text is `$(find_python)"`, where the
next character is `)`. Option B found it immediately. A scan written
specifically to catch this bug did not catch it, while reporting "42 findings"
in a tone indistinguishable from working.

**Measured signal.** Option B over the 104 graded files: one true finding, zero
false positives. That number is the argument. A gate that fires on a tenth of
the tree is one somebody switches off next month; a gate with one finding and
no noise is one that still means something when it fires.

### What this gives up, deliberately

Plain command calls are unchecked, so a script calling a misspelled
`frobnciate` on its own line is still caught only at run time — by `set -e`,
which is the outcome we judged acceptable above. Variable-driven callees
(`$("$py" foo.py)`) are outside the gate by construction and always will be.
And a name present on `PATH` here but absent on another host still passes;
guarding those with `command -v` remains the author's job, which is what the
refusal message says.

### Reversing this

Widen `CALLEE_RE` to the command-position rule and delete the
`(?:\$\(|`)` prefix. Expect the 42 false positives back and budget for teaching
the masker about awk, make and C bodies. The evidence that this is the wrong
call would be a real defect found in plain command position that the
substitution rule missed — which is worth watching for, and has not happened
yet.

### A note on where scanners actually cost

The rule above never changed after it was specified. All 193 false positives
the implementation went through came from the *masker* — deciding which bytes
are shell at all — and this tree is unusually hostile there, because its
refusal messages are English prose that quotes code, so
``echo "\`article_for\` picks by spelling"`` reads as a backquote substitution
unless escapes are blanked. Budget the lexer; the matching is free.
