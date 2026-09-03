#!/usr/bin/env python3
"""Tests for `rustemit.py` and the checker built on it.

`rustemit` exists to remove a hand transcription: `check-evdev-elf-asm.py`
mirrors, in Python, machine code that `kernel/src/proc/elf.rs` emits as byte
literals, and for as long as that file existed nothing compared the two.  The
module rebuilds the buffer from the Rust so the mirror can be checked against
it.

**A guard is worth having only once it has been seen to fire**, so most of what
follows perturbs something and requires the complaint.  A reader who is told
"the mirror is checked against elf.rs" has no way to know whether the check can
fail; these tests are how that claim is paid for.

The last group is the important one: it takes the real checker, corrupts one
byte of its real mirror, and requires the real comparison to name that byte.
Everything else tests a part; that tests the thing.
"""

import importlib.util
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import rustemit  # noqa: E402
from rustemit import Emitter, RustEmitError  # noqa: E402

REPO = HERE.parent
EVDEV_RS = REPO / "kernel" / "src" / "evdev.rs"

failures = []
count = 0


def check(label, condition):
    global count
    count += 1
    if not condition:
        failures.append(label)
        print(f"FAIL {label}")


def raises(label, fn, needle=""):
    """`fn` must raise RustEmitError, and the message must mention `needle`."""
    global count
    count += 1
    try:
        fn()
    except RustEmitError as exc:
        if needle and needle not in str(exc):
            failures.append(f"{label} (message lacked {needle!r}: {exc})")
            print(f"FAIL {label}: message lacked {needle!r}")
        return
    except Exception as exc:  # noqa: BLE001 - any other type is itself the bug
        failures.append(f"{label} (raised {type(exc).__name__}: {exc})")
        print(f"FAIL {label}: raised {type(exc).__name__}, not RustEmitError")
        return
    failures.append(f"{label} (did not raise)")
    print(f"FAIL {label}: did not raise")


# ---------------------------------------------------------------------------
# Statement splitting -- both bugs that actually happened.
# ---------------------------------------------------------------------------

# `&[0u8; 8]` is an array-repeat, not two statements. Splitting it dropped
# eight bytes from the rebuild.
parts = rustemit._split_statements("code.extend_from_slice(&[0u8; 8]);")
check("array-repeat is not split on its inner semicolon",
      len([p for p in parts if p.strip()]) == 1)

# A block ends a statement even though it carries no `;`. Without this the
# block swallowed the next statement, and because blocks are ignored the
# swallowed one was silently discarded -- three bytes of `mov rdi, r8`.
parts = [p.strip() for p in rustemit._split_statements(
    "for site in &sites { let d = 1; code[*site] = d; }\ncode.push(0x4C);"
) if p.strip()]
check("a block ends a statement", len(parts) == 2)
check("the statement after a block survives", parts[1] == "code.push(0x4C)")


# ---------------------------------------------------------------------------
# Constants are read, never assumed.
# ---------------------------------------------------------------------------

