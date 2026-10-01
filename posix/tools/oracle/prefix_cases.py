"""T.61's, ISO_6937's, ISO_6937-2's and ANSI_X3.110's cases for the iconv
oracle (see cp125x_harness.py, whose generator and runner prefix_harness.py
reuses): what the tables cannot say -- a diacritic byte with a bad letter
after it, one at the end of the input or of a call, a pair that needs room
for two bytes -- and the four sets' own special cases, and names."""

U = lambda s: s.encode("utf-8")  # noqa: E731

CASES = [
    # T.61 -> UTF-8: a byte, or a diacritic byte and its letter.
    ("t61_ascii", "UTF-8", "T.61", [b"Hello"], 64, "out"),
    ("t61_pair", "UTF-8", "T.61", [b"\xc2e\xc1A\xc8u"], 64, "out"),
    ("t61_highs", "UTF-8", "T.61", [b"\xa1\xa3\xe0\xfb"], 64, "out"),
    ("t61_bad_letter_low", "UTF-8", "T.61", [b"\xc1\x10A"], 64, "out"),
    ("t61_bad_letter_low_ignore", "UTF-8//IGNORE", "T.61", [b"\xc1\x10A"], 64, "out"),
    ("t61_bad_letter_high_ignore", "UTF-8//IGNORE", "T.61", [b"\xc1\xc2eA"], 64, "out"),
    ("t61_no_such_pair", "UTF-8", "T.61", [b"x\xc1\x42A"], 64, "out"),
    ("t61_no_such_pair_ignore", "UTF-8//IGNORE", "T.61", [b"x\xc1\x42A"], 64, "out"),
    ("t61_no_such_byte_ignore", "UTF-8//IGNORE", "T.61", [b"a\xc0b"], 64, "out"),
    ("t61_diacritic_last", "UTF-8", "T.61", [b"a\xc1"], 64, "out"),
    ("t61_diacritic_across_calls", "UTF-8", "T.61", [b"a\xc1", b"\xc1e"], 64, "out"),
    ("t61_diacritic_alone", "UTF-8", "T.61", [b"\xc3"], 64, "null"),
    ("t61_every_byte_ignore", "UTF-8//IGNORE", "T.61",
     [bytes(range(0, 0xc1)) + bytes(range(0xd0, 0x100))], 1024, "out"),
    ("t61_tight", "UTF-8", "T.61", [b"ab\xc2e"], 3, "out"),
    ("t61_to_wchar", "WCHAR_T", "T.61", [b"a\xc2e"], 64, "out"),
    ("t61_to_wchar_tight", "WCHAR_T", "T.61", [b"a\xc2e"], 4, "out"),
    # UTF-8 -> T.61
    ("t61_enc", "T.61", "UTF-8", [U("À é ü")], 64, "out"),
    ("t61_enc_singles", "T.61", "UTF-8", [U("¡£Ωß")], 64, "out"),
    ("t61_enc_spacing_marks", "T.61", "UTF-8", [U("ˇ˘˙˚˛˝")], 64, "out"),
    ("t61_enc_unwritable", "T.61", "UTF-8", [U("a€b")], 64, "out"),
    ("t61_enc_unwritable_ignore", "T.61//IGNORE", "UTF-8", [U("a€b")], 64, "out"),
    ("t61_enc_translit", "T.61//TRANSLIT", "UTF-8", [U("a€ “b”")], 64, "out"),
    ("t61_enc_tight", "T.61", "UTF-8", [U("aé")], 2, "out"),
    ("t61_enc_tag", "T.61", "UTF-8", [U("a\U000e0041b")], 64, "out"),
    ("t61_names", "UTF-8", "ISO-IR-103", [b"\xc2e"], 64, "out"),
    ("t61_names2", "T.618BIT", "UTF-8", [U("é")], 64, "out"),
    ("t61_names3", "UTF-8", "T.61-8BIT", [b"\xc2e"], 64, "out"),
    # ISO_6937
    ("iso6937_pairs", "UTF-8", "ISO_6937", [b"\xc3o\xc4n\xcfs\xcbc"], 64, "out"),
    ("iso6937_highs", "UTF-8", "ISO_6937", [b"\xa9\xd0\xd5\xdc\xac"], 64, "out"),
    ("iso6937_enc", "ISO_6937", "UTF-8", [U("ôñšç")], 64, "out"),
    ("iso6937_enc_specials", "ISO_6937", "UTF-8", [U("—‘’“”™Ω⅛←↓♪")], 64, "out"),
    ("iso6937_bad_letter_ignore", "UTF-8//IGNORE", "ISO_6937", [b"\xc2\x7f\xc2\x80e"], 64, "out"),
    ("iso6937_names", "UTF-8", "ISO6937", [b"\xc3o"], 64, "out"),
    ("iso6937_names2", "UTF-8", "ISO-IR-156", [b"\xc3o"], 64, "out"),
    ("iso6937_names3", "ISO_6937:1992", "UTF-8", [U("ô")], 64, "out"),
    # ISO_6937-2: three sequences read and never written.
    ("iso69372_decode_only", "UTF-8", "ISO_6937-2", [b"#$\xa6\xa8\xc4 ~"], 64, "out"),
    ("iso69372_enc", "ISO_6937-2", "UTF-8", [U("#¤~")], 64, "out"),
    ("iso69372_pairs", "UTF-8", "ISO_6937-2", [b"\xc2a\xc7z"], 64, "out"),
    ("iso69372_names", "UTF-8", "ISO-IR-90", [b"\xc2a"], 64, "out"),
    ("iso69372_names2", "CSISO90", "UTF-8", [U("á")], 64, "out"),
    # ANSI_X3.110
    ("ansi_pairs", "UTF-8", "ANSI_X3.110", [b"\xc2a\xc8u"], 64, "out"),
    ("ansi_enc_specials", "ANSI_X3.110", "UTF-8", [U("─│┼╱╲◢◣♪")], 64, "out"),
    ("ansi_enc", "ANSI_X3.110", "UTF-8", [U("äöü")], 64, "out"),
    ("ansi_names", "UTF-8", "NAPLPS", [b"\xc2a"], 64, "out"),
    ("ansi_names2", "UTF-8", "ISO-IR-99", [b"\xc2a"], 64, "out"),
    ("ansi_names3", "CSA_T500-1983", "UTF-8", [U("á")], 64, "out"),
]
