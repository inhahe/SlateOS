//! The capabilities the compiler and its dumper name, by their index in
//! `term.h` -- upstream's `CUR` macros (`init_3string` for
//! `CUR Strings[50]`). Written from `names.rs`; the test ties each to its
//! name.

/// Booleans.
pub mod b {
    /// `hc`: `hard_copy`.
    pub const HARD_COPY: usize = 7;
    /// `xon`: `xon_xoff`.
    pub const XON_XOFF: usize = 20;
    /// `OTbs`: `backspaces_with_bs`.
    pub const BACKSPACES_WITH_BS: usize = 37;
    /// `OTns`: `crt_no_scrolling`.
    pub const CRT_NO_SCROLLING: usize = 38;
    /// `OTnc`: `no_correctly_working_cr`.
    pub const NO_CORRECTLY_WORKING_CR: usize = 39;
    /// `OTNL`: `linefeed_is_newline`.
    pub const LINEFEED_IS_NEWLINE: usize = 41;
    /// `OTpt`: `has_hardware_tabs`.
    pub const HAS_HARDWARE_TABS: usize = 42;
    /// `OTxr`: `return_does_clr_eol`.
    pub const RETURN_DOES_CLR_EOL: usize = 43;
}

/// Numbers.
pub mod n {
    /// `it`: `init_tabs`.
    pub const INIT_TABS: usize = 1;
    /// `lines`: `lines`.
    pub const LINES: usize = 2;
    /// `xmc`: `magic_cookie_glitch`.
    pub const MAGIC_COOKIE_GLITCH: usize = 4;
    /// `wsl`: `width_status_line`.
    pub const WIDTH_STATUS_LINE: usize = 7;
    /// `lw`: `label_width`.
    pub const LABEL_WIDTH: usize = 10;
    /// `OTug`: `magic_cookie_glitch_ul`.
    pub const MAGIC_COOKIE_GLITCH_UL: usize = 33;
    /// `OTdC`: `carriage_return_delay`.
    pub const CARRIAGE_RETURN_DELAY: usize = 34;
    /// `OTdN`: `new_line_delay`.
    pub const NEW_LINE_DELAY: usize = 35;
    /// `OTdB`: `backspace_delay`.
    pub const BACKSPACE_DELAY: usize = 36;
    /// `OTdT`: `horizontal_tab_delay`.
    pub const HORIZONTAL_TAB_DELAY: usize = 37;
}

