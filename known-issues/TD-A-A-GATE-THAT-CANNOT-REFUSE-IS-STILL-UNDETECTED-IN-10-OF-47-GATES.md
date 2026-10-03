## TD-A-A-GATE-THAT-CANNOT-REFUSE-IS-STILL-UNDETECTED-IN-10-OF-47-GATES (lane A, 2026-09-03)

**In short:** `scripts/check-gates-can-refuse.py` exists to answer one
question about every build gate — *if this checker finds a problem, can it
actually fail the build, or does it print and exit 0?* Measured today, it
answers correctly for 37 of the 47 gates and is fooled by the other 10. That
is up from 12 of 47 before today's fix, so the gate is now doing real work;
this entry records what it still cannot see, so nobody reads its "ok" as a
statement about all 47.

**How the number is measured, since a checker's own claim is not evidence.**
Take each gate, rewrite every literal non-zero exit status in it to 0 — a
mechanical transform producing a checker that provably cannot refuse
anything — and ask whether `check-gates-can-refuse.py` notices. Anything it
misses is a real blind spot, not a hypothetical one. The census is ~20 lines
of `ast.NodeTransformer` and takes under a second; it is worth re-running
after any change to the analysis, and the numbers below are from
`ab173d435`.

| | detected |
|---|---|
| before `ab173d435` | 12 / 47 |
| after | **37 / 47** |

**What the fix covered.** A `return` of a call to a function defined in the
same module is now resolved by analysing that function, instead of being
waved through as "delegated, assume it can refuse". The idiom that mattered
is `return _decline(reason, detail)` — a helper that prints a no-verdict
message and returns 2 — which appears in most gates here; one such call
anywhere in `main()` used to clear the whole file. Both arms of
`return 1 if findings else 0` are now read as well.

**What still gets through, by shape.** These are the expressions the analysis
still cannot follow, with the gates each affects:

| shape | example | gates |
|---|---|---|
| a returned local variable | `return bad` | `check-evdev-elf-asm.py`, `check-query-status.py`, `check-usage-status.py` |
| a returned comparison / comprehension / subscript | `return classify(...) == 'production'` | `check-option-refusal.py`, `scan-orphan-modules.py` |
| a call whose own body contains an unanalysable expression | `return selftest()` where `selftest` returns a name | `argv-utf8.py`, `check-selftest-reinit.py`, `host-errmsg.py`, `raced-globals.py` |
| no `main()` at all — analysis falls back to the module body, which is much weaker | — | `rustscan.py` |

Note the third row is a *chain*: resolving a call is only as good as the
analysis of the callee, so one unreadable expression deep inside a helper
still clears the caller. That is the conservative direction working as
designed, and it is why the remaining gap is not simply "four more
expression types".

**Proper fix.** Single-assignment local dataflow inside a function: if a
returned name is bound exactly once, from an expression the analysis can
already read, substitute it. That covers the first row outright and part of
the third. The second row needs a value lattice (is this comparison ever
truthy?) and is probably not worth it — a checker that returns a comparison
directly is rare and reads oddly anyway. `rustscan.py` is better fixed at
the source, by giving it a `main()`, than by strengthening the module-level
path for one file.

Do **not** fix this by widening the "assume it can refuse" fallback in the
other direction, i.e. reporting everything unanalysable. The file's header
argues the opposite and is right: this gate runs on every build, and a
version that cries wolf on ten gates gets its findings ignored, at which
point it detects nothing at all regardless of what its analysis can see.

**If it is never fixed:** the current state is safe but oversold. All 47
gates can refuse *today* — that was verified directly, not assumed — so
nothing is currently broken. The risk is future: if one of those 10 gates
later loses its refusal (a `return 1` becomes a `return 0` in a refactor,
which is exactly how `check-doc-links.py` broke and why this file exists),
this will report "ok — all 47 gates can reach a non-zero exit bare" and be
wrong, in the same words it uses when it is right.
