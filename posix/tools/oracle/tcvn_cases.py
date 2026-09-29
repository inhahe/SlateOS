"""TCVN5712-1's cases for the iconv oracle (see cp125x_harness.py, whose
generator and runner tcvn_harness.py reuses)."""

CASES = [
    # TCVN5712-1 -> UTF-8
    ("tc_low_letters", "UTF-8", "TCVN5712-1", [b"\x01\x02\x03\x11"], 64, "out"),
    ("tc_high_letter", "UTF-8", "TCVN5712-1", [b"\xb5x"], 64, "out"),
    ("tc_compose_grave", "UTF-8", "TCVN5712-1", [b"a\xb0"], 64, "out"),
    ("tc_compose_tilde", "UTF-8", "TCVN5712-1", [b"N\xb2"], 64, "out"),
    ("tc_compose_hook", "UTF-8", "TCVN5712-1", [b"o\xb1"], 64, "out"),
    ("tc_mark_alone", "UTF-8", "TCVN5712-1", [b"\xb0"], 64, "out"),
    ("tc_nocompose", "UTF-8", "TCVN5712-1", [b"q\xb0"], 64, "out"),
    ("tc_across_calls", "UTF-8", "TCVN5712-1", [b"e", b"\xb3"], 64, "out"),
    ("tc_held_null", "UTF-8", "TCVN5712-1", [b"xyz"], 64, "null"),
    ("tc_every_byte_valid", "UTF-8", "TCVN5712-1", [bytes(range(0x80, 0x100))], 512, "out"),
    ("tc_nul", "UTF-8", "TCVN5712-1", [b"\x00a"], 64, "out"),
    ("tc_tight", "UTF-8", "TCVN5712-1", [b"ab\xb5c"], 3, "out"),
    ("tc_to_wchar", "WCHAR_T", "TCVN5712-1", [b"a\xb0\x01"], 64, "out"),
    # UTF-8 -> TCVN5712-1
    ("tc_enc_direct", "TCVN5712-1", "UTF-8", ["àẠ \u0003".encode()], 64, "out"),
    ("tc_enc_low", "TCVN5712-1", "UTF-8", ["ÚỤỴ".encode()], 64, "out"),
    ("tc_enc_control_taken", "TCVN5712-1", "UTF-8", ["a\u0001b".encode()], 64, "out"),
    ("tc_enc_control_17", "TCVN5712-1", "UTF-8", ["a\u0011b".encode()], 64, "out"),
    ("tc_enc_decomp", "TCVN5712-1", "UTF-8", ["Ññ".encode()], 64, "out"),
    ("tc_enc_marks", "TCVN5712-1", "UTF-8", ["ạ̀".encode()], 64, "out"),
    ("tc_enc_unwritable", "TCVN5712-1", "UTF-8", ["a一".encode()], 64, "out"),
    ("tc_enc_tight_decomp", "TCVN5712-1", "UTF-8", ["abÑ".encode()], 3, "out"),
    ("tc_names", "UTF-8", "TCVN", [b"a\xb0"], 64, "out"),
    ("tc_names2", "TCVN-5712", "UTF-8", ["à".encode()], 64, "out"),
    ("tc_names3", "UTF-8", "TCVN5712-1:1993", [b"\xb5"], 64, "out"),
]