/// Strings.
pub mod s {
    /// `bel`: `bell`.
    pub const BELL: usize = 1;
    /// `cr`: `carriage_return`.
    pub const CARRIAGE_RETURN: usize = 2;
    /// `cud1`: `cursor_down`.
    pub const CURSOR_DOWN: usize = 11;
    /// `cub1`: `cursor_left`.
    pub const CURSOR_LEFT: usize = 14;
    /// `smacs`: `enter_alt_charset_mode`.
    pub const ENTER_ALT_CHARSET_MODE: usize = 25;
    /// `smcup`: `enter_ca_mode`.
    pub const ENTER_CA_MODE: usize = 28;
    /// `smir`: `enter_insert_mode`.
    pub const ENTER_INSERT_MODE: usize = 31;
    /// `smul`: `enter_underline_mode`.
    pub const ENTER_UNDERLINE_MODE: usize = 36;
    /// `rmacs`: `exit_alt_charset_mode`.
    pub const EXIT_ALT_CHARSET_MODE: usize = 38;
    /// `sgr0`: `exit_attribute_mode`.
    pub const EXIT_ATTRIBUTE_MODE: usize = 39;
    /// `rmcup`: `exit_ca_mode`.
    pub const EXIT_CA_MODE: usize = 40;
    /// `rmir`: `exit_insert_mode`.
    pub const EXIT_INSERT_MODE: usize = 42;
    /// `is1`: `init_1string`.
    pub const INIT_1STRING: usize = 48;
    /// `is2`: `init_2string`.
    pub const INIT_2STRING: usize = 49;
    /// `is3`: `init_3string`.
    pub const INIT_3STRING: usize = 50;
    /// `ich1`: `insert_character`.
    pub const INSERT_CHARACTER: usize = 52;
    /// `kbs`: `key_backspace`.
    pub const KEY_BACKSPACE: usize = 55;
    /// `kcud1`: `key_down`.
    pub const KEY_DOWN: usize = 61;
    /// `kf0`: `key_f0`.
    pub const KEY_F0: usize = 65;
    /// `kf9`: `key_f9`.
    pub const KEY_F9: usize = 75;
    /// `kich1`: `key_ic`.
    pub const KEY_IC: usize = 77;
    /// `kcub1`: `key_left`.
    pub const KEY_LEFT: usize = 79;
    /// `rmkx`: `keypad_local`.
    pub const KEYPAD_LOCAL: usize = 88;
    /// `smkx`: `keypad_xmit`.
    pub const KEYPAD_XMIT: usize = 89;
    /// `nel`: `newline`.
    pub const NEWLINE: usize = 103;
    /// `ich`: `parm_ich`.
    pub const PARM_ICH: usize = 108;
    /// `rs1`: `reset_1string`.
    pub const RESET_1STRING: usize = 122;
    /// `rs2`: `reset_2string`.
    pub const RESET_2STRING: usize = 123;
    /// `rs3`: `reset_3string`.
    pub const RESET_3STRING: usize = 124;
    /// `ind`: `scroll_forward`.
    pub const SCROLL_FORWARD: usize = 129;
    /// `sgr`: `set_attributes`.
    pub const SET_ATTRIBUTES: usize = 131;
    /// `ht`: `tab`.
    pub const TAB: usize = 134;
    /// `mc5p`: `prtr_non`.
    pub const PRTR_NON: usize = 144;
    /// `acsc`: `acs_chars`.
    pub const ACS_CHARS: usize = 146;
    /// `pln`: `plab_norm`.
    pub const PLAB_NORM: usize = 147;
    /// `smln`: `label_on`.
    pub const LABEL_ON: usize = 156;
    /// `rmln`: `label_off`.
    pub const LABEL_OFF: usize = 157;
    /// `kIC`: `key_sic`.
    pub const KEY_SIC: usize = 200;
    /// `kf11`: `key_f11`.
    pub const KEY_F11: usize = 216;
    /// `kf63`: `key_f63`.
    pub const KEY_F63: usize = 268;
    /// `OTi2`: `termcap_init2`.
    pub const TERMCAP_INIT2: usize = 394;
    /// `OTrs`: `termcap_reset`.
    pub const TERMCAP_RESET: usize = 395;
    /// `OTnl`: `linefeed_if_not_lf`.
    pub const LINEFEED_IF_NOT_LF: usize = 396;
    /// `OTbc`: `backspace_if_not_bs`.
    pub const BACKSPACE_IF_NOT_BS: usize = 397;
    /// `OTko`: `other_non_function_keys`.
    pub const OTHER_NON_FUNCTION_KEYS: usize = 398;
    /// `OTG2`: `acs_ulcorner`.
    pub const ACS_ULCORNER: usize = 400;
    /// `OTG3`: `acs_llcorner`.
    pub const ACS_LLCORNER: usize = 401;
    /// `OTG1`: `acs_urcorner`.
    pub const ACS_URCORNER: usize = 402;
    /// `OTG4`: `acs_lrcorner`.
    pub const ACS_LRCORNER: usize = 403;
    /// `OTGR`: `acs_ltee`.
    pub const ACS_LTEE: usize = 404;
    /// `OTGL`: `acs_rtee`.
    pub const ACS_RTEE: usize = 405;
    /// `OTGU`: `acs_btee`.
    pub const ACS_BTEE: usize = 406;
    /// `OTGD`: `acs_ttee`.
    pub const ACS_TTEE: usize = 407;
    /// `OTGH`: `acs_hline`.
    pub const ACS_HLINE: usize = 408;
    /// `OTGV`: `acs_vline`.
    pub const ACS_VLINE: usize = 409;
    /// `OTGC`: `acs_plus`.
    pub const ACS_PLUS: usize = 410;
    /// `meml`: `memory_lock`.
    pub const MEMORY_LOCK: usize = 411;
    /// `memu`: `memory_unlock`.
    pub const MEMORY_UNLOCK: usize = 412;
    /// `box1`: `box_chars_1`.
    pub const BOX_CHARS_1: usize = 413;
}

