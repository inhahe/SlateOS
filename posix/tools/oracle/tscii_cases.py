"""TSCII's cases for the iconv oracle (see cp125x_harness.py, whose generator
and runner tscii_harness.py reuses): every byte, and every byte after each
state the decoder keeps characters back in; every Tamil character after each
consonant the encoder keeps back; the glyph orders TSCII and Unicode disagree
on; and the characters kept back -- across calls, through a full output
buffer, and at a reset, including the three places glibc gets them wrong
(iconv.rs's TSCII section, and `the_tscii_departures` in its tests)."""

U = lambda s: s.encode("utf-8")  # noqa: E731

KA, SA, HA, SSA, RA, JA = "க", "ஸ", "ஹ", "ஷ", "ர", "ஜ"
VIRAMA, U_, UU, I_, II = "்", "ு", "ூ", "ி", "ீ"
E, EE, AI, O, OO, AU = "ெ", "ே", "ை", "ொ", "ோ", "ௌ"
TTA = "ட"
TAMIL = [chr(c) for c in range(0x0B80, 0x0C00)]


def after_each_byte(prefix: bytes) -> bytes:
    """`prefix`, then each byte 0x80-0xFF, then `prefix` again before the next."""
    return b"".join(prefix + bytes([b]) for b in range(0x80, 0x100))


def after_each_tamil(prefix: str) -> bytes:
    """`prefix`, then each character U+0B80-U+0BFF, `prefix` again before the next."""
    return U("".join(prefix + c for c in TAMIL))