evdev_src = EVDEV_RS.read_text(encoding="utf-8")
consts = rustemit.load_consts(evdev_src, ("KEY_MAX", "KEY_BYTES", "EVDEV_IOC_MAGIC"))
# KEY_BYTES is `(KEY_MAX as usize + 8) / 8` -- a computed const. A reader that
# only accepted literals would have to fall back to a hardcoded 96, which is
# the transcription being removed.
check("a computed const is evaluated, not hardcoded",
      consts["KEY_BYTES"] == (consts["KEY_MAX"] + 8) // 8)
check("KEY_BYTES is the value the kernel uses", consts["KEY_BYTES"] == 96)

raises("a renamed constant is reported, not defaulted",
       lambda: rustemit.load_consts(evdev_src, ("KEY_MAX", "NO_SUCH_CONST")),
       "NO_SUCH_CONST")

raises("a constant that cannot be evaluated is reported",
       lambda: rustemit.load_consts(
           "pub const ODD: u32 = some_fn(3);", ("ODD",)),
       "could not be evaluated")

raises("a renamed value function is reported",
       lambda: rustemit.load_value_fns(evdev_src, ("no_such_encoder",)),
       "no_such_encoder")

# `ioc` is read out of evdev.rs rather than transcribed, so the _IOC bit
# layout has exactly one definition in the tree.
ioc = rustemit.load_value_fns(evdev_src, ("ioc",))
em = Emitter(rustemit.load_consts(evdev_src, ("EVDEV_IOC_MAGIC", "IOC_READ")), ioc)
check("the _IOC layout is replayed from evdev::ioc",
      em.value("ioc(IOC_READ, 0x18, 96)") == ((2 << 30) | (96 << 16) | (0x45 << 8) | 0x18))


# ---------------------------------------------------------------------------
# The discovery floor: anything unmodelled is an error, never a skip.
# ---------------------------------------------------------------------------

def _unmodelled():
    e = Emitter({})
    e.run("code.extend_from_slice(&[0x90]); code.frobnicate(7);")


raises("an unmodelled statement stops the run", _unmodelled, "not modelled")

raises("unknown_ok cannot be switched on",
       lambda: Emitter({}).run("code.push(0x90);", unknown_ok=True),
       "unknown_ok=True")

raises("a missing emitting helper is reported",
       lambda: Emitter({}).learn_helpers("fn other() {}", ("sentinel",)),
       "sentinel")

# A value that is neither a literal nor a known constant is not guessed at.
raises("an unresolvable operand stops the run",
       lambda: Emitter({}).run("code.push(MYSTERY_CONSTANT);"),
       "cannot evaluate")

# A patched range with no recorded position would be compared against bytes the
# module cannot know, so it refuses rather than exempting a guessed span.
def _unanchored_patch():
    e = Emitter({})
    e.run("code.push(0x90);")
    e.mark_patched("code[nowhere..nowhere + 8].copy_from_slice(&x.to_le_bytes());")


raises("a patch with no recorded position is reported", _unanchored_patch, "nowhere")


# ---------------------------------------------------------------------------
# Replaying a small function end to end.
# ---------------------------------------------------------------------------

TOY = """
    fn sentinel(code: &mut Vec<u8>, value: u32) {
        code.push(0xBF);
        code.extend_from_slice(&value.to_le_bytes());
    }
    fn jcc(code: &mut Vec<u8>, sites: &mut Vec<usize>, cc: u8) {
        code.extend_from_slice(&[0x0F, cc, 0, 0, 0, 0]);
        sites.push(code.len() - 4);
    }
    const JNZ: u8 = 0x85;
    let mut code: Vec<u8> = Vec::new();
    let mut fail_sites: Vec<usize> = Vec::new();
    sentinel(&mut code, 0xE1);
    jcc(&mut code, &mut fail_sites, JNZ);
"""
toy = Emitter({})
toy.learn_helpers(TOY, ("sentinel", "jcc"))
toy.learn_local_consts(TOY)
toy.run(TOY[TOY.index("const JNZ"):])
check("helper bodies are replayed, not transcribed",
      bytes(toy.code) == bytes([0xBF, 0xE1, 0, 0, 0, 0x0F, 0x85, 0, 0, 0, 0]))
check("a jump displacement is recorded as a wildcard",
      toy.wildcards == {7, 8, 9, 10})


# `compare` must skip the wildcards and nothing else.
check("compare ignores a wildcard byte",
      rustemit.compare(b"\x01\x00\x03", b"\x01\xff\x03", {1}) == [])
check("compare reports a non-wildcard byte",
      len(rustemit.compare(b"\x01\x00\x03", b"\x01\xff\x03", set())) == 1)
check("compare reports a length difference",
      any("length" in d for d in rustemit.compare(b"\x01", b"\x01\x02", set())))


# ---------------------------------------------------------------------------
# The real checker, made to fire on its real data.
# ---------------------------------------------------------------------------

spec = importlib.util.spec_from_file_location(
    "check_evdev_elf_asm", HERE / "check-evdev-elf-asm.py"
)
checker = importlib.util.module_from_spec(spec)
skipped = ""
try:
    spec.loader.exec_module(checker)
except SystemExit as exc:  # capstone missing -- the module exits on import
    # This group is the valuable one, so its absence goes in the FINAL LINE.
    # boot-test.sh prints only that line per suite, and a suite that quietly
    # drops its best tests while still reporting "all N passed" is the same
    # silent-skip defect this whole module was written to remove -- it would
    # just be this file committing it.
    skipped = f" (END-TO-END GROUP SKIPPED: {exc})"
    print(f"SKIP the end-to-end group: {exc}")
    checker = None

if checker is not None:
    for denied in (False, True):
        mirror, _fail, _read_ok = checker.build_mirror(denied)
        built, wildcards = checker.rebuild_from_rust(denied)
        check(f"mirror matches elf.rs (expect_denied={denied})",
              rustemit.compare(built, mirror, wildcards) == [])

        # Now corrupt one byte of the real mirror at a position that is NOT
        # exempt, and require the real comparison to name it. This is the whole
        # claim of the rewrite: before it, this corruption changed nothing.
        pos = next(i for i in range(len(mirror)) if i not in wildcards)
        bad = bytearray(mirror)
        bad[pos] ^= 0xFF
        diffs = rustemit.compare(built, bytes(bad), wildcards)
        check(f"a one-byte drift is caught (expect_denied={denied})",
              len(diffs) == 1 and f"offset 0x{pos:x}" in diffs[0])

        # A dropped statement changes the length, which must also be caught --
        # that is the shape the block-swallowing bug actually took.
        diffs = rustemit.compare(built, bytes(mirror[:-3]), wildcards)
        check(f"a short mirror is caught (expect_denied={denied})",
              any("length" in d for d in diffs))

    raises("a renamed emission function is reported",
           lambda: checker._function_text("fn something_else() {}", checker.FN),
           checker.FN)


if failures:
    print(f"\n{len(failures)} of {count} rustemit test(s) FAILED:")
    for f in failures:
        print(f"  {f}")
    sys.exit(1)
print(f"all {count} rustemit tests passed{skipped}")
