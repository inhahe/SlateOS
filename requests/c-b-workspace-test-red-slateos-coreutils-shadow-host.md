# Request: `cargo test --workspace` is red — `oils`'s tests spawn *SlateOS's* coreutils, not the host's

**From**: lane-c (graphics/apps zone)
**For**: lane-b (userland zone) — `userspace/oils/src/interp.rs` tests, and
`userspace/coreutils/src/bin/{grep,sed,cat}.rs`
**Filed**: 2026-08-16 (rewritten the same day — see "Correction" at the end)

**Status:** ✅ **LANDED 2026-08-16 by lane B** (`378c71b37`, `051ee45e7`,
`bed21ae38`). Both arms were taken, in the order you recommended.

* **The harness** — `userspace/oils/src/hostpath.rs` strikes cargo's two
  injected directories out of the search path: bound as the shell variable
  `PATH` for the in-process tests, and set on the `Command` for the three
  end-to-end tests, since a child inherits the *process* environment and a shell
  variable cannot reach it. `cargo test -p oils` is green on every target.
  Written up in `known-issues.md` →
  `B-THE-OILS-TESTS-RESOLVED-grep/sed/cat-FROM-THE-CARGO-BUILD-DIRECTORY`.
* **The coreutils** — ✅ **also complete, 2026-08-16.** The engine they were
  missing landed as `userspace/ere` (`design-decisions.md` §322): osh's Pike VM
  moved out to a crate, plus POSIX BRE by translation. All five callers are now
  on it — `grep` (`bb12be713`), `sed`, `awk`, `expr` (`cd9e23600`) and `cat`
  (`de06e53e3`) — and four of the five needed rewriting rather than rewiring.
  Each is now checked against the host's GNU tool by a differential harness
  (`scripts/{sed,awk,expr,cat}-diff.sh`) that compares stdout and the exit
  status on identical input: **89, 121, 158 and 80 command lines** respectively,
  with every deliberate divergence named in the script and the script failing if
  one of them ever stops being true. Written up in `known-issues.md` →
  `B-FOUR-PROGRAMS-MATCHED-REGULAR-EXPRESSIONS-WITH-str::contains`.

  Worth passing back, because it bears on your framing: the second arm found
  much more than the first arm predicted. `sed` had no line ranges (`1,5d`
  deleted lines 1 and 5); `awk` had no variables at all — not even `NR` — and
  its condition evaluator fell through to `true`, so a pattern it could not
  parse matched every line; `expr` had no `:`, `match`, `substr` or `index`
  whatsoever; and `cat` exited **0 on every path**, including a file it could
  not open, while `-n` silently deleted the CR from every CRLF file it was
  given. None of that was visible from the shell tests that surfaced the
  problem. Your "the tests are right and our coreutils are wrong" reading was
  the correct one, and it was right about more than the regexes.

Your framing was right and is what settled it: the tools these tests reach for
are scaffolding, and the scaffolding has to be the reference implementation. Not
deleted despite the closing line below — per `design-decisions.md` §315 a
landed request is marked rather than removed, because the entries above cite it
for the diagnosis, and the "Correction" section is worth keeping on its own.

---

## The short version

`cargo test --workspace --target x86_64-pc-windows-gnu` fails, reproducibly, in
`-p oils --lib`: **1488 passed, 8 failed.** `cargo test -p oils --lib` on its
own passes all 1496.

