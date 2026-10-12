## TD-D-HOST-CLIPPY-IS-OLDER-THAN-THE-UNIX-HALF-GATES — a clean clippy on the Windows host does not predict the pre-push unix-half gate, which lints with WSL's newer toolchain (lane D, 2026-10-07)
**Status:** OPEN — tech debt; the workaround below works, the fix is one toolchain for both

**In short:** before pushing, lanes run clippy on the Windows host, which
has Rust 1.95. The pre-push gate that compiles a crate's `#[cfg(unix)]`
code (`coreutils-unix-half`, `scripts/coreutils-check.sh --only linux`)
runs clippy inside WSL, whose default toolchain is Rust 1.98. Clippy 1.98
has lints 1.95 does not. Where a crate denies `clippy::all`, as posix and
pwhash do, those lints are errors. So a crate clean on the host can still
be refused by the gate. That happened to lane D on 2026-10-07: fifteen
errors in code already on main (`chunks_exact_to_as_chunks` and one
`question_mark`), which had passed every earlier check because the gate
had not run on those pushes.

**What is installed (2026-10-07):**

| Where | Toolchain | clippy |
|---|---|---|
| Windows host (cargo on PATH) | stable 1.95.0 (2026-04-14) | 0.1.95 |
| WSL `stable-x86_64-unknown-linux-gnu`, its default | 1.98.0 (2026-08-18) | 0.1.98 |
| WSL `nightly` | 1.100.0-nightly (2026-09-03) | 0.1.100 |

**Workaround:** before pushing a crate that has `#[cfg(unix)]` code, run
the gate's own reproduction:

    bash scripts/coreutils-check.sh --only linux --no-test --dir <crate>

**The proper fix** is one toolchain version for both. Either pin it in the
tree (`rust-toolchain.toml`, which the workspace does not have, so that
rustup on both sides installs the same version), or update the host to
the WSL version. Either changes every lane's build, so it wants agreement
rather than one lane's edit: the root `Cargo.toml` and the build
environment are no lane's (`open-questions.md` → A-Q11). Until then, every
host clippy run answers for 1.95 only.