CASES = [
    # TSCII -> UTF-8
    ("ts_ascii", "UTF-8", "TSCII", [b"Tamil 1"], 64, "out"),
    ("ts_every_byte", "UTF-8//IGNORE", "TSCII", [bytes(range(0x80, 0x100))], 1024, "out"),
    ("ts_every_byte_wchar", "WCHAR_T//IGNORE", "TSCII", [bytes(range(0x80, 0x100))], 2048, "out"),
    ("ts_consonant", "UTF-8", "TSCII", [b"\xb8"], 64, "out"),
    ("ts_consonant_u", "UTF-8", "TSCII", [b"\xcc\xdc"], 64, "out"),
    ("ts_consonant_virama", "UTF-8", "TSCII", [b"\xec"], 64, "out"),
    ("ts_vowel_sign_before", "UTF-8", "TSCII", [b"\xa6\xb8"], 64, "out"),
    ("ts_vowel_sign_e_o", "UTF-8", "TSCII", [b"\xa6\xb8\xa1"], 64, "out"),
    ("ts_vowel_sign_ee_oo", "UTF-8", "TSCII", [b"\xa7\xb8\xa1"], 64, "out"),
    ("ts_vowel_sign_ee_au", "UTF-8", "TSCII", [b"\xa7\xb8\xaa"], 64, "out"),
    ("ts_vowel_sign_ai", "UTF-8", "TSCII", [b"\xa8\xb8"], 64, "out"),
    ("ts_vowel_sign_alone", "UTF-8", "TSCII", [b"\xa6"], 64, "out"),
    ("ts_vowel_sign_alone_null", "UTF-8", "TSCII", [b"\xa6"], 64, "null"),
    ("ts_vowel_sign_before_ascii", "UTF-8", "TSCII", [b"\xa6a"], 64, "out"),
    ("ts_vowel_sign_two_consonants", "UTF-8", "TSCII", [b"\xa6\xb8\xb9"], 64, "out"),
    ("ts_vowel_sign_twice", "UTF-8", "TSCII", [b"\xa6\xb8\xa6"], 64, "out"),
    ("ts_vowel_e_aa_no_consonant", "UTF-8", "TSCII", [b"\xa6\xa1"], 64, "out"),
    ("ts_vowel_ee_au_no_consonant", "UTF-8", "TSCII", [b"\xa7\xaa"], 64, "out"),
    ("ts_vowel_across_calls", "UTF-8", "TSCII", [b"\xa6", b"\xb8", b"\xa1"], 64, "out"),
    ("ts_sa_virama_u", "UTF-8", "TSCII", [b"\x8a\xa4"], 64, "out"),
    ("ts_ha_virama_uu", "UTF-8", "TSCII", [b"\x8b\xa5"], 64, "out"),
    ("ts_sa_virama_u_across_calls", "UTF-8", "TSCII", [b"\x8a", b"\xa4"], 64, "out"),
    ("ts_sa_virama_alone", "UTF-8", "TSCII", [b"\x8a"], 64, "out"),
    ("ts_sa_virama_then_a", "UTF-8", "TSCII", [b"\x8aa"], 64, "out"),
    ("ts_sa_virama_then_e", "UTF-8", "TSCII", [b"\x8a\xa6\xb8"], 64, "out"),
    ("ts_sri", "UTF-8", "TSCII", [b"\x82"], 64, "out"),
    ("ts_kssa", "UTF-8", "TSCII", [b"\x87"], 64, "out"),
    ("ts_kssa_virama", "UTF-8", "TSCII", [b"\x8c"], 64, "out"),
    ("ts_two_chars", "UTF-8", "TSCII", [b"\x99\x88"], 64, "out"),
    ("ts_quotes_copyright", "UTF-8", "TSCII", [b"\x91\x92\x93\x94\xa9"], 64, "out"),
    ("ts_unmapped", "UTF-8", "TSCII", [b"a\xa0b"], 64, "out"),
    ("ts_unmapped_ignore", "UTF-8//IGNORE", "TSCII", [b"a\xa0\xffb"], 64, "out"),
    ("ts_unmapped_after_vowel_sign", "UTF-8", "TSCII", [b"\xa6\xa0"], 64, "out"),
    # Every byte after each state that keeps a character back.
    ("ts_after_e", "UTF-8//IGNORE", "TSCII", [after_each_byte(b"\xa6")], 8192, "out"),
    ("ts_after_ee", "UTF-8//IGNORE", "TSCII", [after_each_byte(b"\xa7")], 8192, "out"),
    ("ts_after_ai", "UTF-8//IGNORE", "TSCII", [after_each_byte(b"\xa8")], 8192, "out"),
    ("ts_after_e_ka", "UTF-8//IGNORE", "TSCII", [after_each_byte(b"\xa6\xb8")], 8192, "out"),
    ("ts_after_ee_ka", "UTF-8//IGNORE", "TSCII", [after_each_byte(b"\xa7\xb8")], 8192, "out"),
    ("ts_after_ai_ka", "UTF-8//IGNORE", "TSCII", [after_each_byte(b"\xa8\xb8")], 8192, "out"),
    ("ts_after_sa_virama", "UTF-8//IGNORE", "TSCII", [after_each_byte(b"\x8a")], 8192, "out"),
    ("ts_after_ha_virama", "UTF-8//IGNORE", "TSCII", [after_each_byte(b"\x8b")], 8192, "out"),
    # Through a full output buffer: the second character, or the rest of
    # 0x82's four, kept back for the next call.
    ("ts_pair_tight_wchar", "WCHAR_T", "TSCII", [b"\x99", b""], 4, "out"),
    ("ts_sri_tight_wchar4", "WCHAR_T", "TSCII", [b"\x82", b"", b"", b""], 4, "out"),
    ("ts_sri_tight_wchar8", "WCHAR_T", "TSCII", [b"\x82", b""], 8, "out"),
    ("ts_sri_tight_wchar12", "WCHAR_T", "TSCII", [b"\x82", b""], 12, "out"),
    ("ts_kssa_tight_wchar4", "WCHAR_T", "TSCII", [b"\x87", b"", b""], 4, "out"),
    ("ts_kssa_virama_tight_wchar4", "WCHAR_T", "TSCII", [b"\x8c", b"", b"", b""], 4, "out"),
    ("ts_sri_reset_tight", "WCHAR_T", "TSCII", [b"\x82"], 4, "out"),
    ("ts_sri_reset_tight_utf8", "UTF-8", "TSCII", [b"\x82"], 3, "out"),
    ("ts_vowel_reset_room", "WCHAR_T", "TSCII", [b"\xa6"], 4, "out"),
    ("ts_vowel_consonant_reset", "UTF-8", "TSCII", [b"\xa6\xb8"], 3, "out"),
    ("ts_vowel_consonant_reset_room", "UTF-8", "TSCII", [b"\xa6\xb8"], 64, "out"),
    ("ts_vowel_consonant_reset_tight", "UTF-8", "TSCII", [b"\xa6\xb8"], 5, "out"),
    # ...and written at the next call with input: where glibc's loop writes
    # the first of them again for each of the others.
    ("ts_sri_then_a_wchar8", "WCHAR_T", "TSCII", [b"\x82", b"a", b"a"], 8, "out"),
    ("ts_sri_then_a_utf8_6", "UTF-8", "TSCII", [b"\x82", b"a", b"a"], 6, "out"),
    ("ts_sri_then_a_wchar4", "WCHAR_T", "TSCII", [b"\x82", b"a", b"a", b"a", b"a"], 4, "out"),
    # UTF-8 -> TSCII
    ("ts_enc_ascii", "TSCII", "UTF-8", [U("Tamil 1")], 64, "out"),
    ("ts_enc_tamil_block", "TSCII//IGNORE", "UTF-8", [U("".join(TAMIL))], 1024, "out"),
    ("ts_enc_consonant_alone", "TSCII", "UTF-8", [U(KA)], 64, "out"),
    ("ts_enc_consonant_alone_null", "TSCII", "UTF-8", [U(KA)], 64, "null"),
    ("ts_enc_consonant_then_ascii", "TSCII", "UTF-8", [U(KA + "a")], 64, "out"),
    ("ts_enc_consonant_u", "TSCII", "UTF-8", [U(KA + U_ + KA + UU)], 64, "out"),
    ("ts_enc_consonant_virama", "TSCII", "UTF-8", [U(KA + VIRAMA)], 64, "out"),
    ("ts_enc_consonant_signs_before", "TSCII", "UTF-8", [U(KA + E + KA + EE + KA + AI)], 64, "out"),
    ("ts_enc_consonant_o_oo_au", "TSCII", "UTF-8", [U(KA + O + KA + OO + KA + AU)], 64, "out"),
    ("ts_enc_tta_i_ii", "TSCII", "UTF-8", [U(TTA + I_ + TTA + II)], 64, "out"),
    ("ts_enc_sa_u", "TSCII", "UTF-8", [U(SA + U_)], 64, "out"),
    ("ts_enc_ha_uu", "TSCII", "UTF-8", [U(HA + UU)], 64, "out"),
    ("ts_enc_sa_virama", "TSCII", "UTF-8", [U(SA + VIRAMA)], 64, "out"),
    ("ts_enc_ssa_virama", "TSCII", "UTF-8", [U(SSA + VIRAMA)], 64, "out"),
    ("ts_enc_kssa", "TSCII", "UTF-8", [U(KA + VIRAMA + SSA)], 64, "out"),
    ("ts_enc_kssa_virama", "TSCII", "UTF-8", [U(KA + VIRAMA + SSA + VIRAMA)], 64, "out"),
    ("ts_enc_sri", "TSCII", "UTF-8", [U(SA + VIRAMA + RA + II)], 64, "out"),
    ("ts_enc_sa_virama_ra", "TSCII", "UTF-8", [U(SA + VIRAMA + RA)], 64, "out"),
    ("ts_enc_sa_virama_ra_a", "TSCII", "UTF-8", [U(SA + VIRAMA + RA + "a")], 64, "out"),
    ("ts_enc_o_alone", "TSCII", "UTF-8", [U(O + OO + AU)], 64, "out"),
    ("ts_enc_digits_quotes", "TSCII", "UTF-8", [U("௧௰‘’“”©")], 64, "out"),
    ("ts_enc_unwritable", "TSCII", "UTF-8", [U("a€b")], 64, "out"),
    ("ts_enc_unwritable_ignore", "TSCII//IGNORE", "UTF-8", [U("a€b")], 64, "out"),
    ("ts_enc_translit", "TSCII//TRANSLIT", "UTF-8", [U("a€b")], 64, "out"),
    ("ts_enc_translit_after_consonant", "TSCII//TRANSLIT", "UTF-8", [U(KA + "€" + U_)], 64, "out"),
    ("ts_enc_tag", "TSCII", "UTF-8", [U("a\U000e0041b")], 64, "out"),
    ("ts_enc_tag_after_consonant", "TSCII", "UTF-8", [U(KA + "\U000e0041" + U_)], 64, "out"),
    ("ts_enc_tamil_unwritable", "TSCII", "UTF-8", [U("a஀b")], 64, "out"),
    ("ts_enc_unwritable_after_consonant", "TSCII", "UTF-8", [U(KA + "஀")], 64, "out"),
    ("ts_enc_across_calls", "TSCII", "UTF-8", [U(KA), U(U_)], 64, "out"),
    ("ts_enc_sri_across_calls", "TSCII", "UTF-8", [U(SA), U(VIRAMA), U(RA), U(II)], 64, "out"),
    ("ts_enc_tight_sign_before", "TSCII", "UTF-8", [U(KA + E)], 1, "out"),
    ("ts_enc_tight_o", "TSCII", "UTF-8", [U(KA + O)], 2, "out"),
    ("ts_enc_tight_two_kept", "TSCII", "UTF-8", [U(SA + VIRAMA + RA + "a")], 1, "out"),
    ("ts_enc_reset_tight", "TSCII", "UTF-8", [U(SA + VIRAMA + RA)], 1, "out"),
    ("ts_enc_from_wchar", "TSCII", "WCHAR_T", ["கு".encode("utf-32-le")], 64, "out"),
    # Every Tamil character after each consonant, or consonant and VIRAMA,
    # the encoder keeps back.
    ("ts_enc_after_ka", "TSCII//IGNORE", "UTF-8", [after_each_tamil(KA)], 4096, "out"),
    ("ts_enc_after_tta", "TSCII//IGNORE", "UTF-8", [after_each_tamil(TTA)], 4096, "out"),
    ("ts_enc_after_ja", "TSCII//IGNORE", "UTF-8", [after_each_tamil(JA)], 4096, "out"),
    ("ts_enc_after_ssa", "TSCII//IGNORE", "UTF-8", [after_each_tamil(SSA)], 4096, "out"),
    ("ts_enc_after_sa", "TSCII//IGNORE", "UTF-8", [after_each_tamil(SA)], 4096, "out"),
    ("ts_enc_after_ha", "TSCII//IGNORE", "UTF-8", [after_each_tamil(HA)], 4096, "out"),
    ("ts_enc_after_ka_virama", "TSCII//IGNORE", "UTF-8", [after_each_tamil(KA + VIRAMA)], 4096, "out"),
    ("ts_enc_after_sa_virama", "TSCII//IGNORE", "UTF-8", [after_each_tamil(SA + VIRAMA)], 4096, "out"),
    ("ts_enc_after_kssa", "TSCII//IGNORE", "UTF-8", [after_each_tamil(KA + VIRAMA + SSA)], 4096, "out"),
    ("ts_enc_after_sa_virama_ra", "TSCII//IGNORE", "UTF-8", [after_each_tamil(SA + VIRAMA + RA)], 4096, "out"),
]
