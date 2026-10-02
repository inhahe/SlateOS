### TD-OILS-CMDSUB-DOLLAR-NOT-INCREMENTAL. A `$( … )` body runs as one pre-parsed program, so an `alias`/`shopt` set inside it does not affect the rest of it — 2026-07-29 — ✅ RESOLVED 2026-07-29

**Where:** `userspace/oils/src/interp.rs` `Shell::command_sub` (the
`exec_program(prog, …)` call); `userspace/oils/src/parser.rs`
`parse_cmdsub_body`, which produces the `CmdSubBody::Parsed` program.

**What:** bash parses a `$( … )` body *twice*: once with the enclosing token
stream, purely to find the matching `)` (that scan is where the fatal syntax
error of `TD-OILS-CMDSUB-ERR-FATALITY` item 2 comes from), and again at
expansion time, when `command_substitute` hands the body *text* to
`parse_and_execute`. The second pass is a read-eval loop, so the body is read
one logical line at a time and a state change made by line N is visible to the
parse of line N+1. osh only does the first pass and then executes the resulting
`Program` wholesale, so it is not.

**Reproduce:**
```sh
shopt -s expand_aliases; x=$(alias q="echo hi"
q); echo "[$x]"
# bash: [hi]
# osh:  osh: line 2: q: command not found  /  []
```
The same probe with `shopt -s extglob` in place of the alias diverges the same
way.

**Note:** the backtick spelling was fixed on 2026-07-29 (see
`TD-OILS-CMDSUB-ERR-FATALITY` item 1) because deferring its parse to expansion
time *is* the fix; `Shell::backtick_sub` routes the body through
`run_source_flow_out`. This entry is the residue: the `$( … )` spelling still
needs its body re-read at expansion time.

**Fix (2026-07-29):** done as described. `CmdSubLineMap` — which was private to
`parser.rs` and knew only the `$( … )` rank rule — was generalised into a public
`ast::LineMap` enum covering *both* renumbering rules the shell uses:
`Offset(n)` (a REPL fragment, an `eval` string, a backtick body) and
`CmdSub { pre, close_line, ranked }` (the rank rule). It also subsumes the
old free functions `shift_lines`/`shift_segs`, which were the same traversal
with the other mapping function; both are now `map_lines`/`map_segs` taking a
`&LineMap`. `LineMap` additionally provides `shifted(n)` (compose with a
re-lexed tail's restart, for `IncrementalParser::relex`) and `unmap(reported)`
(the partial inverse, for echoing the offending source line back in a
diagnostic).

`IncrementalParser::new` and `Shell::{run_source_flow_out, format_parse_error}`
now take a `LineMap` instead of a `u32` base. `parse_cmdsub_body` returns the
map alongside the program, `CmdSubBody::Parsed` became a struct variant holding
`{ prog, src, map }`, and `Shell::command_sub` takes `(src, map, read_file)` and
runs the *text* through `run_source_flow_out` — so the one function now serves
both spellings, and `backtick_sub` is gone (the backtick body just passes
`LineMap::Offset(close_line - 1)`). `Shell::backtick_body` was renamed
`comsub_read_eval`, since the flag now means "this read-eval loop is a command
substitution's, of either spelling" rather than "backtick".

`run_command_sub_text` (a `$( … )` embedded in an arithmetic expression, which
reaches the interpreter as raw text) was routed through the same path, deleting
the second, program-based `command_sub` that had been duplicating the capture /
exit-trap / `$(<file)` logic.

**Coverage:** `tests/corpus/cmdsub-incremental.sh` — an alias defined by the
body applying to the rest of it (and not leaking out), a `shopt` doing the same,
an expansion-time syntax error that is not fatal to the caller, the commands
before it having already run, the error repeating once per expansion in a loop,
the `$LINENO` rank rule still holding, and the `$(< file)` fast path unaffected.
Byte-identical to bash 5.2.

**Note:** `shopt -s extglob` inside the body deliberately does *not* appear in
that corpus case: `@(` is rejected by the *first* read, which is fatal to the
whole script, so it never reaches the re-read. `expand_aliases` is the only
shopt whose effect is purely on the read-eval loop.
