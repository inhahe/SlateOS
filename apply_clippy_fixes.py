#!/usr/bin/env python3
"""Rustfix-style applier for clippy MachineApplicable suggestions.

`cargo clippy --fix` is non-functional in this environment (write-back
phase no-ops), so we parse `cargo clippy --message-format=json` output
ourselves and apply only the suggestions rustc marks MachineApplicable.

NOTE: rustc's JSON `byte_start`/`byte_end` are GLOBAL SourceMap offsets,
not file-relative, so they are useless here. We use the per-span
`line_start`/`column_start`/`line_end`/`column_end` (1-based, char-based
on the LF-normalized source) instead. We normalize each file's CRLF -> LF
before applying (rustc's line/col view is LF; invisible to git under
core.autocrlf=input). A recompile after applying is the safety net.

Usage: python apply_clippy_fixes.py <clippy-json-file> [--only LINT[,LINT...]]
"""
import sys, json, collections, os

ROOT = os.path.dirname(os.path.abspath(__file__))


def collect_suggestions(msg, out, accept_maybe=False):
    """Walk a diagnostic + its children, collecting applicable spans.

    By default only MachineApplicable spans are collected. When
    `accept_maybe` is True (used for a curated allowlist of purely
    cosmetic doc-comment lints whose single-span suggestions are
    mechanical leading-whitespace dedents), MaybeIncorrect spans are
    also collected. We never auto-apply HasPlaceholders/Unspecified.
    """
    ok = {"MachineApplicable"}
    if accept_maybe:
        ok.add("MaybeIncorrect")
    for sp in msg.get("spans", []):
        rep = sp.get("suggested_replacement")
        app = sp.get("suggestion_applicability")
        if rep is not None and app in ok:
            out.append(sp)
    for child in msg.get("children", []):
        collect_suggestions(child, out, accept_maybe)


def line_col_to_offset(line_starts, line, col):
    """1-based line, 1-based char col -> absolute char offset in joined text."""
    # line_starts[i] = char offset of start of line (i+1)
    return line_starts[line - 1] + (col - 1)


def main():
    if len(sys.argv) < 2:
        print("usage: apply_clippy_fixes.py <json> [--only lint,lint]")
        sys.exit(2)
    json_path = sys.argv[1]
    only = None
    if "--only" in sys.argv:
        only = set(sys.argv[sys.argv.index("--only") + 1].split(","))
    # Lints whose MaybeIncorrect single-span suggestions are safe to apply
    # (cosmetic doc-comment whitespace only; cannot affect compiled code).
    also_maybe = set()
    if "--also-maybe" in sys.argv:
        also_maybe = set(sys.argv[sys.argv.index("--also-maybe") + 1].split(","))

    spans = []
    with open(json_path, "r", encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                obj = json.loads(line)
            except json.JSONDecodeError:
                continue
            if obj.get("reason") != "compiler-message":
                continue
            m = obj.get("message", {})
            # Only deny-level (clippy::all) errors; warn-level lints are
            # intentional-by-design per the workspace config (TD2).
            if m.get("level") != "error":
                continue
            code = (m.get("code") or {}).get("code") or ""
            if only is not None and code not in only:
                continue
            collect_suggestions(m, spans, accept_maybe=(code in also_maybe))

    by_file = collections.defaultdict(list)
    for sp in spans:
        fn = sp["file_name"]
        norm = fn.replace("\\", "/")
        if "kernel/src/" not in norm:
            continue
        by_file[fn].append(sp)

    total_applied = 0
    total_skipped = 0
    for fn, file_spans in sorted(by_file.items()):
        path = os.path.join(ROOT, fn)
        if not os.path.exists(path):
            print(f"  MISSING {fn}")
            continue
        with open(path, "rb") as fh:
            raw = fh.read()
        text = raw.replace(b"\r\n", b"\n").decode("utf-8", errors="strict")
        lines = text.split("\n")  # lines without the '\n'
        # char offset of the start of each line in `text`
        line_starts = [0] * (len(lines) + 1)
        acc = 0
        for i, ln in enumerate(lines):
            line_starts[i] = acc
            acc += len(ln) + 1  # + '\n'
        line_starts[len(lines)] = acc

        edits = []
        for sp in file_spans:
            try:
                s = line_col_to_offset(line_starts, sp["line_start"], sp["column_start"])
                e = line_col_to_offset(line_starts, sp["line_end"], sp["column_end"])
            except (IndexError, KeyError):
                continue
            edits.append((s, e, sp["suggested_replacement"]))

        edits = sorted(set(edits), key=lambda x: x[0], reverse=True)
        applied = 0
        skipped = 0
        last_start = len(text) + 1
        for s, e, rep in edits:
            if s > e or e > len(text) or s < 0:
                skipped += 1
                continue
            if e > last_start:  # overlaps a later-applied edit
                skipped += 1
                continue
            text = text[:s] + rep + text[e:]
            last_start = s
            applied += 1
        with open(path, "wb") as fh:
            fh.write(text.encode("utf-8"))
        total_applied += applied
        total_skipped += skipped
        print(f"  {fn}: applied {applied}, skipped {skipped}")

    print(f"\nTOTAL applied {total_applied}, skipped {total_skipped}")


if __name__ == "__main__":
    main()