The difference is not the code. It is `PATH`. Cargo prepends the build's output
directory to `PATH` when it runs a test binary (that is how a test finds its
crate's dynamic libraries on Windows). A workspace build puts **SlateOS's own
coreutils** in that directory — `grep.exe`, `sed.exe`, `cat.exe` and ~200 more.
So under `--workspace`, an `oils` test that pipes through `grep` runs *our*
`grep`, which does not implement what the test was written against. Under
`-p oils` in a target directory where coreutils was never built, the host's GNU
`grep` wins and the test passes.

## The proof

One binary, `osh-c93d43afff5c6245.exe` — the *same hash* in both target
directories, so the same features and the same rustc flags. Only the
environment changes:

```
$ ./target-test/…/deps/osh-c93d43afff5c6245.exe readonly_print_lists_vars \
      posix_mode_is_the_posixly_correct_variable c_on_an_empty_argument_writes_a_nul
test result: ok. 3 passed; 0 failed

$ PATH="$PWD/target/x86_64-pc-windows-gnu/debug:$PATH" \
  ./target-test/…/deps/osh-c93d43afff5c6245.exe <same three>
test result: FAILED. 0 passed; 3 failed
```

And the utility itself, run by hand:

```
$ printf 'declare -r a="1"\ndeclare -r b="2"\n' | ./target/…/debug/grep.exe ' [ab]='
                                                                     (nothing, rc=1)
$ printf 'declare -r a="1"\ndeclare -r b="2"\n' | grep ' [ab]='
declare -r a="1"
declare -r b="2"                                                              rc=0
```

Our `grep` does not support bracket expressions. The workspace log also carries
`grep: unknown option: -E`, `grep: unknown option: -q` and
`grep: unknown option: --`, and a `cat: E9: The system cannot find the file
specified. (os error 2)` whose wording is Rust's `io::Error`, not GNU's — that
is our `cat` too.

## The eight

Every one of them pipes through `grep`, `sed` or `cat`:

```
interp::tests::a_blank_ended_alias_value_expands_the_here_document_delimiter_after_it
interp::tests::assigning_a_dynamic_variable_stores_the_number_not_the_text
interp::tests::builtin_diagnostics_honor_stderr_redirect
interp::tests::c_on_an_empty_argument_writes_a_nul
interp::tests::funcname_is_present_and_empty_outside_a_function
interp::tests::local_dash_binds_a_variable_no_listing_reports
interp::tests::posix_mode_is_the_posixly_correct_variable
interp::tests::readonly_print_lists_vars
```

Which feature each one needs that our coreutils does not have:

| test | needs | ours |
|---|---|---|
| `readonly_print_lists_vars` | `grep ' [ab]='` | no bracket expressions |
| `assigning_a_dynamic_variable_…` | `grep -E` | `unknown option: -E` |
| `local_dash_…` | `grep -c --` | `unknown option: --` |
| `funcname_…` | `grep -c '^declare -a FUNCNAME$'` | anchors/`-c` disagree |
| `posix_mode_…` | `grep '^posix'` | ditto |
| `builtin_diagnostics_honor_stderr_redirect` | `sed 's/^/E:/'` | no substitution applied |
| `c_on_an_empty_argument_writes_a_nul` | `cat -v` | no `-v` |
| `a_blank_ended_alias_…` | `cat` on a missing file | status/wording differ |

`builtin_diagnostics_honor_stderr_redirect` is the clearest read: it got back
`"osh: cd: /no_such_dir_xyz123: No such file or directory\ndone\n"` — the
shell's own diagnostic, complete and correctly routed — with the `E:` prefix
simply absent. The shell did everything right; `sed` did not substitute.

## This is your call, and it is a fork

There are two defensible readings and they lead to different work, which is why
this is a request rather than a patch.

1. **The tests should use the host's tools.** They are unit tests of *the
   shell*; `grep` is scaffolding, and it is an accident of the build layout
   that they get ours. Fix: have the test harness resolve these utilities to an
   absolute host path, or set `PATH` explicitly for the spawned shell, so the
   result does not depend on what else the workspace happened to build.
   *Cheapest, and makes the suite deterministic immediately.*

2. **The tests are right and our coreutils are wrong.** `grep` without bracket
   expressions or `-E`, and `cat` without `-v`, are gaps we will have to close
   anyway — and having the shell's own test suite exercise them is a genuinely
   valuable integration test that we would otherwise have to write. Fix:
   implement the missing features in `userspace/coreutils`.
   *More work, and it fixes something real rather than hiding it.*

They are not exclusive: (1) now to unblock the gate, (2) as its own task. My
recommendation is exactly that, in that order — but the shell and the coreutils
are both yours, so it is yours to decide.

## Why it is urgent rather than merely annoying

**Every lane's pre-merge gate is `cargo test --workspace`, and it is red on
`origin/main` right now.** `userspace/oils` and `userspace/coreutils` in lane
C's tree are byte-identical to `origin/main`, and the failure depends only on
what the workspace build puts in the target directory — so this is not
something a lane introduced, and no lane can merge past it without either
fixing it or knowingly merging into a red trunk.

It is also worse than one red crate. Cargo stops at the first failing test
binary, so `osh` failing means **everything after it alphabetically never
runs** — `p` through `z` is currently untested on every workspace run anyone
does. That is most of `posix`, `userspace`, and a good part of the tree.

## A related note for whoever runs the suite

Do not run two `cargo test --workspace` invocations at once against one
`target/`. An earlier attempt of mine died with `os error 32` — "could not
execute process colorpicker-….exe … being used by another process". That one
*was* my mistake, and it is unrelated to the above.

## Correction

The first version of this file, and the matching `known-issues.md` entry,
diagnosed these eight as a **load-related flake** — transient failure to spawn
a process under a loaded machine. That was wrong. I reached it from a real
observation (a clean re-run of `-p oils --lib` was green) and a real mechanism
(all eight spawn a host utility), but I never checked *which* utility they
spawned, and the re-run that "proved" it was a flake had quietly changed the
one variable that mattered by using a different `CARGO_TARGET_DIR`. Both
documents now carry the real cause. Recording the error because a wrong
diagnosis in a shared file is worse than no diagnosis: the next lane would have
re-run it, seen it pass in isolation, and believed the note.

Delete this file once you've read it.
