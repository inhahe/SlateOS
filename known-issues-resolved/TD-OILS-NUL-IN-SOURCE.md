### TD-OILS-NUL-IN-SOURCE. osh keeps NUL bytes read from shell source; bash drops them, and refuses a script whose first line has one — 2026-08-02 — ✅ **RESOLVED 2026-08-02**

**Resolution.** Both halves are now bash's, and each lives in exactly one place.

* `lexer::strip_nuls` is the reader's NUL removal — bash's `shell_getc` throwing a
  NUL away as it reads it. It is called from `IncrementalParser::new` and
  `parse_opts`, which are the *only* two doors into the lexer, so every way source
  arrives goes through it: a script file, `-c`, `eval`, `.`/`source`, a trap body,
  a piped REPL, a `$( … )` re-read. Putting it at the parser rather than at each
  reader also keeps the byte offsets honest — the spans kept alongside the tokens
  index the text that was tokenized, and that is the stripped text. Source with no
  NUL (all real source) is borrowed through untouched.
* `interp::head_is_binary` is bash's `check_binary_file`: an ELF magic, or a NUL
  before the first newline. `main.rs`'s `Plan::Script` arm applies it before parsing
  a byte and exits 126 with `FILE: FILE: cannot execute binary file` — named twice
  because `$0` is by then the script itself, which is what bash's `internal_error`
  produces. `shell_script_indirection` (the shebangless-exec path) now shares the
  same classifier instead of open-coding it.

Two things about bash here were **measured, not assumed**:

* the sample sizes differ per caller and are therefore observable. `open_shell_script`
  (`bash FILE`) reads **80** bytes — a NUL at byte 79 of line 1 is refused, one at
  byte 80 is not — while `shell_execve` reads **128**. `head_is_binary` takes the
  sample and does not pick its size; `interp::SCRIPT_BINARY_SAMPLE` is the 80 and
  the exec path keeps its own 128.
* only that reader refuses. `source`ing the very same file reads it happily, NULs
  and all, and so does a piped REPL: the gate is on the script the shell was
  *invoked* on and nowhere else.

Covered by `tests/corpus/nul-bytes-in-shell-source.sh` (byte-identical to bash
5.2.37 on the first run) plus
`a_file_is_binary_only_for_a_nul_in_the_part_that_was_sampled` in `interp.rs` and
`the_reader_drops_a_nul_before_the_lexer_sees_it` in `parser.rs`. The
`latenul.sh` case this issue had blocked is restored to
`tests/corpus/exec-shebangless-script.sh`, so "a NUL past the first newline is
still text" is now *run* rather than only unit-tested. Full suite green (1155 lib
tests), clippy clean, corpus sweep 264 matched / 0 failed.

**What it was, below.**

**Where:** every place osh reads shell source — `userspace/oils/src/main.rs`
(`Plan::Script`, the stdin REPL) and the `source`/`.` builtin in
`userspace/oils/src/interp.rs`. The bytes reach the lexer exactly as read.

**What:** two separate bash behaviours, neither of which osh has.

1. bash's `shell_getc` **discards NUL bytes** from the input stream, so a NUL
   never reaches a word. osh keeps them, and a word carrying one blows up much
   later — when the word is handed to `Command`, which cannot put a NUL in an
   argument.
2. bash's `open_shell_script` applies `check_binary_file` to the script it was
   asked to *read* and refuses a binary one outright.

```
$ printf 'echo one\n\000\necho two\n' > n.sh
$ bash n.sh   →  one / two                                        (rc 0)
$ osh  n.sh   →  one / n.sh: line 2: ^@: nul byte found in provided data / two

$ printf 'echo a\000b\n' > m.sh
$ bash m.sh   →  bash: m.sh: cannot execute binary file
$ osh  m.sh   →  a^@b
```

**Proper fix.** Strip NUL bytes where source bytes enter the lexer — one place
for all three readers, not three — and add the `check_binary_file` gate (already
written, as `shell_script_indirection`'s classifier) to the script reader, so a
binary handed to `osh FILE` is refused rather than parsed.

**Impact.** Found while writing `tests/corpus/exec-shebangless-script.sh`, whose
"a NUL past the first newline is still text" case cannot be *run* until this is
fixed. The classifier is unit-tested there instead, and the corpus case names
this issue where the run belongs.
