## `TD-OILS-ENABLE-F-AND-D-FOLLOW-THE-NO-DLOPEN-BUILD` (lane B, 2026-08-26) — **open**, `SCOPE: out of frozen scope (§305)`

**In short:** `enable -f file name` asks the shell to load a new builtin out of a
shared library file, and `enable -d name` asks it to unload one again. osh
answers both with a flat refusal. The reference bash we diff against does not
refuse — it genuinely tries, and fails for its own reason (the file isn't there,
or isn't a library, or hasn't got the right symbol in it). So the two shells
print different words and return different numbers for these two options. This
is a difference in what the two programs were *built to be able to do*, not a
mistake in osh, and nothing on SlateOS can hit it.

**What each side does.** Measured against GNU bash 5.2.21 (x86_64-pc-linux-gnu):

| command | bash (has `dlopen`) | osh |
|---|---|---|
| `enable -f /nosuch/lib.so foo` | `cannot open shared object /nosuch/lib.so: …No such file or directory`, rc=1 | `dynamic loading not available`, rc=2 |
| `enable -f libc.so.6 foo` | `cannot find foo_struct in shared object …`, rc=1 | as above |
| `enable -fn x` | `-f` takes the bundled `n` as its filename, rc=1 | as above |
| `enable -d echo` | `echo: not dynamically loaded`, rc=1 | usage line alone, rc=2 |
| `enable -d nosuchname` | `nosuchname: not a shell builtin`, rc=1 | usage line alone, rc=2 |
| `enable -d` (no names) | falls through to the ordinary listing, rc=0 | usage line alone, rc=2 |

osh's column is not invented: it is what bash itself prints when configured
*without* `dlopen`, which is the build MSYS bash was and the build osh
deliberately imitates (`Shell::builtin_enable` in `userspace/oils/src/interp.rs`
says so in its comments, and `enable_refuses_the_dynamic_loading_options` pins
it). The corpus prose used to state that column as though it were bash's
behaviour everywhere; that was an artifact of the reference shell of the day
being MSYS, and is now corrected.

**Why this is not going to be fixed.** §305 froze osh's bash-fidelity scope, and
this divergence fails every one of its three "fix it" tests: nothing we ship or
run calls `enable -f`; it is not a crash, hang, data-loss, security or
propagating-wrong-status bug; and it is not a regression. It lands squarely on
the excluded list instead — diagnostic wording and an exit status for an option
that cannot do anything here. More to the point, **§305's own reasoning depends
on osh not needing a dynamic linker** (decision part 1), and SlateOS has none:
there is no `dlopen` anywhere in the tree and the ELF loader has no `PT_INTERP`
handling. A shared object osh could load does not exist on the target.

**What was done instead.** The five corpus probes whose answers depend on
`dlopen` (`-f FILE`, `-fn x`, `-d echo`, `-nd echo`, `-dn echo`) were removed
from `userspace/oils/tests/corpus/a-enable-lists-and-refuses.sh` rather than the
whole case being waived with `EXPECT-DIFF` — waiving the case would have dropped
regression cover from the ~60 assertions in it that *do* match, including the one
that matters most, that both shells name the same 61 builtins in the same order.
The four probes that survive (`-f` with no argument, `-nf`, `-z`, `-sz`) are
option-parser behaviour, which happens before either letter is acted on and is
identical in both builds.

**If SlateOS ever grows a dynamic linker**, this becomes answerable and should be
revisited — but note that even then osh could not *run* a bash loadable builtin,
because those are C functions taking bash's own `WORD_LIST`, and osh's word list
is Rust. The most it could ever do is reproduce bash's failure messages more
precisely.
