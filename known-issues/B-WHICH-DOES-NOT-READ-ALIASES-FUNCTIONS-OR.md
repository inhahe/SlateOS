## `B-WHICH-DOES-NOT-READ-ALIASES-FUNCTIONS-OR-~USER` (lane B, 2026-08-27) — **open**, missing feature

**In short:** Our `which` now matches GNU which 2.21 for everything it does, but
three of upstream's features are accepted-and-ignored rather than implemented.
None of them can make `which` answer *wrongly* — each only makes it answer less
than upstream would — but a script that relies on them will get a different
answer than it does on Linux.

**What is missing.**

| Missing | Upstream behaviour | Ours |
|---|---|---|
| `--read-alias` / `-i` | Reads shell aliases on stdin (the shell function in `alias`(1)'s `which` wrapper pipes them in) and reports an alias before searching `PATH`. | Option parsed and ignored; only `PATH` is searched. |
| `--read-functions` | Same, for shell function definitions. | Option parsed and ignored. |
| `~user` in a `PATH` element | Expands to that user's home via `getpwnam`. | Left literal, so the element never matches. `~` and `~/…` *are* expanded from `$HOME` and work. |

**Why they are not done.** The alias/function features are not really `which`
features — they are a protocol between `which` and the shell that invoked it,
and the useful half of that protocol (the `which` shell function that pipes
`alias`/`declare -f` in) does not exist in our shell yet. Implementing the
reader without the writer produces a feature nobody can reach. `~user` needs
`getpwnam` and the `struct passwd` layout, which the coreutils crate does not
bind today; `~` and `~/…` cover the case that actually appears in a `PATH`.

**Proper fix.** For the aliases: land the shell-side wrapper first, then read
stdin as bytes and match before the `PATH` scan. For `~user`: bind `getpwnam_r`
(reentrant, so the returned buffer is ours) behind `#[cfg(unix)]` alongside the
existing `euidaccess`/`geteuid` block in `which.rs`, and expand in
`expand_tilde`, which already isolates the decision.

**Where it lives:** `userspace/coreutils/src/bin/which.rs` — the ignored options
are in `parse_args`, and `expand_tilde` is where `~user` would go. The module
doc's "What is not implemented" section says the same thing at the code.
