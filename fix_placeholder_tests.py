"""
Replace placeholder `mod tests { #[test] fn test_basic() { assert!(true); } }`
in userspace CLI stubs with real tests.

The placeholder triggers clippy::assertions_on_constants under --all-targets.

Strategy:
- Read each file flagged by grep
- Parse out the `fn run_<name>(...) -> i32` signature
- Emit a test module that exercises basename, strip_ext, and run_X
  on --help, -h, --version, and the default (empty args) path
- All four return 0 by contract in these stubs (they print and exit 0)
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

PLACEHOLDER = "#[cfg(test)]\nmod tests { #[test] fn test_basic() { assert!(true); } }\n"
PLACEHOLDER_VARIANTS = [
    "#[cfg(test)]\nmod tests { #[test] fn test_basic() { assert!(true); } }",
    "#[cfg(test)]\nmod tests {\n    #[test]\n    fn test_basic() {\n        assert!(true);\n    }\n}",
    "#[cfg(test)]\nmod tests {\n    #[test]\n    fn test_basic() { assert!(true); }\n}",
]

# Signature shapes we support
RE_RUN_2ARG_STR = re.compile(r"^fn (run_\w+)\(args: &\[String\], _?\w+: &str\) -> i32", re.M)
RE_RUN_2ARG_BOOL = re.compile(r"^fn (run_\w+)\(args: &\[String\], _?\w+: bool\) -> i32", re.M)
RE_RUN_2ARG_REV = re.compile(r"^fn (run_\w+)\(_?\w+: &str, args: &\[String\]\) -> i32", re.M)
RE_RUN_1ARG = re.compile(r"^fn (run_\w+)\(args: &\[String\]\) -> i32", re.M)
RE_RUN_VEC_2 = re.compile(r"^fn (run_\w+)\(args: Vec<String>, .*\) -> i32", re.M)
RE_RUN_VEC_1 = re.compile(r"^fn (run_\w+)\(args: Vec<String>\) -> i32", re.M)
RE_BASENAME = re.compile(r"^fn basename\(", re.M)
RE_STRIP_EXT = re.compile(r"^fn strip_ext\(", re.M)


def make_tests(run_name: str, arg_shape: str, has_basename: bool, has_strip_ext: bool, file_name_hint: str) -> str:
    # arg_shape: '2slice', '1slice', '2vec', '1vec'
    imports = []
    if has_basename:
        imports.append("basename")
    if has_strip_ext:
        imports.append("strip_ext")
    imports.append(run_name)
    use_line = f"    use super::{{{', '.join(imports)}}};"

    lines: list[str] = []
    lines.append("#[cfg(test)]")
    lines.append("mod tests {")
    lines.append(use_line)
    lines.append("")
    if has_basename:
        lines.append("    #[test]")
        lines.append("    fn basename_strips_path() {")
        lines.append(f'        assert_eq!(basename("/usr/bin/{file_name_hint}"), "{file_name_hint}");')
        lines.append(f'        assert_eq!(basename(r"C:\\bin\\{file_name_hint}.exe"), "{file_name_hint}.exe");')
        lines.append('        assert_eq!(basename("plain"), "plain");')
        lines.append("    }")
        lines.append("")
    if has_strip_ext:
        lines.append("    #[test]")
        lines.append("    fn strip_ext_removes_extension() {")
        lines.append(f'        assert_eq!(strip_ext("{file_name_hint}.exe"), "{file_name_hint}");')
        lines.append('        assert_eq!(strip_ext("no-ext"), "no-ext");')
        lines.append("    }")
        lines.append("")

    # run tests
    lines.append("    #[test]")
    lines.append("    fn help_and_version_exit_zero() {")
    if arg_shape == "2slice":
        lines.append(f'        assert_eq!({run_name}(&["--help".to_string()], "{file_name_hint}"), 0);')
        lines.append(f'        assert_eq!({run_name}(&["-h".to_string()], "{file_name_hint}"), 0);')
        lines.append(f'        assert_eq!({run_name}(&["--version".to_string()], "{file_name_hint}"), 0);')
    elif arg_shape == "2slice_bool":
        lines.append(f'        assert_eq!({run_name}(&["--help".to_string()], false), 0);')
        lines.append(f'        assert_eq!({run_name}(&["-h".to_string()], false), 0);')
        lines.append(f'        assert_eq!({run_name}(&["--version".to_string()], false), 0);')
    elif arg_shape == "2slice_rev":
        lines.append(f'        assert_eq!({run_name}("{file_name_hint}", &["--help".to_string()]), 0);')
        lines.append(f'        assert_eq!({run_name}("{file_name_hint}", &["-h".to_string()]), 0);')
        lines.append(f'        assert_eq!({run_name}("{file_name_hint}", &["--version".to_string()]), 0);')
    elif arg_shape == "1slice":
        lines.append(f'        assert_eq!({run_name}(&["--help".to_string()]), 0);')
        lines.append(f'        assert_eq!({run_name}(&["-h".to_string()]), 0);')
        lines.append(f'        assert_eq!({run_name}(&["--version".to_string()]), 0);')
    elif arg_shape == "2vec":
        lines.append(f'        assert_eq!({run_name}(vec!["--help".to_string()], "{file_name_hint}"), 0);')
        lines.append(f'        assert_eq!({run_name}(vec!["-h".to_string()], "{file_name_hint}"), 0);')
        lines.append(f'        assert_eq!({run_name}(vec!["--version".to_string()], "{file_name_hint}"), 0);')
    elif arg_shape == "1vec":
        lines.append(f'        assert_eq!({run_name}(vec!["--help".to_string()]), 0);')
        lines.append(f'        assert_eq!({run_name}(vec!["-h".to_string()]), 0);')
        lines.append(f'        assert_eq!({run_name}(vec!["--version".to_string()]), 0);')
    lines.append("    }")
    lines.append("")
    lines.append("    #[test]")
    lines.append("    fn default_invocation_exits_zero() {")
    if arg_shape == "2slice":
        lines.append(f'        assert_eq!({run_name}(&[], "{file_name_hint}"), 0);')
    elif arg_shape == "2slice_bool":
        lines.append(f"        assert_eq!({run_name}(&[], false), 0);")
    elif arg_shape == "2slice_rev":
        lines.append(f'        assert_eq!({run_name}("{file_name_hint}", &[]), 0);')
    elif arg_shape == "1slice":
        lines.append(f"        assert_eq!({run_name}(&[]), 0);")
    elif arg_shape == "2vec":
        lines.append(f'        assert_eq!({run_name}(vec![], "{file_name_hint}"), 0);')
    elif arg_shape == "1vec":
        lines.append(f"        assert_eq!({run_name}(vec![]), 0);")
    lines.append("    }")
    lines.append("}")
    return "\n".join(lines) + "\n"


def process_file(path: Path) -> tuple[bool, str]:
    text = path.read_text(encoding="utf-8")

    # Find run function name + signature shape
    arg_shape = None
    run_name = None
    for shape, regex in (
        ("2slice", RE_RUN_2ARG_STR),
        ("2slice_bool", RE_RUN_2ARG_BOOL),
        ("2slice_rev", RE_RUN_2ARG_REV),
        ("1slice", RE_RUN_1ARG),
        ("2vec", RE_RUN_VEC_2),
        ("1vec", RE_RUN_VEC_1),
    ):
        m = regex.search(text)
        if m:
            run_name = m.group(1)
            arg_shape = shape
            break
    if not run_name:
        return False, "no run_X function found"

    has_basename = bool(RE_BASENAME.search(text))
    has_strip_ext = bool(RE_STRIP_EXT.search(text))

    # Derive file_name_hint from the crate directory name (strip -cli suffix)
    crate_dir = path.parent.parent.name  # .../userspace/<crate>/src/main.rs
    file_name_hint = crate_dir.removesuffix("-cli") if crate_dir.endswith("-cli") else crate_dir

    tests = make_tests(run_name, arg_shape, has_basename, has_strip_ext, file_name_hint)

    # Replace placeholder. Try variants.
    new_text = None
    for variant in PLACEHOLDER_VARIANTS:
        if variant in text:
            new_text = text.replace(variant, tests.rstrip("\n"))
            # ensure trailing newline preserved
            if not new_text.endswith("\n"):
                new_text += "\n"
            break
    if new_text is None:
        return False, "placeholder not found verbatim"

    path.write_text(new_text, encoding="utf-8")
    return True, f"{run_name} {arg_shape}"


def main() -> int:
    root = Path("userspace")
    candidates = list(root.glob("*/src/main.rs"))
    fixed = 0
    skipped = 0
    skip_reasons: dict[str, int] = {}
    for p in candidates:
        try:
            text = p.read_text(encoding="utf-8")
        except Exception:
            continue
        if "fn test_basic() { assert!(true); }" not in text and "fn test_basic() {\n        assert!(true);" not in text:
            continue
        ok, reason = process_file(p)
        if ok:
            fixed += 1
        else:
            skipped += 1
            skip_reasons[reason] = skip_reasons.get(reason, 0) + 1
            print(f"SKIP {p}: {reason}", file=sys.stderr)
    print(f"Fixed: {fixed}, Skipped: {skipped}")
    for r, n in skip_reasons.items():
        print(f"  {r}: {n}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
