### TD-OILS-A-DUP-WORD-THAT-NAMES-NO-DESCRIPTOR-TOOK-THE-WRONG-BRANCH-FOUR-WAYS — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — the `DupOut`/`DupIn` arms of
`Shell::resolve_one_redirect` and `Shell::apply_persistent_redirect`.

`[N]>&WORD` and `[N]<&WORD` are the only redirects whose *instruction* is
decided after the expansion. bash's ladder (`do_redirection_internal`,
`redir.c` ~795–870) is narrower than "a number dups, anything else is a
filename", and osh got four rungs of it wrong. Found while writing the corpus
case for the extra `cannot duplicate fd` line.

| # | bash | osh was | fix |
|---|---|---|---|
| 1 | the word expanded to **no word** (unset-and-unquoted, or split into several): a *move* becomes a close of its own redirector, everything else is `ambiguous redirect` | `v=; true 3>&$v-` said `ambiguous redirect`; bash is silent, rc=0 | `Shell::expand_redirect_word_once` hands the ambiguity back as `None` instead of as an error, and the dup arms read it as a close |
| 4 | `>&WORD` becomes a file **only** on fd 1, **only** as a plain dup — never a move, never a `{v}` | `true 1>&nope-` created the file `nope`; bash says `ambiguous redirect` | `Shell::dup_word_becomes_file` reproduces all three conditions |
| — | a `{v}` redirect that becomes a close stops being an allocation: bash takes the descriptor out of `$v` | `e=-; true {v}>&$e` allocated fd 10 | `PersistentBind::VarfdClose(fd)` — the *caller* performs the close, because only it knows whether it is `exec`'s (permanent) or a command's (undone at exit) |
| — | `redirection_error` names the **variable** for bash's own negative codes (`AMBIGUOUS_REDIRECT`, `NOCLOBBER_REDIRECT`, `RESTRICTED_REDIRECT`) but the **path** for a real `errno` | `true {v}>&nope` said `nope`; bash says `v` | free fn `varfd_error_subject`, applied by wrappers on both redirect entry points |

A fifth, structural one fell out of the same work: `apply_persistent_redirect`
expanded its filename with `expand_to_string`, so **`exec` never reported
`ambiguous redirect` at all** — `v="a b"; exec > $v` silently created two
files. It now goes through `Shell::expand_redirect_target` like every other
redirect.

Corpus case:
`a-dup-word-that-names-no-descriptor-translates-three-different-ways.sh`.
