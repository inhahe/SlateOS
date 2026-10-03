## 16. nushell port — build with the msvc nightly toolchain; leave upstream warnings unpatched

**Date:** 2026-06-03

**Decided by:** Claude (autonomous) — a build-toolchain workaround and a
don't-touch-upstream call made while bringing up the nushell port; both
reversible.

**Context:**
The project's default toolchain is `1.93.1-x86_64-pc-windows-gnu` (per
`rust-toolchain.toml`). The nushell port hit an upstream gnu-toolchain build
issue. Separately, building nushell surfaces a handful of upstream warnings not
introduced by our port: an unused `PipelineMetadata` import in
`nu-cli/.../history_.rs`, an unused `path::Path` import in `nu/src/command.rs`, a
dead-code `ListPath` variant in the nu binary, and a future-incompat warning on
`proc-macro-error2 v2.0.1` (a dep of a dep).

**Decision:**
- **Build nushell specifically with `rustup run nightly-x86_64-pc-windows-msvc`**
  until the upstream gnu issue is resolved or the broader project moves to msvc.
  The project pin is NOT changed; this affects only how nushell is built, no
  other workspace crate.
- **Do not patch the upstream nushell warnings** — they are warnings only, not
  introduced by our port, and not worth carrying local edits to upstream code.

**Why not the alternatives:** upgrading the project-wide toolchain pin to msvc
would affect every crate and is a larger decision than one port warrants;
patching upstream warnings creates a maintenance burden against future nushell
updates for no functional gain.

**Where it lives:** the nushell port build invocation (msvc nightly); the warnings
live in upstream `nu-cli`/`nu` sources. Reverse the toolchain workaround when the
gnu issue is fixed or the project moves to msvc.
