### TD-KSHELL-LINE-EDITOR-IS-UTF8. The shell's line editor and tab completion cannot type or complete a non-UTF-8 filename — 2026-08-13 — LOGGED 2026-08-13

**Where:** `kernel/src/kshell.rs`, the line-editor buffer and the tab-completion
path (a code comment there points at this entry).

**What it is:** `D-VFS-PATHS-ARE-STR-NOT-BYTES` made the VFS, the syscall
boundary and `DirEntry.name` byte-clean, so a file named `re\xffport.txt` can
now be created, listed, opened and deleted through the API. The kshell line
editor, however, still holds the command line as a `String` and completes
against `String`s. So such a file is visible in `ls` (via `.display()`, which
renders the stray byte as U+FFFD) but **cannot be typed or tab-completed**:
the completion candidate is dropped rather than inserted, and there is no way
to enter the raw byte from the keyboard.

**Why it is debt and not a bug:** nothing is corrupted or silently lost — the
name is displayed honestly and every programmatic path works. It is a
usability gap in one interactive front end.

**Proper fix:** make the editor buffer a `Vec<u8>` with a separate
grapheme/column model for cursor movement and redraw (the display width of a
byte run is already computed lossily for rendering; that part stays), have
completion produce `PathBuf` candidates, and add an escape input (e.g. the
`$'\xff'` spelling the shell already parses in word expansion) so a byte with
no key can still be entered. Not blocked on anything; deferred because the
editor's cursor arithmetic is `char`-indexed throughout and converting it is a
self-contained but non-trivial rewrite.

**Scoping pass 2026-08-14 — two claims above are wrong, and the real cost is
in a different place than this entry says.** Read the code before planning
from the paragraph above.

**Correction 1: the cursor arithmetic is not `char`-indexed. It is already
byte-indexed throughout.** `cursor` is derived from and compared against
`buf.len()` (a *byte* length) at every site — `kshell.rs:3333, 3370, 3374,
3376, 3395, 3410, 3509, 3531, 3554, 3573, 3582, 3586` — and the redraw path
already slices bytes (`buf.as_bytes().get(cursor..)`, `redraw_from_cursor`,
`kshell.rs:2955-2969`) and already emits bytes (`console::putchar(b)`).
`replace_line` (`kshell.rs:2921-2949`) is likewise pure byte arithmetic. So
the editor is not a UTF-8-aware editor that needs converting to bytes; it is a
byte editor whose buffer happens to be typed `String`. Converting the four
editor functions is close to a mechanical type change (`String`→`Vec<u8>`,
`&str`→`&[u8]`), *not* the "non-trivial rewrite" this entry warns about.

Corollary: because the arithmetic is byte-based while `String` is char-based,
the current code is already subtly wrong for multibyte input — `String::remove`
/`String::insert` at a non-char-boundary byte index panic. It never fires only
because of the input guard described next.

**Correction 2: the escape-input suggestion cites the wrong shell.** The
`$'\xff'` ANSI-C quoting that "the shell already parses in word expansion" is
**Oils/osh** — `userspace/oils/`, Lane B, userspace. This entry is about
**kshell**, the in-kernel shell, which has no ANSI-C quoting at all (no
`$'...'` parser exists in `kernel/src/kshell.rs`). So the escape input has to
be *built*, not reused, and whoever picks this up should not plan around
borrowing it.

**The actual gate on typing a high byte** is a single match guard:
`kshell.rs:3580`, `ch if ch >= 0x20 && ch < 0x7F`. Bytes ≥ 0x80 fall through
to the `_ => {}` arm and are silently discarded. **Landmine for whoever widens
it:** the insert on the next line is `buf.insert(cursor, ch as char)`
(`kshell.rs:3583`). `ch` is a `u8`; `ch as char` maps 0x80..=0xFF to
U+0080..=U+00FF, which `String::insert` then encodes as **two** UTF-8 bytes.
Widening the guard without changing the buffer type would therefore insert two
bytes for one keystroke and desynchronise the byte-indexed cursor from the
buffer — a corruption bug, and a worse state than today's honest refusal. The
type change must land first, or with it.

**Where the cost actually is: the parser, not the editor.** The editor is four
functions (`read_line` `3191-3601`, `reverse_search_mode` `3061-3190`,
`replace_line`, `redraw_from_cursor`) with exactly **one** real caller
(`kshell.rs:2892`; the only other is a unit test at `64187`). The expensive
part is everything downstream of that caller, which is uniformly `&str`:
`execute(line: &str)` (`3872`), `execute_single` (`4149`), and the sibling
statement executors `execute_while_loop`, `execute_select`,
`execute_until_loop`, `execute_for_loop`, `execute_cfor_loop`, `execute_case`,
`execute_function`, `execute_input_redirect`, `execute_redirect`,
`execute_heredoc`, `execute_pipe_chain` — plus `resolve_path(path: &str) ->
String` (`208`), which is the ~270-call-site figure this entry quotes. History
(`entries: Vec<String>`, `2629`) has to move to `Vec<Vec<u8>>` as well, or
recalling a byte-bearing command re-corrupts it.

**Correction 3 (2026-08-14): 270 is the `resolve_path` count, not the size of
the job — and quoting it here has been under-selling this task by ~6×.** The
conversion is bounded by every *`str`-only method* reachable from
`execute()`, not by one function's callers. Measured over the 84,845 lines of
`kernel/src/kshell.rs`:

| method | sites | on `[u8]`? |
|---|---:|---|
| `.parse::<…>` | 1153 | no — parse at the numeric leaf via `from_utf8` |
| `.split_whitespace` | 552 | **no equivalent — must be written** |
| `.trim*` | 292 | free (`trim_ascii`, already stable; used in `oci.rs:683`) |
| `.push_str` | 160 | free (`extend_from_slice`) |
| `.starts_with` | 138 | free (`slice::starts_with`) |
| `.as_str` | 109 | free (`&v[..]`) |
| `.strip_prefix` | 89 | free (`slice::strip_prefix`) |
| `.to_lowercase` | 63 | `to_ascii_lowercase` (already byte-correct here) |
| `.ends_with` | 53 | free (`slice::ends_with`) |
| `.splitn` | 51 | signature differs (`slice::splitn` takes a predicate) |
| `.split_once` | 15 | **no equivalent — must be written** |