#[cfg(test)]
mod tests {
    use crate::names;

    #[test]
    fn every_index_is_the_capability_it_names() {
        assert_eq!(
            names::BOOLFNAMES.get(super::b::HARD_COPY),
            Some(&"hard_copy")
        );
        assert_eq!(names::BOOLFNAMES.get(super::b::XON_XOFF), Some(&"xon_xoff"));
        assert_eq!(
            names::BOOLFNAMES.get(super::b::BACKSPACES_WITH_BS),
            Some(&"backspaces_with_bs")
        );
        assert_eq!(
            names::BOOLFNAMES.get(super::b::CRT_NO_SCROLLING),
            Some(&"crt_no_scrolling")
        );
        assert_eq!(
            names::BOOLFNAMES.get(super::b::NO_CORRECTLY_WORKING_CR),
            Some(&"no_correctly_working_cr")
        );
        assert_eq!(
            names::BOOLFNAMES.get(super::b::LINEFEED_IS_NEWLINE),
            Some(&"linefeed_is_newline")
        );
        assert_eq!(
            names::BOOLFNAMES.get(super::b::HAS_HARDWARE_TABS),
            Some(&"has_hardware_tabs")
        );
        assert_eq!(
            names::BOOLFNAMES.get(super::b::RETURN_DOES_CLR_EOL),
            Some(&"return_does_clr_eol")
        );
        assert_eq!(
            names::NUMFNAMES.get(super::n::INIT_TABS),
            Some(&"init_tabs")
        );
        assert_eq!(names::NUMFNAMES.get(super::n::LINES), Some(&"lines"));
        assert_eq!(
            names::NUMFNAMES.get(super::n::MAGIC_COOKIE_GLITCH),
            Some(&"magic_cookie_glitch")
        );
        assert_eq!(
            names::NUMFNAMES.get(super::n::WIDTH_STATUS_LINE),
            Some(&"width_status_line")
        );
        assert_eq!(
            names::NUMFNAMES.get(super::n::LABEL_WIDTH),
            Some(&"label_width")
        );
        assert_eq!(
            names::NUMFNAMES.get(super::n::MAGIC_COOKIE_GLITCH_UL),
            Some(&"magic_cookie_glitch_ul")
        );
        assert_eq!(
            names::NUMFNAMES.get(super::n::CARRIAGE_RETURN_DELAY),
            Some(&"carriage_return_delay")
        );
        assert_eq!(
            names::NUMFNAMES.get(super::n::NEW_LINE_DELAY),
            Some(&"new_line_delay")
        );
        assert_eq!(
            names::NUMFNAMES.get(super::n::BACKSPACE_DELAY),
            Some(&"backspace_delay")
        );
        assert_eq!(
            names::NUMFNAMES.get(super::n::HORIZONTAL_TAB_DELAY),
            Some(&"horizontal_tab_delay")
        );
        assert_eq!(names::STRFNAMES.get(super::s::BELL), Some(&"bell"));
        assert_eq!(
            names::STRFNAMES.get(super::s::CARRIAGE_RETURN),
            Some(&"carriage_return")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::CURSOR_DOWN),
            Some(&"cursor_down")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::CURSOR_LEFT),
            Some(&"cursor_left")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::ENTER_ALT_CHARSET_MODE),
            Some(&"enter_alt_charset_mode")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::ENTER_CA_MODE),
            Some(&"enter_ca_mode")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::ENTER_INSERT_MODE),
            Some(&"enter_insert_mode")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::ENTER_UNDERLINE_MODE),
            Some(&"enter_underline_mode")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::EXIT_ALT_CHARSET_MODE),
            Some(&"exit_alt_charset_mode")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::EXIT_ATTRIBUTE_MODE),
            Some(&"exit_attribute_mode")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::EXIT_CA_MODE),
            Some(&"exit_ca_mode")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::EXIT_INSERT_MODE),
            Some(&"exit_insert_mode")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::INIT_1STRING),
            Some(&"init_1string")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::INIT_2STRING),
            Some(&"init_2string")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::INIT_3STRING),
            Some(&"init_3string")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::INSERT_CHARACTER),
            Some(&"insert_character")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::KEY_BACKSPACE),
            Some(&"key_backspace")
        );
        assert_eq!(names::STRFNAMES.get(super::s::KEY_DOWN), Some(&"key_down"));
        assert_eq!(names::STRFNAMES.get(super::s::KEY_F0), Some(&"key_f0"));
        assert_eq!(names::STRFNAMES.get(super::s::KEY_F9), Some(&"key_f9"));
        assert_eq!(names::STRFNAMES.get(super::s::KEY_IC), Some(&"key_ic"));
        assert_eq!(names::STRFNAMES.get(super::s::KEY_LEFT), Some(&"key_left"));
        assert_eq!(
            names::STRFNAMES.get(super::s::KEYPAD_LOCAL),
            Some(&"keypad_local")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::KEYPAD_XMIT),
            Some(&"keypad_xmit")
        );
        assert_eq!(names::STRFNAMES.get(super::s::NEWLINE), Some(&"newline"));
        assert_eq!(names::STRFNAMES.get(super::s::PARM_ICH), Some(&"parm_ich"));
        assert_eq!(
            names::STRFNAMES.get(super::s::RESET_1STRING),
            Some(&"reset_1string")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::RESET_2STRING),
            Some(&"reset_2string")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::RESET_3STRING),
            Some(&"reset_3string")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::SCROLL_FORWARD),
            Some(&"scroll_forward")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::SET_ATTRIBUTES),
            Some(&"set_attributes")
        );
        assert_eq!(names::STRFNAMES.get(super::s::TAB), Some(&"tab"));
        assert_eq!(names::STRFNAMES.get(super::s::PRTR_NON), Some(&"prtr_non"));
        assert_eq!(
            names::STRFNAMES.get(super::s::ACS_CHARS),
            Some(&"acs_chars")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::PLAB_NORM),
            Some(&"plab_norm")
        );
        assert_eq!(names::STRFNAMES.get(super::s::LABEL_ON), Some(&"label_on"));
        assert_eq!(
            names::STRFNAMES.get(super::s::LABEL_OFF),
            Some(&"label_off")
        );
        assert_eq!(names::STRFNAMES.get(super::s::KEY_SIC), Some(&"key_sic"));
        assert_eq!(names::STRFNAMES.get(super::s::KEY_F11), Some(&"key_f11"));
        assert_eq!(names::STRFNAMES.get(super::s::KEY_F63), Some(&"key_f63"));
        assert_eq!(
            names::STRFNAMES.get(super::s::TERMCAP_INIT2),
            Some(&"termcap_init2")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::TERMCAP_RESET),
            Some(&"termcap_reset")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::LINEFEED_IF_NOT_LF),
            Some(&"linefeed_if_not_lf")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::BACKSPACE_IF_NOT_BS),
            Some(&"backspace_if_not_bs")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::OTHER_NON_FUNCTION_KEYS),
            Some(&"other_non_function_keys")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::ACS_ULCORNER),
            Some(&"acs_ulcorner")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::ACS_LLCORNER),
            Some(&"acs_llcorner")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::ACS_URCORNER),
            Some(&"acs_urcorner")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::ACS_LRCORNER),
            Some(&"acs_lrcorner")
        );
        assert_eq!(names::STRFNAMES.get(super::s::ACS_LTEE), Some(&"acs_ltee"));
        assert_eq!(names::STRFNAMES.get(super::s::ACS_RTEE), Some(&"acs_rtee"));
        assert_eq!(names::STRFNAMES.get(super::s::ACS_BTEE), Some(&"acs_btee"));
        assert_eq!(names::STRFNAMES.get(super::s::ACS_TTEE), Some(&"acs_ttee"));
        assert_eq!(
            names::STRFNAMES.get(super::s::ACS_HLINE),
            Some(&"acs_hline")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::ACS_VLINE),
            Some(&"acs_vline")
        );
        assert_eq!(names::STRFNAMES.get(super::s::ACS_PLUS), Some(&"acs_plus"));
        assert_eq!(
            names::STRFNAMES.get(super::s::MEMORY_LOCK),
            Some(&"memory_lock")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::MEMORY_UNLOCK),
            Some(&"memory_unlock")
        );
        assert_eq!(
            names::STRFNAMES.get(super::s::BOX_CHARS_1),
            Some(&"box_chars_1")
        );
    }
}
