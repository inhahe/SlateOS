### TD-OILS-GREAT-AND-FILE-LOSES-ITS-SPELLING. The parser rewrites `>&file` into `&>file`, so nothing downstream can tell the two apart — 2026-08-03 — ✅ **FIXED 2026-08-03**

**Where:** `userspace/oils/src/parser.rs`, the `was_great_and` rewrite at the end
of the redirection parser; `userspace/oils/src/interp.rs`,
`Shell::resolve_dup_out`'s `DupWord::NotAFd` branch and the
`RedirectOp::WriteBoth` arm of `resolve_redirect_list`.

**What.** bash's parser keeps `>&word` as `r_duplicating_output_word` and only
`do_redirection_internal` converts it to `r_err_and_out`, at redirection time,
when the redirector is fd 1 and the expanded word is not a number. osh takes the
shortcut of deciding at *parse* time: a `>&` with a literal, non-numeric target
and no explicit fd is rewritten to `RedirectOp::WriteBoth`, the same node `&>`
produces. The shortcut gives the right *effect*, but the spelling is gone, and
two things need it.

**Reproduce (1) — `declare -f` prints the wrong operator:**

```
$ osh -c 'f() { echo hi >& out; }; declare -f f'
f ()
{
    echo hi &> out
}
```

bash prints `echo hi >&out` (and note it writes no space after `>&`, where it
writes one after `&>`).

**Reproduce (2) — posix mode globs the two differently.** Per
TD-OILS-POSIX-MODE-BEHAVIOURS-ARE-NOT-IMPLEMENTED, a posix non-interactive shell
globs a *dup* word but not a *filename* one, and `>&` is a dup word:

```
$ bash --norc -c 'set -o posix; cd /tmp/d; echo hi &> g*; echo "&>: $?"
                  echo hi >& g*; echo ">&: $?"'
&>: 0        # created a file literally named g*
g*: ambiguous redirect
>&: 1        # globbed onto two names
```

osh said 0 for both, because both were `WriteBoth`, which
`resolve_redirect_list` classifies as `RedirWord::Filename`.

**A third thing the shortcut hides,** found while probing: the runtime path that
handles the forms the parser *cannot* rewrite (`1>&file`, `>&$v`) does not do the
special-filename check the `WriteBoth` arm does, so `1>& /dev/stderr` fails where
`>& /dev/stderr` works. bash treats them alike.

**Fix.** The parse-time rewrite is gone; the node stays `RedirectOp::DupOut` and
the call is made at redirection time, where `1>&file` and `>&$v` were already
being decided. `WriteBoth`/`AppendBoth` now mean `&>`/`&>>` and nothing else.

Both runtime `NotAFd`/fd-1 branches — `Shell::resolve_dup_out` for the transient
path and `Shell::apply_persistent_dup_out` for `exec` — gained the
special-filename check the `WriteBoth` arms already had, which is what fixes the
third finding above; the persistent one also gained the two `set_std_read_half`
clears, since a `>& file` takes the whole descriptor just as `&> file` does.
`job_holds_sink` gained a matching guard so a `>& file` on fd 1 still counts as
giving up both descriptors — the `&>` arm below it was doing that for the
rewritten node, and without it `x=$(cmd >& f &)` would have started blocking.

The printer needed a model of bash's *three* dup instructions rather than one,
so `ast.rs` grew `DupSpelling` + `dup_spelling()` (the classification is the
parser's, so it never looks through quotes: `>&"2"` is a word, `>&'-'` is a word)
and `unparse.rs` a `dup_src` helper. bash's rules, pinned against all three
forms: a close always prints as an *output* dup with its fd shown (`cat <&-`
prints `cat 0>&-`), a number always shows its fd (`>&2` prints `1>&2`), and only
a dup word may leave the operator's default fd off (`>& out` → `>&out`,
`2>& out` → `2>&out`, `<& in` → `<&in`).

Covered by `a-dup-redirect-keeps-the-spelling-it-was-written-with.sh`, by the
`>&`-versus-`&>` glob asymmetry now included in
`posix-mode-stops-splitting-a-redirection-word-but-still-globs-a-dup.sh`, and by
a lib test in `unparse.rs`.

**Found while fixing, and still open:** `set -C` does not protect a `&> file`
(or a `>& file`) target. bash refuses both with `cannot overwrite existing file`;
osh checks noclobber only in the `Write`/`Clobber`/`Append` arms. See
TD-OILS-NOCLOBBER-IGNORES-THE-AMPERSAND-FORMS.