So ~**1520 `str`-method sites** plus ~**1150 `parse::<>` sites**, against the
~270 this entry advertised. The useful split is that **631** of them
(`trim`/`starts_with`/`ends_with`/`strip_prefix`) need *no* new code at all —
`[u8]` already has them — so the extension trait only has to supply
`split_ascii_whitespace`, `split_once` and a `splitn` shim. That is a small
trait carrying a very large mechanical diff, which is the opposite shape from
what this entry originally implied ("a self-contained rewrite of the cursor
arithmetic").

**Representation decisions (settled, so the next session need not re-litigate):**
reuse the existing `Path`/`PathBuf` (`kernel/src/fs/path.rs`) for path leaves
rather than inventing a second byte-string type; make the parser helpers an
**extension trait on `[u8]`**, *not* a newtype — a newtype forces wrap/unwrap
noise across all ~1500 sites for no invariant that `[u8]` doesn't already
give; and write it in-house rather than adding `bstr` (the kernel is `no_std`
on a custom target, `bstr` is only ever a host-side transitive dependency
here, and `fs/path.rs` already proves the in-house pattern carries its weight).

**Consequence for sequencing.** A partial conversion is worse than none: if the
editor becomes byte-clean but `execute()` still takes `&str`, the lossy step
just moves from the keyboard to the parser entry, where it is *less* visible.
So this should land as one coherent change over the editor **and** the
statement executors, not as an "editor first, parser later" split. That is the
real reason it is a big task — not the cursor arithmetic.

**Status:** unblocked and in-lane; not started, because the conversion wants a
free build machine to iterate against (a boot test was occupying QEMU during
this scoping pass).

**[A] Update 2026-08-14 — stage (a) has LANDED: `kernel/src/bytestr.rs`.**

The scoping pass above reduced the ~1520 `str`-method call sites to three
missing operations; those now exist. `ByteStrExt` is an extension trait on
`[u8]` supplying `split_ascii_whitespace` (552 sites), `splitn_byte` (51),
`split_once_byte`/`split_once_str` (15), plus `split_byte`,
`rsplit_once_byte`, `find_str` and `find_byte`. The other 631 sites
(`trim*`, `starts_with`, `ends_with`, `strip_prefix`) need no new code —
`[u8]` already has them.

An extension trait rather than a newtype: a newtype buys no invariant that
`[u8]` lacks (there *is* no invariant — any byte sequence is legal) while
forcing wrap/unwrap noise across all ~1500 sites. `split_ascii_whitespace`
deliberately keeps `str`'s exact spelling so those 552 sites need **no edit
at all** once the buffer type changes; the delimiter-taking ones are suffixed
`_byte`/`_str` so a bare `split_once` cannot silently resolve to a future
inherent slice method, which would win over the trait and change behaviour
with no compile error.

**How the equivalence was verified, and why that mattered.** The module
claims each method matches its `str` counterpart *exactly*. A ~1500-site
mechanical diff is only safe while that holds — a helper differing in one
edge case plants a bug at whichever site hits it, with nothing in the diff to
show for it. Checking that claim against a *reading* of the documentation was
not good enough, because the awkward edges are exactly where a confident
misreading is likely. So `scripts/bytestr-oracle.rs` runs the self-test's
exact cases through real `std::str` on the host and prints what std actually
does. All cases matched, including `splitn(0, …)` → nothing, `"a,".splitn(2)`
→ `["a", ""]`, `"".split(',')` → one empty field, and `find("")` → `Some(0)`.
The oracle lives on the host because the kernel is `no_std` and cannot link
std — which is precisely why the equivalence cannot be asserted in-tree.

`bytestr::self_test()` is wired into the boot battery (the kernel binary sets
`test = false`, so `self_test()` is the only place these run). Boot test
PASSED, and the serial log carries all five subtest lines plus
`bytestr::self_test PASSED` — confirmed present rather than merely
not-failing, since a self-test that never executes is indistinguishable from
one that passes.

**Remaining, unchanged in substance:** stages (b) and (c) still must land
together per the paragraph above. Stage (b) is `line_buf` (kshell.rs:2872),
`History.entries` (2631) and four functions (`replace_line` 2921,
`redraw_from_cursor` 2955, `reverse_search_mode` 3061, `read_line` 3191).
Note the landmine at kshell.rs:3583: `buf.insert(cursor, ch as char)` takes a
*byte* index, so widening the `ch >= 0x20 && ch < 0x7F` guard at 3580 to
accept 0x80..=0xFF **without** changing the buffer type would encode each such
byte as two UTF-8 bytes while `cursor += 1` advances by one, desynchronising
the cursor from the buffer. The type change and the guard widening must be in
the same commit.

**[A] Correction 4 (2026-08-14) — this entry's scope estimate is ~40× low, and
stages (b)+(c) are now gated on operator decision `open-questions.md` Q45.**

The "~1520 call sites" figure counted *method calls*. Measured against the
file: `kshell.rs` is **84,845 lines** and **879 of its 1,024 functions** take
or return `&str`/`String`. `execute_single` is a full bash-like parser (alias
expansion, array syntax, `(( ))` arithmetic, `eval`, pipes, redirects,
heredocs) whose logic is text-oriented throughout. So "one coherent change
over the editor **and** the statement executors" means rewriting essentially
the whole shell in a single unreviewable commit, against a shell that
currently works — for a defect this entry itself classifies as *not* data loss.

Also measured, and cutting the other way: only **6** `from_utf8_lossy` sites
exist in the whole file, and all six are file-*content* formatting (`column`,
`diff`), not path handling. The byte-purity problem is confined to the path
pipeline, which is a small part of those 879 signatures.

That suggests a decomposition this entry did not consider: make the *expanded
word* byte-clean rather than the command line — convert word expansion,
`resolve_path`, completion and the path-consuming commands, and let the user
reach arbitrary bytes through the `$'\xff'` escape the shell **already parses**
(7 sites). That is how bash itself works (source is text, expanded argument is
a byte string), it fixes the user-visible bug, and it does *not* fall to this
entry's "partial conversion is worse than none" objection — that objection
targets a **layer** split (editor byte-clean, parser not), whereas this
converts one **data path** end-to-end and so introduces no lossy step.

A-vs-B is an architectural fork on a large, costly-to-reverse change, and B
knowingly departs from the plan written above, so it is the operator's call:
see `open-questions.md` **Q45**. Stage (a) is independently useful either way.

**[A] 2026-08-24 — Q45 is answered (B), and Correction 4's premise for B is
wrong. Read this before planning stages (b)/(c).**

*Q45 no longer gates this.* The operator resolved it **2026-08-21 as option
B, the expanded word** (`design-decisions.md` §261): one data path —
keystroke to syscall — goes byte-clean end to end, while the source line
stays text, as in bash. The paragraph above has read "the operator's call"
for three days after the call was made, which is how an unblocked task keeps
looking blocked.

*But B's stated escape hatch does not exist.* Correction 4 justified B partly
on letting the user reach arbitrary bytes "through the `$'\xff'` escape the
shell **already parses** (7 sites)". Checked: **kshell has no ANSI-C quoting
at all.** Those 7 sites are Rust byte literals — `b'$'` and `push('$')` in the
expansion code — matched by a grep for `$'`, which is a substring of `b'$'`.
The file contains no `$'…'` parser, no hex-escape decoder, and no
`is_ascii_hexdigit`/`from_str_radix` pair anywhere near word expansion. This
also means **Correction 2 was right** and Correction 4 silently contradicted
it; two corrections in the same entry, written the same day, disagree, and
the later and wronger one is the one the chosen option leans on.

Consequence for scope, which is real but bounded: the escape has to be
**built**, and it needs somewhere to put a non-UTF-8 byte once parsed. The
only escape decoder in the file today is `interpret_echo_escapes`, which
serves `echo -e` alone, has no `\xNN` case, and returns a `String` — so it is
a model for the syntax, not a component to reuse. The output sink is the
binding constraint: `shell_write` takes `&str` and `SHELL_OUTPUT` is a
`String`, so command-substitution capture must go byte-clean as part of (b),
not after it.

*Found while checking the above, fixed separately:* `interpret_echo_escapes`
corrupted **every non-ASCII character** — it walked bytes into a `String` via
`bytes[i] as char`, doubling each byte of a multi-byte sequence, so a sourced
`echo -e "café"` printed `cafÃ©`. Written up under *Fixed Bugs* as
`B-KSHELL-ECHO-E-MANGLES-NON-ASCII` (`abeaca2aa`). It is the same `as char`
landmine this entry already documents at `kshell.rs:3583` for the input
guard — the entry flagged the one on the *input* path and missed the
identical one on the *output* path, which was already live.

**[A] 2026-08-24 — stage (b), the path pipeline, has LANDED (`961a160a3`).**

`resolve_path` is now `fn resolve_path<P: AsRef<Path>>(P) -> PathBuf`, `CWD`
is a `Mutex<PathBuf>`, and `TermSession.cwd` is a `PathBuf` — so a 0xFF byte
survives `cd`, resolution and session switch intact. `fs::bench` (7
signatures) and the five filesystem `mount` pass-throughs went with it.

*The ~270-call-site figure this entry has quoted since 2026-08-13 never had
to be paid.* `fs/path.rs` already implements `AsRef<Path>` for `str`,
`String`, `[u8]`, `Vec<u8>` and `PathBuf`, so making `resolve_path` generic
left every forwarding caller compiling untouched. The compiler enumerated the
real cost: **387 errors**, of which **289 (75%)** were the single mechanical
`PathBuf: !Display` case and only ~98 carried judgment. Driving the edits
from rustc's own JSON byte spans (rather than a regex) put each one exactly
on the expression rustc objected to.

**The finding worth carrying forward, because it is a silent-corruption class
the type system does not catch:** `format!("{}.gz", input.display())`
type-checks *as a path*, because `String: AsRef<Path>`. It also writes a
U+FFFD-mangled filename to disk. My own mechanical `.display()` pass
introduced ten of these (five compressors, five decompressor `.out`
fallbacks); the compiler caught them only incidentally, because the sibling
`if` branch happened to be a `PathBuf`. Had both branches been `String`, they
would have compiled clean and corrupted filenames silently. They are now
`path_with_suffix`/`decompressed_output_path`, appending via `extend_bytes` —
not `push`, which inserts a separator. **Anyone extending this conversion
should grep for `display()` inside path construction, not trust the types.**

Also fixed in passing: `extension_hint` used `rsplit_once('.')`, which
searches the whole path, so `archive.tar/notes` reported extension
`tar/notes`. Now `Path::extension`, which stops at the last component.

*Verification:* `self_test` sections 4 and 5 assert 20 normalisation cases
(`.`, `..`, root escape, repeated/trailing separators, cwd-relative) and 3
that drive 0xFF through `resolve_path` — the case a `String` return type
could not represent at all.

**Still open, and deliberately not narrowed:** `fs::pipe` and `fs::templates`
store paths as `String` internally, and the environment is a `String` map.
Rather than paper over these with `display()` — which would hand the callee a
*different path than the user typed* — `path_arg_as_str` refuses loudly at
`mkfifo` and `template`, and is grep-able as the marker for that follow-up.
`$PWD` stays lossy on purpose and says so in-code: resolution goes through
`CWD`, never `$PWD`, so a U+FFFD there cannot redirect a file operation.

**Remaining for (b)/(c):** word expansion → `Vec<u8>`; completion → `PathBuf`
candidates; the `\xNN` escape (still must be *built* — Correction 2 above is
the right one); and the output sink (`shell_write` takes `&str`,
`SHELL_OUTPUT` is a `String`, `capture_command` returns `String`), which per
the paragraph above must land *with* (b), not after. The editor buffer
(`line_buf`, `History.entries`) and the 3580 input guard are unchanged — and
the 3583 `ch as char` landmine is still live, so that guard must not be
widened before the buffer type changes.

**[A] 2026-08-24 — the output sink has LANDED, and it uncovered a live
data-destruction bug on the way.** The sink was the binding constraint named
above; it is now bytes end to end:

| Was | Is |
|---|---|
| `SHELL_OUTPUT: Mutex<Option<String>>` | `Mutex<Option<Vec<u8>>>` |
| `capture_command(&str) -> String` | `-> Vec<u8>` |
| `shell_write(&str)` only | `shell_write_bytes(&[u8])` is the primitive; `shell_write` wraps it |
| `console::write_str(&str)` only | `console::write_bytes(&[u8])` is the primitive |
| `serial::_print` only | `serial::_print_bytes` alongside it |
| `dispatch_with_input(&str, &str)` | `(&str, &[u8])` |
| four hand-rolled copies of the `>>` read-modify-write | one `redirect_write` |

Three notes on *how*, because each was a place the obvious move was wrong:

1. **The console never needed `&str`.** `write_str` looped `putchar(byte)` —
   it was always byte-oriented, and the `&str` was a restriction on *callers*,
   not a property the renderer had. So `write_bytes` is the primitive and
   `write_str` is the two-line wrapper, not the other way round. A byte that
   is not valid UTF-8 renders as whatever glyph the font maps it to, which is
   the honest outcome for a glyph grid.
2. **The serial deadlock-escape protocol was NOT duplicated.** `_print_bytes`
   needed the same per-CPU `IN_PRINT` claim-before-lock dance as `_print` (a
   nested print from an exception must take the unlocked `emergency()` port).
   A second copy that drifted from the first would deadlock only under a
   nested exception — the hardest case to reproduce and the most urgent to
   survive — so the protocol was extracted into one `with_serial(emit)` that
   both call. `serial::reentrancy_self_test` covers it.
3. **`lf_to_crlf` (ssh *and* telnet) went byte-wise.** Iterating bytes instead
   of `chars()` is behaviour-identical for valid UTF-8 — only `\n` and `\r`
   are special-cased and both are ASCII, so neither can appear as a UTF-8
   continuation byte — and it is the only form that can carry a non-UTF-8
   filename onto the wire now that the capture buffer is byte-clean. In
   passing: ssh's version *does* double-convert a pre-existing CRLF while its
   comment claimed it did not; telnet's (which tracks `prev_cr`) does not. The
   divergence is pre-existing and harmless — kshell emits bare LF — but it is
   now asserted in both self-tests, so a future change to either is noticed
   rather than discovered.

**Where the line was drawn, and why it is a named refusal rather than a silent
lossy decode.** 19 `cmd_*_input` implementations still take `&str`, and
several (`sed`, `awk`, `tr`, `cut`, `fold`) are genuine `char`-oriented text
interpreters — converting them is the largest remaining chunk and does not
belong in this commit. Rather than scatter 19 `from_utf8_lossy` calls, there
is one documented chokepoint, `shell_bytes_as_str`, mirroring the
`path_arg_as_str` precedent: it **refuses** and exits non-zero instead of
substituting U+FFFD. That distinction matters most for the consumers that
*write* rather than print — `tee` writes its input to a file, so a lossy
decode there is the same data-destruction class as the `>>` bug below,
arriving through a different door. Behaviour is unchanged today (no producer
emits non-UTF-8 yet); it is the marker for the follow-up and the guard against
the first byte-clean producer corrupting these stages. Follow-up: convert the
byte-safe members — `head`, `tail`, `tac`, `tee`, `wc`, `paste`, `mapfile`,
`nl`, `rev` — to `&[u8]`, leaving only the real text interpreters behind the
chokepoint.

Also removed in passing: `execute_input_redirect` used to reject a non-UTF-8
file with `"{path}: not a text file"` *before* dispatch. That gate now belongs
to `dispatch_with_input`, which knows which command is about to receive the
bytes — deciding at the read would keep a byte-safe consumer from ever seeing
a file it could have handled.

*Verification:* `kshell::self_test` sections 6 and 7 (real VFS writes for the
append fix; `trim_trailing_newlines` and `shell_bytes_as_str` over 0xFF).
Clippy diffed before/after across all five touched files: **no new warnings,
two fewer** (`self_test` now genuinely uses `?`, and one `+` became
`saturating_add`).

**Still remaining for (b)/(c)** — unchanged by this: word expansion
(`expand_vars -> String`, 24 callers) → `Vec<u8>`; completion → `PathBuf`
candidates; the `\xNN` escape (still must be built); the editor buffer
(`line_buf`, `History.entries`) and the 3580 input guard, with the 3583
`ch as char` landmine still live.

**[A] Correction 5 (2026-09-02) — the "still remaining" list directly above
names work that the chosen option does not want done. Read this before picking
the task up.**

That list predates nothing — it was written after §261 was decided — but it
still carries two items from the *rejected* option A, and one of them is the
scariest-sounding thing in the whole entry, so it has been deterring the wrong
reader. `design-decisions.md` §261 says, in the operator's own decision:

> A raw byte still cannot be *typed* literally — you write `$'\xff'`, exactly
> as in bash […] Completion emits the `$'\xff'` spelling for candidates that
> are not valid UTF-8.

**So under the option that was actually chosen, `line_buf` and
`History.entries` stay `String`, and the 3580 input guard is never widened.**
Every byte the user needs to reach is spelled in ASCII on the command line and
becomes a raw byte during *expansion*. That is the whole content of the
"data-flow split, not a layer split" argument §261 is built on.

Two consequences, and both make the job smaller than this entry reads:

- **The 3583 `ch as char` landmine is not on the path.** It is armed only by
  widening the 3580 guard, which option B never does. It remains a real trap
  and the warning above should stay — but it is a warning to anyone who
  wanders back toward option A, not a task.
- **The editor is not touched at all.** Which means the "must land as one
  coherent change over the editor **and** the statement executors" sequencing
  constraint, quoted from Correction 3, does not bind here either — there is
  no editor half to keep in step.

**What is genuinely left, measured 2026-09-02 against the file:**

| piece | where | state |
|---|---|---|
| `$'…'` ANSI-C quoting with `\xNN` | word expansion | **must be built** — Correction 2 is the right one; kshell has no `$'…'` parser (Correction 4's "already parses, 7 sites" counted `b'$'` Rust literals) |
| the byte accumulator it feeds | `expand_vars_bytes`, `kshell.rs:881` | **already exists**, and already copies uninterpreted bytes through verbatim |
| the narrowing seam to delete | `expand_vars`, `kshell.rs:1117` | **8 production callers** (1569, 2396, 2769, 2994, 3152, 3233, 3300, 6051); the ~30 other matches are `self_test` assertions. The "24 callers" figure above is stale, and the wrapper's own doc comment already nominates itself as the seam |
| completion stops dropping candidates | `tab_complete`, `kshell.rs:5922`; the drop is the `.filter_map(\|e\| e.name.to_str()…)` at 5982 | re-spell as `$'\xff'` rather than skip. Note the inserted text stays ASCII, which is exactly why the editor needs no change |
| `resolve_path`, `CWD`, path-consuming commands | — | **already byte-clean** since `961a160a3` |

The one ordering constraint that *does* still bind: the escape parser and the
completion re-spelling must agree on the spelling, since completion's output is
fed back through the parser on the next keystroke. A round-trip assertion over
a 0xFF name (complete → re-expand → compare bytes) is the test that pins it,
and it is the one case a self-test in this area must have.

**[A] Correction 6 (2026-09-02) — auditing the `$'…'` path stage by stage
found a live bug that has nothing to do with bytes, and settled where the
decode can and cannot go. The table above is right about *what* is left and
optimistic about the size of one row.**

*The live bug, which is independent of all of this and fixable on its own.*
`expand_vars_bytes` (`kshell.rs:881`) has no `$'` arm, so `$'…'` falls into
the `$`-followed-by-anything-else arm at ~1051, which emits the `$` and the
`'` and advances **one** byte — without setting `in_single_quote`. The
construct's own **closing** `'` is therefore read as an *opening* quote, and
every word after it on the line is treated as single-quoted until the next
`'` appears. So today:

```
echo $'\x41' $HOME      # prints:  $'\x41' $HOME
```

— the `$HOME` is not expanded, and neither is anything else for the rest of
the line. This is a quote-state desynchronisation, not a missing feature: it
would be a bug even if `$'…'` were never going to mean anything. The minimal
fix is a copy-through arm that consumes the whole construct verbatim
(honouring `\'`), which leaves the text unchanged but keeps the quote state
honest, and is the natural place for the decode to be added later.

*Where the decode can go.* The tempting shortcut is to decode at
`resolve_path` (`kshell.rs:675`) — one site, already generic over
`AsRef<Path>`, already returning `PathBuf`, and every one of its ~257 callers
would get it for free with no signature churn. **It is wrong, and the reason
is the escape hatch.** `expand_vars_bytes` skips its whole `$` branch inside
single quotes, so single-quoting is how a user says "I mean those characters
literally" — exactly as in bash. But `remove_quotes` (`kshell.rs:1424`) then
strips the quotes, so by the time the word reaches `resolve_path` the quoted
and unquoted spellings are *the same bytes*. Decoding there would make a file
whose name literally contains `$'…'` unreachable, with no way to spell it.
Late decoding conflates data with syntax; `$'…'` is a quoting construct and
has to be removed by the parser that recognises quoting, which is
`expand_vars_bytes` and only that.

*Which makes the "8 production callers" row optimistic about one of them.*
Seven are cheap. Site **6051** is not: it narrows to a `String` because it
feeds `execute_single`, a full bash-like `&str` parser (alias expansion,
arrays, `(( ))`, `eval`, pipes, redirects, heredocs), and a `String` cannot
hold the 0xFF that `$'\xff'` exists to produce. So the decode is *gated on*
converting `execute_single` to bytes — that conversion is the task, not a
prerequisite to be noted and skipped. (Three of the other seven — 1569, 3233,
3300 — feed numeric/arithmetic parsers that genuinely want a `String`;
converting those would add `from_utf8` noise for nothing and they should be
left alone.)

*Two more stages need work once bytes flow, both found by reading rather than
by reasoning from names:* `remove_quotes` (1424) and `split_words` (3338)
both strip quotes with no backslash or `$'` awareness, so each needs to pass
the construct through verbatim. `expand_braces` (1273) and
`split_chain_operators` (6134) are already correct — both toggle their quote
state symmetrically on `'` — and need no edit.

**[A] 2026-08-24 — the follow-up landed too: the line-oriented commands are
byte-clean end to end** (`b3c828edb`, `eb8f4109d`). The sink could carry bytes
that no *command* could produce; that gap is closed for the commands where
byte handling is unambiguously correct. Converted, on **both** their pipe and
their file paths (which had drifted apart from each other): `sort`, `uniq`,
`head`, `tail`, `wc`, `nl`, `rev`, `tac`, `tee`, `paste`, and bare `cat`.

The enabling piece is `bytestr::lines()`, matching `str::lines` exactly. It
had to be *written* rather than substituted with `split_byte(b'\n')`, which
differs in three ways that each add or drop a line in a real pipeline:

| | `str::lines` / `bytestr::lines` | `split_byte(b'\n')` |
|---|---|---|
| `"a\n"` | one line | two (`"a"`, `""`) |
| `""` | no lines | one empty field |
| `"a\r\n"` | `"a"` (CR stripped) | `"a\r"` |

The first is the one that matters: shell output almost always ends in `\n`,
so getting it wrong appends a blank line to *every* `head`/`tail`/`nl`.

**Four silent wrong answers found while converting** — each printed
plausible-looking output and exited **0**, which is why none had been noticed:

| Command(s) | Was | Effect |
|---|---|---|
| `head`, `tail`, `nl`, `rev` (file path) | `from_utf8(&data).unwrap_or("<binary>")` | printed one line reading `<binary>` — a well-formed answer unrelated to the file |
| `tac` (file path) | `from_utf8(&data).unwrap_or("")` | printed **nothing at all** |
| `paste` (both paths) | `from_utf8(&data).unwrap_or("")` | undecodable file became an *empty column*; output kept its shape, so blank cells read as "that file had nothing there" |
| `sort`, `uniq` (file path) | raw arg passed to the VFS, no `resolve_path` | the only two file commands missing cwd resolution, so `cd /tmp && sort notes.txt` reported the file missing while `cat notes.txt` beside it worked |

**The chokepoint narrowed rather than moved.** `dispatch_with_input` now calls
`shell_bytes_as_str` **per-command at the point of use** instead of once at the
top. Two consequences, both deliberate: a command that ignores its input
entirely is no longer refused for a reason that cannot apply to it; and
converting one more command is now a matter of moving its arm up into the
byte-clean block. Still behind the chokepoint, with the reason recorded on the
dispatch arm itself: `grep`, `cut`, `tr`, `fold` (`char`-oriented — character
classes, field indices, display width), `sed`, `awk` (real text interpreters),
`xargs`, `column` (re-parse into words / measure display width), and
`mapfile`/`readarray` — which is the one blocked on something **other than
itself**: it stores its lines in the shell environment, still a `String` map,
so converting it would move the decode one call deeper rather than remove it.
Two corrections to the follow-up list in the entry above, both found only by
reading the implementations rather than reasoning from the command names:

- It listed **`mapfile`** as byte-safe. It is not — see the environment
  blocker just described. It stays behind the chokepoint.
- The first conversion commit then over-corrected and grouped **`paste`** with
  `mapfile` as environment-blocked. That was also wrong: `paste` only built
  `String`s internally, by its own choice, and nothing downstream of it needs
  text. Hence the second commit.

The lesson worth keeping is the one that also caught `rev` during the first
pass (initially assumed unsafe because reversing a line's bytes would tear a
multi-byte character — but this `rev` reverses the *order of lines*, an alias
of `tac`, which cannot): **classify each command by reading it, never by what
its name implies.** Three of the four guesses made from names were wrong.

*Verification:* `kshell::self_test` section 8 drives the real
`dispatch_with_input` rather than the `cmd_*_input` functions directly,
because the **routing** decision is the part that can regress; it covers each
converted command over 0xFF input, `tee`'s byte-identical file copy, `paste`
against an undecodable column, the file-path halves, the `sort`/`uniq` cwd
fix, and both sides of the routing decision (a text-only command still refuses
and exits 1; `echo` still runs). `bytestr::self_test` section 6 pairs every
`lines` assertion with the `str::lines` call it must agree with. Clippy diffed
before/after by stashing only the two touched files: **93 warnings before, 93
after, identical multisets.**

**[A] 2026-09-02 — stage (b) is three commits, not one, because the shell has
no backslash escape *at all*: `echo \'` silently disables `$VAR` expansion
for the rest of the line.**

**In short:** in a shell, a backslash means "the next character is an ordinary
character, not punctuation" — `echo it\'s ok` should print `it's ok`. kshell
does not implement that anywhere. The backslash is passed through as a literal
character, and the `'` it was supposed to neutralise is read as *opening a
quoted region* that then never closes. Everything after it on the line is
treated as quoted, so `$HOME` and friends stop expanding. Nothing warns; the
line just quietly does something else.

This was found by reading, not by a failure, while scoping stage (b). Three
places implement the same grammar and all three are blind to it:

| Where | Line | What it gets wrong |
|---|---|---|
| `expand_vars_bytes` | 881 | Tracks `in_single_quote` without consulting backslashes, so `\'` flips the state. Also expands `"\$HOME"`, which bash does not. |
| `remove_quotes` | 1469 | `a\'b` → `a\b` (backslash leaks *and* the quote opens). Bash: `a'b`. |
| `split_words` | 3383 | `cp a\ b dst` splits into three words, so a file whose name contains a space becomes two missing files. |

**The state-inversion in `expand_vars_bytes` is the same bug, in the same
function, that the `$'…'` arm at line 1051 was written to fix** — that arm's
comment already spells out the failure mode ("inverting quoting for the entire
rest of the line: `$'a' $HOME` stopped expanding `$HOME`"). `$'…'` was one of
two constructs that can put an unbalanced-looking `'` in the byte stream. `\'`
is the other, and it was missed.

**Layering, so the fix does not double-unescape.** Expansion *preserves*
quoting and dispatch *removes* it — that separation already exists and is why
`expand_braces` can see `'b,c'` as quoted. The backslash must follow the same
rule: `expand_vars_bytes` copies the `\` and the byte it protects through
unchanged (expanding nothing in between, and not letting the protected byte
change quote state), and the escape is honoured exactly once, later, at the
quote-removal stage. Verified 2026-09-02 that `expand_vars_bytes` consumes a
backslash *only* inside `$'…'` (line 1078, which copies the pair verbatim by
design), so adding the pass-through does not double-process.

**Sequenced as three commits**, because they are three separable behaviours
and the third can change argument counts:

1. `expand_vars_bytes`: a backslash protects the next byte from expansion and
   from quote-state tracking; both bytes are copied through.
2. `remove_quotes` and `split_words`: honour the escape, via one shared helper
   so the two cannot drift. The three contexts have different rules and all
   three matter — unquoted `\c` → `c`; inside `"…"` a backslash is special
   only before `"`, `` ` ``, `$`, `\`, so `"C:\dir"` must keep its backslash;
   inside `'…'` there are no escapes at all.
3. `split_words`: an explicitly quoted empty string is a word. `cmd ''`
   currently passes *no* argument, because the accumulator is empty and empty
   accumulators are dropped. This is last because it changes arity at 15 call
   sites, two of which (`kshell.rs:139412`, `:140259`) branch on
   `split_words(args).is_empty()`.

**A false invariant found in passing, to fix with (2).** The comment at
`kshell.rs:7159` justifies `dispatch_with_input`'s fallback arm with "that is
a no-op, since a dequoted string has no quotes left to remove". That is not
true — the suite's own `remove_quotes("\"it's\"") == "it's"` at line 10926
leaves a quote in the result, and dequoting *that* again yields `its`. The
code is nevertheless correct, for a different reason: the fallback passes
`line`, the original, not `args`. The comment should say so, because someone
who believes the stated reason could "simplify" it into a real bug.

**[A] 2026-09-02 — why (b) has to come first: without an escape, correct tab
completion is not *expressible*, never mind unimplemented.**

Pressing Tab on a file whose name contains a space produces a command line
that does not refer to that file. `tab_complete` (kshell.rs:5967) inserts the
matched name raw — line 6037 for the unique match, 6050 for the common prefix
— with no escaping anywhere in the function. Completing `My Doc.txt` yields

```
cat My Doc.txt
```

which the very next stage splits into two arguments, so the command reports
two missing files, both named after halves of a file that exists. The user did
not type that line; the shell wrote it for them.

**There is no way to fix this at the completion site.** The correct output is
`cat My\ Doc.txt`, and today that is *worse* than the broken version:
`remove_quotes` leaves the backslash in, so the argument becomes
`My\ Doc.txt` and the file is still not found. Quoting it as `'My Doc.txt'`
fails differently — completion would have to know whether the cursor is
already inside a quoted region, and it cannot, because `word_start` at line
6001 is a bare `text_before.rfind(' ')` with no quote or backslash awareness.
Completing inside `cat "My D<TAB>` therefore starts the word at `D`.

This is the argument for the sequencing, stated in terms of what is possible
rather than what is convenient: **stage (b) is not a prerequisite of (d) by
scheduling preference, it is a prerequisite by grammar.** Emitting an escape
is only correct once something honours it; until then every candidate spelling
of a completed name containing a space is wrong in a different way.

Two consequences for the eventual (d):

- Completion must **escape what it inserts**, using the rules (b) adds, and
  the round-trip is the assertion worth writing: for any directory entry,
  parsing what completion inserted must yield the original name back. One
  property covers both the space case and the 0xFF case.
- Completion must find the **word start** with the same scanner the parser
  uses, not `rfind(' ')`. Two implementations of "where does this word begin"
  is how the cursor ends up inside a quoted region the parser thinks it left.

The 0xFF case (line 6027, which drops any candidate whose name is not UTF-8)
is unchanged and still waits on (c) for the byte-clean word path — but it is
now clearly the *second* reason completion is wrong, not the first.

**[A] Correction 7 (2026-09-04) — (c) is named after the wrong function, and
the name has been making it look ~10× bigger than it is. Read this before
planning (c).**

**In short:** the entry says stage (c) is "convert `execute_single` to bytes",
and describes `execute_single` as a full bash-like parser — which it is, and
which is why nobody has started. But converting *that function* would not fix
anything, because it is not where the byte dies. Measured against the file
today, the byte dies one call further down, at a function that is 20 lines
long and already has a proven conversion mechanism sitting next to it.

*Where the byte actually dies.* `execute_single` (6408) does no text work of
its own — it is the ERR-trap wrapper around `execute_single_inner` (6438).
The line reaches `dispatch` (7271), whose whole relevant body is:

```rust
let mut parts = line.splitn(2, ' ');
let cmd = parts.next().unwrap_or("");
let raw_args = parts.next().unwrap_or("").trim();
let dequoted = if command_parses_own_quotes(cmd) { None }
               else { Some(remove_quotes(raw_args)) };
let args = dequoted.as_deref().unwrap_or(raw_args);
```

`remove_quotes` returns a `String`, and `args: &str` is then handed to **651**
`cmd_*(args: &str)` functions. So if `execute_single`'s parameter became
`&[u8]` and nothing else changed, `dispatch` would simply `from_utf8` at its
top: the narrowing seam moves from site 6051 to line 7271, the diff is large,
and **no user-visible behaviour changes at all.** That is the same failure
shape this file documents everywhere else — work that runs, passes, and is
about the wrong thing.

*Why the real seam is much cheaper than the entry implies.* Three facts, each
checked against the file rather than inferred:

| fact | consequence |
|---|---|
| `resolve_path` is `<P: AsRef<Path>>` (699) and `fs/path.rs:287` has `impl AsRef<Path> for [u8]` | the 15 commands written `resolve_path(args)` accept `&[u8]` with **no edit** |
| `shell_bytes_as_str` (7084) already exists, with 6 call sites | the per-arm refusal chokepoint `dispatch_with_input` uses is directly reusable for `dispatch` |
| `command_parses_own_quotes` already exists as an opt-in list | commands migrate one at a time, exactly as `TD-KSHELL-COMMANDS-TAKE-A-FLAT-STRING-NOT-ARGV` describes |

So the shape of (c) is: give `dispatch` a `&[u8]` line and `&[u8]` args; let
the path-consuming arms take them unchanged; put `shell_bytes_as_str` on the
arms that are genuine text interpreters (`sed`, `awk`, `tr`, `cut`, `fold`,
`grep`, `column`, `xargs` — the same set already behind the chokepoint on the
*input* side, which is not a coincidence). `execute_single` and its sibling
statement executors keep taking `&str` for as long as they like: a shell's
*source line* being text is what §261 chose, and only the *expanded word* has
to carry bytes.

*What this does not change.* The ordering constraint from Correction 5 still
binds — the `$'…'` parser and completion's re-spelling must agree, and the
round-trip assertion is still the test that pins it. And the quote-state
desynchronisation bug Correction 6 found in `expand_vars_bytes` (`echo $'\x41'
$HOME` stops expanding for the rest of the line) is still live, still
independent of all of this, and is still the cheapest thing here to fix.

*Why the misnaming happened, since it is the reusable lesson.* Correction 6
traced the decode's dependency to "site 6051 feeds `execute_single`" and
stopped at the first function it reached that looked expensive. It named the
*caller* rather than following the value to where it is actually narrowed. A
dependency argument has to be walked to the point of loss, not to the first
alarming name on the path — otherwise the write-up inherits the cost of a
function that was never on the critical path, and the task sits untouched for
the reason the write-up invented.

**[A] 2026-09-04 — (d)'s three escaping rules are now checked against real
bash, 59/60, and the one disagreement is a *different* bug (see
`A-KSHELL-TAB-COMPLETION-LOOKS-UP-THE-UNEXPANDED-WORD` below).**

Before writing `quote_suffix`, its rules were put to real bash the way this
project's gates do it — the round-trip property, over 20 filenames × the three
contexts:

> what bash parses out of `<opener><already-typed><quote_suffix(rest, ctx)><closer>`
> is the original filename, byte for byte, and is **one** word.

| `Ctx` | rule | result |
|---|---|---|
| `Single` | pass bytes through; `'` becomes `'\''` | 20/20 |
| `Double` | backslash-escape exactly `"` `\` `$` `` ` `` (`DQ_ESCAPABLE` minus `\n`, which a completion cannot insert) | 19/20 |
| `Unquoted` | wrap the whole suffix in `'…'` if any byte is special; adjacent quoting concatenates, so `My` + `' Doc.txt'` stays one word | 20/20 |

Names covered include a space, an apostrophe, a double quote, a backslash, a
`$`, a backtick, `* ; & | < > ( ) ~ # ! { } [ ] ,` and an embedded newline.

*Two methodology notes, because the first draft of this oracle got both wrong
and would have "passed" while grading nothing.* It ran `bash -c` with the
script as an **argv element**, and the Windows argv round trip ate the
backslashes — in a check whose subject is backslashes. That is precisely the
failure `scripts/check-shellquote-vs-bash.py` documents having been burned by,
and `scripts/bashprobe.py` exists to prevent; using it fixed five of the seven
apparent mismatches. The remaining two were a `str`-vs-`bytes` comparison that
reported *every* case as a mismatch — which is the harmless direction, but only
by luck.

**[A] 2026-09-05 — ✅ stage (d) is DONE. Completion escapes what it inserts,
and the escaping is pinned by real bash.** ((a), (b′) and (c) remain open; this
closes only (d).)

**In short:** pressing Tab on a file whose name contains a space, an
apostrophe or a `$` used to produce a command line naming a *different* file —
or two files that do not exist. Completion now inserts a spelling that parses
back to the name it matched, in whichever quoting region the cursor happens to
be in.

*What was added.* `shellquote::quote_suffix(suffix, ctx)`, plus
`quote_suffix_str` for the line editor's `String` buffer. The `_str` form is a
one-line wrapper over the byte form and **not** a second implementation: two
spellings of one escaping rule is the disease this module exists to end, and a
`char`-oriented copy would drift from the byte-oriented original the first time
one of them was corrected. It returns `Option` rather than asserting — the
conversion cannot fail for a `&str` input, since every byte the escaping adds
is ASCII, but a kernel that panics on a Tab keypress is worse than one that
declines to complete.

*Why a suffix function rather than the existing `quote_word`.* Completion never
gets to write the whole word. The user has already typed part of it, possibly
inside a quote they opened, and the only edit completion may make is an
insertion at the cursor — so the bytes emitted have to be correct *inside the
region already open*, which is a different problem in each context and in none
of them is the answer "wrap it in single quotes". The rules are the three from
the 2026-09-04 note above, unchanged: `Ctx::Single` passes bytes through and
spells `'` as `'\''`; `Ctx::Double` backslash-escapes `"` `\` `$` `` ` ``;
`Ctx::Unquoted` wraps in `'…'` if any byte is special, which stays one word
because adjacent quoting concatenates (`My` + `' Doc.txt'`).

*The one rule that looks like an omission and is not.* `\n` is in
`DQ_ESCAPABLE` but is deliberately **not** escaped in the double-quoted case:
inside `"…"`, `\<newline>` is a line continuation, so escaping a newline would
*delete* it. A raw newline is already literal there. The case is in the table
below with that reason written next to it, because it is exactly the kind of
"missing" entry a later reader would helpfully add.

*Where it is wired.* `kshell::tab_complete`, both insertion sites — the unique
match and the common prefix. The context is computed once, from
`trailing_context` at the cursor, and an unterminated quote is the normal case
there rather than an error: the user is mid-word by definition. The unique
match still closes the quote before the separating space; the common prefix
deliberately does not, because the word is not finished.

*What pins it, in four layers.* Each catches something the others cannot:

| layer | asserts | catches |
|---|---|---|
| `shellquote::self_test` §8 | the literal output spelling for each rule | a rule quietly changing |
| `shellquote::self_test` §9 | round trip over 10 names × every split point × 3 contexts | a rule that is right on its own example and wrong one byte over |
| `kshell::self_test` rung 119 | the real `tab_complete`, against real VFS entries, unquoted with the dispatcher's own `remove_quotes` | the two being individually right and *not wired together* |
| `scripts/check-kshell-rungs-vs-bash.py` `SUFFIX_CASES` | what **real bash** parses out of the composed line | all of the above agreeing with each other and with nothing outside |

The bash leg is the one worth explaining. §8 and §9 both grade `quote_suffix`
with *our* scanner, so a wrong-but-self-consistent escaping satisfies both.
`SUFFIX_CASES` transcribes §8's output literals — read out of `shellquote.rs`
verbatim, so the table cannot drift into fiction — composes
`<opener><typed><inserted><closer>`, and asks bash what that means. All five
agree. Note it is the *output* spelling that is pinned to the Rust, not the
input: pinning the input would leave the kernel free to assert any output at
all while the checker measured a string it had invented itself.

*Rung 116 still passes unchanged*, which is the useful control: it covers where
the word *begins*, and none of its five expectations move when what is
*inserted* becomes escaped. Three independent defects in one function, and the
fix for each leaves the other two's evidence intact.

> **Both rung citations above were dangling until 2026-09-05, and the evidence
> trail now reads as though they never were.** Rungs 116–119 ran and passed
> from the day they were written, but announced themselves with a `// --- N:`
> *comment* instead of a `serial_println!`, so they printed nothing. Grepping a
> boot log for "rung 119" — the route this table tells you to take — found
> nothing, for either of the two rungs cited on this page. The banners were
> added in the same batch that added this note, so the citations are true now;
> they were not when they were written. `scripts/check-selftest-rung-numbers.py`
> gained a rule that fails the build on any rung citation naming a rung no
> banner announces, so this cannot recur silently. It found five citations
> across `kshell.rs` and this file, covering exactly rungs 116–119.

*Still not fixed here:* completion looks the word up **unexpanded** — see
`A-KSHELL-TAB-COMPLETION-LOOKS-UP-THE-UNEXPANDED-WORD` immediately below. §9's
round trip deliberately does not model expansion (`strip_quotes` does not
expand and a real shell does), and says so in place, so that gap is not
silently absorbed into a property that would then read as covering it.
