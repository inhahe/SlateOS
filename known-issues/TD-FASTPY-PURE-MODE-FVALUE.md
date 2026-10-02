### TD-FASTPY-PURE-MODE-FVALUE. fastpy pure-native mode (`-DFPY_PURE_MODE`, no CPython bridge) can't dispatch methods on, or iterate, a "flexible value" (FVALUE) — the element type you get from indexing OR iterating an `append`-built list — 2026-07-23 — OPEN (fastpy codegen constraint; documented + worked around)

**What it is (a fastpy compiler limitation, not an OS bug).** When writing OS
components as native Python compiled by fastpy (initiative F), the pure-native
runtime excludes `runtime/cpython_bridge.c` (replaced by `bridge_stub.c`). Any
operation that would dynamic-dispatch through the bridge is unavailable and
fails as either a **link error** or a **runtime hang**:

1. **Method dispatch on an FVALUE → link error.** Indexing an `append`-built
   list (`x = mylist[i]`) yields an FVALUE, not a concrete type. Calling a
   method on it (e.g. `x.find('/')`) emits a call to `fpy_fv_call_method1`,
   which is undefined in pure mode → `undefined symbol: fpy_fv_call_method1`
   at link time. (codegen conditions FV dispatch on receiver VKind ∈
   {PATH,PYOBJ,MIXED,FVALUE}: compiler/codegen.py ~42549.)
2. **Iterating an FVALUE → silent infinite spin.** Iterating an element of an
   `append`-built **nested** list (list-of-lists) — e.g. `st = stages[i]` or
   even `for st in stages:` then `for tok in st:` — iterates an FVALUE, which
   spins forever in a **pure-CPU loop with no syscalls and no output** (looks
   like a dead hang: QEMU alive, serial log frozen, no child ever forked).
   This one is nastier than (1) because it *compiles cleanly* and only manifests
   at runtime.

**Workarounds (both proven in `services/fastpy-minishell/build.py`).**
- Never call methods on an indexed list element; capture the concrete value
  from the **loop variable** instead (`for t in toks: ... cmd = t`), then call
  methods on `t`/`cmd`.
- Never build a **list-of-lists**. Restructure to a **single flat forward
  pass** over one flat `append` list (append a sentinel to flush the tail case
  uniformly). Flat-list iteration by loop variable, `str` methods on the loop
  variable, list literals (`PATH = [...]`) and `argv` handed to `os.execv` are
  all fine.

**Proper fix (upstream, in fastpy — out of scope for the OS repo).** Emit
pure-mode-safe monomorphic paths for FVALUE method-dispatch and iteration (or
teach codegen to propagate concrete element types through `append`-built
containers so the element isn't an FVALUE in the first place). Until then, the
constraint is a known authoring rule for fastpy-compiled OS components.
