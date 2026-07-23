#!/usr/bin/env python3
"""One-shot refactor: convert fastpy self-test `include_bytes!` embeds in
kernel/src/proc/spawn.rs into runtime disk loads via load_test_elf().

For each  `static FASTPY_<X>_ELF: &[u8] = include_bytes!(".../fastpy-<name>/fastpy-<name>.elf");`
declaration (1- or 2-line form), replace it with a `let fastpy_<x>_elf = match
load_test_elf("fastpy-<name>") {...}` block that self-skips (return Ok(())) when
the fixture is absent, and rename every downstream `FASTPY_<X>_ELF` reference to
the lowercase local `fastpy_<x>_elf`.

The `&` needed at each spawn_process(...) call (Vec<u8> vs &[u8]) is left for the
compiler to flag and is fixed by hand — this script only does the unambiguous
declaration + rename work. fastpy-hello is already converted by hand and has no
`static` decl left, so it is naturally skipped.
"""
import re
import sys
import pathlib

SRC = pathlib.Path("kernel/src/proc/spawn.rs")
text = SRC.read_text(encoding="utf-8", newline="")

# Match the static decl, 1- or 2-line, capturing the const name and the fastpy dir name.
decl_re = re.compile(
    r'[ \t]*static (FASTPY_[A-Z0-9_]+_ELF): &\[u8\] =\s*'
    r'include_bytes!\("\.\./\.\./\.\./services/(fastpy-[a-z0-9]+)/fastpy-[a-z0-9]+\.elf"\);',
    re.MULTILINE,
)

consts_seen = {}  # const_name -> key

def repl(m):
    const_name = m.group(1)
    key = m.group(2)            # e.g. fastpy-getmtime
    snake = const_name.lower()  # e.g. fastpy_getmtime_elf
    consts_seen[const_name] = key
    return (
        f'    let {snake} = match load_test_elf("{key}") {{\n'
        f'        Some(v) => v,\n'
        f'        None => {{\n'
        f'            serial_println!(\n'
        f'                "[spawn] SKIP {key}: fixture absent on /mnt/tests (lean build)"\n'
        f'            );\n'
        f'            return Ok(());\n'
        f'        }}\n'
        f'    }};'
    )

text, n_decls = decl_re.subn(repl, text)

# Rename every remaining reference of each converted const to its snake local.
n_refs = 0
for const_name in consts_seen:
    snake = const_name.lower()
    text, c = re.subn(r'\b' + const_name + r'\b', snake, text)
    n_refs += c

SRC.write_text(text, encoding="utf-8", newline="")
print(f"converted {n_decls} static decl(s); renamed {n_refs} reference(s)")
print(f"distinct consts: {len(consts_seen)}")
for c, k in sorted(consts_seen.items()):
    print(f"  {c} -> {c.lower()}  ({k})")
