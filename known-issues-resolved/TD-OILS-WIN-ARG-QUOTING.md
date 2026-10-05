### TD-OILS-WIN-ARG-QUOTING. An argument containing `"` or `\` is mangled on the way to an external MSYS command — 2026-07-31 — ✅ RESOLVED 2026-08-01

**Where:** `userspace/oils/src/wincmd.rs` (the whole module) and
`userspace/oils/src/interp.rs` — `Shell::push_child_args`, through which
all three spawn sites now hand a child its arguments.
**Windows development host only**; on the SlateOS target `exec` takes an
argv vector and there is no command line to quote.

Windows has no argv: the parent builds one string and the child parses
it. Rust's `Command::arg` uses the MSVC convention (backslash is an
escape before a quote), and MSYS/Cygwin's own parser uses a different one
(backslash is an escape everywhere, `"` is a quote delimiter). So an
argument osh hands to an MSYS binary comes out different from the one it
was given:

```sh
env printf '[%s]\n' 'a"b' 'e\f'
# bash: [a"b]  [e\f]
# osh : [a\b e\f]        — one argument, the quote swallowed the space
printf 'y="3"\n' | sed -E 's/="[0-9]+"/=N/'
# bash: y=N        osh: y="3"   (sed sees s/=\"[0-9]+\"/=N/ and errors on the backref)
printf 'SECONDS=3\n' | sed -E 's/^([A-Z]+)=[0-9]+$/\1=Q/'
# bash: SECONDS=Q  osh: 1=Q     (the \1 lost its backslash)
```

This is invisible for builtins (no command line is built) and for
native-Windows children (same convention), so it only bites the MSYS
toolchain — which is exactly what the differential corpus uses, so it
silently limits what a corpus case can express. Corpus cases work around
it today by spelling no `"` and no `\` in an argument to an external
command.

**The proper fix** is to quote per the callee's convention: detect a
Cygwin/MSYS child (its import table names `msys-2.0.dll`/`cygwin1.dll`)
and build the command line with `CommandExt::raw_arg` using Cygwin's
rules, falling back to Rust's MSVC quoting for everything else. Worth
doing because it is the differential harness's fidelity, but it is host
scaffolding rather than shell semantics, so it is not on the critical
path.

**Measured (2026-08-01).** Cygwin's parser was probed directly, by
handing a real MSYS bash a raw command line from native-Windows Python
(`subprocess.run(<string>)` on `os.name == 'nt'` passes the string
through unaltered) and printing the argv it built. Two findings:

*The mangling is not only quoting: an **unquoted** argument is glob- and
tilde-expanded by the child.* Cygwin's `build_argv` runs the expansions a
Unix shell would have run before `exec`, on the assumption that no shell
did. So `tr '\n' '~'` reaches `tr` as `/c/Users/inhah`, and a bare `*`
reaches it as the whole directory listing — a divergence with no `"` or
`\` anywhere in sight, which is how it was found (a probe's `tr` output
came back with `/` where bash had `~`). Arguments only survive today when
they happen to match nothing (`~x`, `a*b`).

The parse, in full (raw fragment → argv element):

| raw | argv | raw | argv |
|---|---|---|---|
| `plain` | `plain` | `a\\b` | `a\\b` |
| `~` | `/c/Users/inhah` | `"a\\b"` | `a\b` |
| `"~"` | `~` | `"a\"` | `a"` |
| `~x` | `~x` | `"a\\"` | `a\` |
| `*` | *(the cwd listing)* | `q"r` | `qr` |
| `"*"` | `*` | `"q""r"` | `q"r` |
| `a*b` | `a*b` | `"q\"r"` | `q"r` |
| `a\nb` | `a\nb` | `""` | *(empty)* |
| `"a\nb"` | `a\nb` | `a"b c"d` | `ab cd` |

That is: inside `"…"`, `\\`→`\`, `\"`→`"`, `""`→`"`, and everything else
is literal (`\n` stays two characters, unlike the MSVC rule where a
backslash run before a quote halves); the closing `"` ends the section
rather than the argument, and quotes are removed in place. `$`, backtick,
`'`, tab, `%PATH%`, `;|&`, `()<>^` all pass through a quoted section
untouched.

**Derived encoding:** wrap *every* argument in `"…"` — which also
suppresses the glob/tilde pass — and inside it replace `\` with `\\` and
`"` with `\"`. Verified round-trip for `a\b`, `q"r`, `a\`, `~`, `*` and
the empty string. Note it differs from Rust's MSVC quoting on exactly one
point, `\\` inside quotes, which is why the encoder cannot be global: it
has to be chosen per callee.

**Fixed 2026-08-01.** The three inline `cmd.args(…)` sites were collapsed
into `Shell::push_child_args`, and on Windows that helper asks
`wincmd::is_cygwin_program` whether the callee links `cygwin1.dll` or
`msys-2.0.dll` — read from the executable's PE import table, since it is
the *runtime* and not the program that re-parses the command line. When
it does, each argument goes through `wincmd::quote_arg` and is appended
with `CommandExt::raw_arg`; everything else keeps Rust's MSVC quoting.
The lookup is cached by command spelling, so the `$PATH` search behind it
is paid once per distinct command rather than once per spawn, and it goes
through `find_in_path` rather than `resolve_external` so that asking the
question does not disturb the `hash` cache.

The workaround is retired with it: a corpus case may now spell `"` and
`\` in an argument to an external command. Corpus case
`external-args.sh`, which pins the whole surface — quotes, backslashes,
regex backreferences, glob and tilde characters, empty and
space-bearing arguments, tabs/newlines/high-bit bytes, and the argument
*count* — plus the `wincmd` unit tests, which cover the encoder and drive
the import walk over a synthetic PE image and every truncation of it.
