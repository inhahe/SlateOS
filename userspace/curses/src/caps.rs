//! Every terminfo capability's index, as ncurses 6.4's generated
//! `term.h` numbers them: `CUR Booleans[i]`, `CUR Numbers[i]`,
//! `CUR Strings[i]`. Generated from that file; the names are its
//! macros', upper-cased.

/// The boolean capabilities.
pub mod boolean {
    /// `auto_left_margin`.
    pub const AUTO_LEFT_MARGIN: usize = 0;
    /// `auto_right_margin`.
    pub const AUTO_RIGHT_MARGIN: usize = 1;
    /// `no_esc_ctlc`.
    pub const NO_ESC_CTLC: usize = 2;
    /// `ceol_standout_glitch`.
    pub const CEOL_STANDOUT_GLITCH: usize = 3;
    /// `eat_newline_glitch`.
    pub const EAT_NEWLINE_GLITCH: usize = 4;
    /// `erase_overstrike`.
    pub const ERASE_OVERSTRIKE: usize = 5;
    /// `generic_type`.
    pub const GENERIC_TYPE: usize = 6;
    /// `hard_copy`.
    pub const HARD_COPY: usize = 7;
    /// `has_meta_key`.
    pub const HAS_META_KEY: usize = 8;
    /// `has_status_line`.
    pub const HAS_STATUS_LINE: usize = 9;
    /// `insert_null_glitch`.
    pub const INSERT_NULL_GLITCH: usize = 10;
    /// `memory_above`.
    pub const MEMORY_ABOVE: usize = 11;
    /// `memory_below`.
    pub const MEMORY_BELOW: usize = 12;
    /// `move_insert_mode`.
    pub const MOVE_INSERT_MODE: usize = 13;
    /// `move_standout_mode`.
    pub const MOVE_STANDOUT_MODE: usize = 14;
    /// `over_strike`.
    pub const OVER_STRIKE: usize = 15;
    /// `status_line_esc_ok`.
    pub const STATUS_LINE_ESC_OK: usize = 16;
    /// `dest_tabs_magic_smso`.
    pub const DEST_TABS_MAGIC_SMSO: usize = 17;
    /// `tilde_glitch`.
    pub const TILDE_GLITCH: usize = 18;
    /// `transparent_underline`.
    pub const TRANSPARENT_UNDERLINE: usize = 19;
    /// `xon_xoff`.
    pub const XON_XOFF: usize = 20;
    /// `needs_xon_xoff`.
    pub const NEEDS_XON_XOFF: usize = 21;
    /// `prtr_silent`.
    pub const PRTR_SILENT: usize = 22;
    /// `hard_cursor`.
    pub const HARD_CURSOR: usize = 23;
    /// `non_rev_rmcup`.
    pub const NON_REV_RMCUP: usize = 24;
    /// `no_pad_char`.
    pub const NO_PAD_CHAR: usize = 25;
    /// `non_dest_scroll_region`.
    pub const NON_DEST_SCROLL_REGION: usize = 26;
    /// `can_change`.
    pub const CAN_CHANGE: usize = 27;
    /// `back_color_erase`.
    pub const BACK_COLOR_ERASE: usize = 28;
    /// `hue_lightness_saturation`.
    pub const HUE_LIGHTNESS_SATURATION: usize = 29;
    /// `col_addr_glitch`.
    pub const COL_ADDR_GLITCH: usize = 30;
    /// `cr_cancels_micro_mode`.
    pub const CR_CANCELS_MICRO_MODE: usize = 31;
    /// `has_print_wheel`.
    pub const HAS_PRINT_WHEEL: usize = 32;
    /// `row_addr_glitch`.
    pub const ROW_ADDR_GLITCH: usize = 33;
    /// `semi_auto_right_margin`.
    pub const SEMI_AUTO_RIGHT_MARGIN: usize = 34;
    /// `cpi_changes_res`.
    pub const CPI_CHANGES_RES: usize = 35;
    /// `lpi_changes_res`.
    pub const LPI_CHANGES_RES: usize = 36;
    /// `backspaces_with_bs`.
    pub const BACKSPACES_WITH_BS: usize = 37;
    /// `crt_no_scrolling`.
    pub const CRT_NO_SCROLLING: usize = 38;
    /// `no_correctly_working_cr`.
    pub const NO_CORRECTLY_WORKING_CR: usize = 39;
    /// `gnu_has_meta_key`.
    pub const GNU_HAS_META_KEY: usize = 40;
    /// `linefeed_is_newline`.
    pub const LINEFEED_IS_NEWLINE: usize = 41;
    /// `has_hardware_tabs`.
    pub const HAS_HARDWARE_TABS: usize = 42;
    /// `return_does_clr_eol`.
    pub const RETURN_DOES_CLR_EOL: usize = 43;
}

/// The number capabilities.
pub mod number {
    /// `columns`.
    pub const COLUMNS: usize = 0;
    /// `init_tabs`.
    pub const INIT_TABS: usize = 1;
    /// `lines`.
    pub const LINES: usize = 2;
    /// `lines_of_memory`.
    pub const LINES_OF_MEMORY: usize = 3;
    /// `magic_cookie_glitch`.
    pub const MAGIC_COOKIE_GLITCH: usize = 4;
    /// `padding_baud_rate`.
    pub const PADDING_BAUD_RATE: usize = 5;
    /// `virtual_terminal`.
    pub const VIRTUAL_TERMINAL: usize = 6;
    /// `width_status_line`.
    pub const WIDTH_STATUS_LINE: usize = 7;
    /// `num_labels`.
    pub const NUM_LABELS: usize = 8;
    /// `label_height`.
    pub const LABEL_HEIGHT: usize = 9;
    /// `label_width`.
    pub const LABEL_WIDTH: usize = 10;
    /// `max_attributes`.
    pub const MAX_ATTRIBUTES: usize = 11;
    /// `maximum_windows`.
    pub const MAXIMUM_WINDOWS: usize = 12;
    /// `max_colors`.
    pub const MAX_COLORS: usize = 13;
    /// `max_pairs`.
    pub const MAX_PAIRS: usize = 14;
    /// `no_color_video`.
    pub const NO_COLOR_VIDEO: usize = 15;
    /// `buffer_capacity`.
    pub const BUFFER_CAPACITY: usize = 16;
    /// `dot_vert_spacing`.
    pub const DOT_VERT_SPACING: usize = 17;
    /// `dot_horz_spacing`.
    pub const DOT_HORZ_SPACING: usize = 18;
    /// `max_micro_address`.
    pub const MAX_MICRO_ADDRESS: usize = 19;
    /// `max_micro_jump`.
    pub const MAX_MICRO_JUMP: usize = 20;
    /// `micro_col_size`.
    pub const MICRO_COL_SIZE: usize = 21;
    /// `micro_line_size`.
    pub const MICRO_LINE_SIZE: usize = 22;
    /// `number_of_pins`.
    pub const NUMBER_OF_PINS: usize = 23;
    /// `output_res_char`.
    pub const OUTPUT_RES_CHAR: usize = 24;
    /// `output_res_line`.
    pub const OUTPUT_RES_LINE: usize = 25;
    /// `output_res_horz_inch`.
    pub const OUTPUT_RES_HORZ_INCH: usize = 26;
    /// `output_res_vert_inch`.
    pub const OUTPUT_RES_VERT_INCH: usize = 27;
    /// `print_rate`.
    pub const PRINT_RATE: usize = 28;
    /// `wide_char_size`.
    pub const WIDE_CHAR_SIZE: usize = 29;
    /// `buttons`.
    pub const BUTTONS: usize = 30;
    /// `bit_image_entwining`.
    pub const BIT_IMAGE_ENTWINING: usize = 31;
    /// `bit_image_type`.
    pub const BIT_IMAGE_TYPE: usize = 32;
    /// `magic_cookie_glitch_ul`.
    pub const MAGIC_COOKIE_GLITCH_UL: usize = 33;
    /// `carriage_return_delay`.
    pub const CARRIAGE_RETURN_DELAY: usize = 34;
    /// `new_line_delay`.
    pub const NEW_LINE_DELAY: usize = 35;
    /// `backspace_delay`.
    pub const BACKSPACE_DELAY: usize = 36;
    /// `horizontal_tab_delay`.
    pub const HORIZONTAL_TAB_DELAY: usize = 37;
    /// `number_of_function_keys`.
    pub const NUMBER_OF_FUNCTION_KEYS: usize = 38;
}

/// The string capabilities.
pub mod string {
    /// `back_tab`.
    pub const BACK_TAB: usize = 0;
    /// `bell`.
    pub const BELL: usize = 1;
    /// `carriage_return`.
    pub const CARRIAGE_RETURN: usize = 2;
    /// `change_scroll_region`.
    pub const CHANGE_SCROLL_REGION: usize = 3;
    /// `clear_all_tabs`.
    pub const CLEAR_ALL_TABS: usize = 4;
    /// `clear_screen`.
    pub const CLEAR_SCREEN: usize = 5;
    /// `clr_eol`.
    pub const CLR_EOL: usize = 6;
    /// `clr_eos`.
    pub const CLR_EOS: usize = 7;
    /// `column_address`.
    pub const COLUMN_ADDRESS: usize = 8;
    /// `command_character`.
    pub const COMMAND_CHARACTER: usize = 9;
    /// `cursor_address`.
    pub const CURSOR_ADDRESS: usize = 10;
    /// `cursor_down`.
    pub const CURSOR_DOWN: usize = 11;
    /// `cursor_home`.
    pub const CURSOR_HOME: usize = 12;
    /// `cursor_invisible`.
    pub const CURSOR_INVISIBLE: usize = 13;
    /// `cursor_left`.
    pub const CURSOR_LEFT: usize = 14;
    /// `cursor_mem_address`.
    pub const CURSOR_MEM_ADDRESS: usize = 15;
    /// `cursor_normal`.
    pub const CURSOR_NORMAL: usize = 16;
    /// `cursor_right`.
    pub const CURSOR_RIGHT: usize = 17;
    /// `cursor_to_ll`.
    pub const CURSOR_TO_LL: usize = 18;
    /// `cursor_up`.
    pub const CURSOR_UP: usize = 19;
    /// `cursor_visible`.
    pub const CURSOR_VISIBLE: usize = 20;
    /// `delete_character`.
    pub const DELETE_CHARACTER: usize = 21;
    /// `delete_line`.
    pub const DELETE_LINE: usize = 22;
    /// `dis_status_line`.
    pub const DIS_STATUS_LINE: usize = 23;
    /// `down_half_line`.
    pub const DOWN_HALF_LINE: usize = 24;
    /// `enter_alt_charset_mode`.
    pub const ENTER_ALT_CHARSET_MODE: usize = 25;
    /// `enter_blink_mode`.
    pub const ENTER_BLINK_MODE: usize = 26;
    /// `enter_bold_mode`.
    pub const ENTER_BOLD_MODE: usize = 27;
    /// `enter_ca_mode`.
    pub const ENTER_CA_MODE: usize = 28;
    /// `enter_delete_mode`.
    pub const ENTER_DELETE_MODE: usize = 29;
    /// `enter_dim_mode`.
    pub const ENTER_DIM_MODE: usize = 30;
    /// `enter_insert_mode`.
    pub const ENTER_INSERT_MODE: usize = 31;
    /// `enter_secure_mode`.
    pub const ENTER_SECURE_MODE: usize = 32;
    /// `enter_protected_mode`.
    pub const ENTER_PROTECTED_MODE: usize = 33;
    /// `enter_reverse_mode`.
    pub const ENTER_REVERSE_MODE: usize = 34;
    /// `enter_standout_mode`.
    pub const ENTER_STANDOUT_MODE: usize = 35;
    /// `enter_underline_mode`.
    pub const ENTER_UNDERLINE_MODE: usize = 36;
    /// `erase_chars`.
    pub const ERASE_CHARS: usize = 37;
    /// `exit_alt_charset_mode`.
    pub const EXIT_ALT_CHARSET_MODE: usize = 38;
    /// `exit_attribute_mode`.
    pub const EXIT_ATTRIBUTE_MODE: usize = 39;
    /// `exit_ca_mode`.
    pub const EXIT_CA_MODE: usize = 40;
    /// `exit_delete_mode`.
    pub const EXIT_DELETE_MODE: usize = 41;
    /// `exit_insert_mode`.
    pub const EXIT_INSERT_MODE: usize = 42;
    /// `exit_standout_mode`.
    pub const EXIT_STANDOUT_MODE: usize = 43;
    /// `exit_underline_mode`.
    pub const EXIT_UNDERLINE_MODE: usize = 44;
    /// `flash_screen`.
    pub const FLASH_SCREEN: usize = 45;
    /// `form_feed`.
    pub const FORM_FEED: usize = 46;
    /// `from_status_line`.
    pub const FROM_STATUS_LINE: usize = 47;
    /// `init_1string`.
    pub const INIT_1STRING: usize = 48;
    /// `init_2string`.
    pub const INIT_2STRING: usize = 49;
    /// `init_3string`.
    pub const INIT_3STRING: usize = 50;
    /// `init_file`.
    pub const INIT_FILE: usize = 51;
    /// `insert_character`.
    pub const INSERT_CHARACTER: usize = 52;
    /// `insert_line`.
    pub const INSERT_LINE: usize = 53;
    /// `insert_padding`.
    pub const INSERT_PADDING: usize = 54;
    /// `key_backspace`.
    pub const KEY_BACKSPACE: usize = 55;
    /// `key_catab`.
    pub const KEY_CATAB: usize = 56;
    /// `key_clear`.
    pub const KEY_CLEAR: usize = 57;
    /// `key_ctab`.
    pub const KEY_CTAB: usize = 58;
    /// `key_dc`.
    pub const KEY_DC: usize = 59;
    /// `key_dl`.
    pub const KEY_DL: usize = 60;
    /// `key_down`.
    pub const KEY_DOWN: usize = 61;
    /// `key_eic`.
    pub const KEY_EIC: usize = 62;
    /// `key_eol`.
    pub const KEY_EOL: usize = 63;
    /// `key_eos`.
    pub const KEY_EOS: usize = 64;
    /// `key_f0`.
    pub const KEY_F0: usize = 65;
    /// `key_f1`.
    pub const KEY_F1: usize = 66;
    /// `key_f10`.
    pub const KEY_F10: usize = 67;
    /// `key_f2`.
    pub const KEY_F2: usize = 68;
    /// `key_f3`.
    pub const KEY_F3: usize = 69;
    /// `key_f4`.
    pub const KEY_F4: usize = 70;
    /// `key_f5`.
    pub const KEY_F5: usize = 71;
    /// `key_f6`.
    pub const KEY_F6: usize = 72;
    /// `key_f7`.
    pub const KEY_F7: usize = 73;
    /// `key_f8`.
    pub const KEY_F8: usize = 74;
    /// `key_f9`.
    pub const KEY_F9: usize = 75;
    /// `key_home`.
    pub const KEY_HOME: usize = 76;
    /// `key_ic`.
    pub const KEY_IC: usize = 77;
    /// `key_il`.
    pub const KEY_IL: usize = 78;
    /// `key_left`.
    pub const KEY_LEFT: usize = 79;
    /// `key_ll`.
    pub const KEY_LL: usize = 80;
    /// `key_npage`.
    pub const KEY_NPAGE: usize = 81;
    /// `key_ppage`.
    pub const KEY_PPAGE: usize = 82;
    /// `key_right`.
    pub const KEY_RIGHT: usize = 83;
    /// `key_sf`.
    pub const KEY_SF: usize = 84;
    /// `key_sr`.
    pub const KEY_SR: usize = 85;
    /// `key_stab`.
    pub const KEY_STAB: usize = 86;
    /// `key_up`.
    pub const KEY_UP: usize = 87;
    /// `keypad_local`.
    pub const KEYPAD_LOCAL: usize = 88;
    /// `keypad_xmit`.
    pub const KEYPAD_XMIT: usize = 89;
    /// `lab_f0`.
    pub const LAB_F0: usize = 90;
    /// `lab_f1`.
    pub const LAB_F1: usize = 91;
    /// `lab_f10`.
    pub const LAB_F10: usize = 92;
    /// `lab_f2`.
    pub const LAB_F2: usize = 93;
    /// `lab_f3`.
    pub const LAB_F3: usize = 94;
    /// `lab_f4`.
    pub const LAB_F4: usize = 95;
    /// `lab_f5`.
    pub const LAB_F5: usize = 96;
    /// `lab_f6`.
    pub const LAB_F6: usize = 97;
    /// `lab_f7`.
    pub const LAB_F7: usize = 98;
    /// `lab_f8`.
    pub const LAB_F8: usize = 99;
    /// `lab_f9`.
    pub const LAB_F9: usize = 100;
    /// `meta_off`.
    pub const META_OFF: usize = 101;
    /// `meta_on`.
    pub const META_ON: usize = 102;
    /// `newline`.
    pub const NEWLINE: usize = 103;
    /// `pad_char`.
    pub const PAD_CHAR: usize = 104;
    /// `parm_dch`.
    pub const PARM_DCH: usize = 105;
    /// `parm_delete_line`.
    pub const PARM_DELETE_LINE: usize = 106;
    /// `parm_down_cursor`.
    pub const PARM_DOWN_CURSOR: usize = 107;
    /// `parm_ich`.
    pub const PARM_ICH: usize = 108;
    /// `parm_index`.
    pub const PARM_INDEX: usize = 109;
    /// `parm_insert_line`.
    pub const PARM_INSERT_LINE: usize = 110;
    /// `parm_left_cursor`.
    pub const PARM_LEFT_CURSOR: usize = 111;
    /// `parm_right_cursor`.
    pub const PARM_RIGHT_CURSOR: usize = 112;
    /// `parm_rindex`.
    pub const PARM_RINDEX: usize = 113;
    /// `parm_up_cursor`.
    pub const PARM_UP_CURSOR: usize = 114;
    /// `pkey_key`.
    pub const PKEY_KEY: usize = 115;
    /// `pkey_local`.
    pub const PKEY_LOCAL: usize = 116;
    /// `pkey_xmit`.
    pub const PKEY_XMIT: usize = 117;
    /// `print_screen`.
    pub const PRINT_SCREEN: usize = 118;
    /// `prtr_off`.
    pub const PRTR_OFF: usize = 119;
    /// `prtr_on`.
    pub const PRTR_ON: usize = 120;
    /// `repeat_char`.
    pub const REPEAT_CHAR: usize = 121;
    /// `reset_1string`.
    pub const RESET_1STRING: usize = 122;
    /// `reset_2string`.
    pub const RESET_2STRING: usize = 123;
    /// `reset_3string`.
    pub const RESET_3STRING: usize = 124;
    /// `reset_file`.
    pub const RESET_FILE: usize = 125;
    /// `restore_cursor`.
    pub const RESTORE_CURSOR: usize = 126;
    /// `row_address`.
    pub const ROW_ADDRESS: usize = 127;
    /// `save_cursor`.
    pub const SAVE_CURSOR: usize = 128;
    /// `scroll_forward`.
    pub const SCROLL_FORWARD: usize = 129;
    /// `scroll_reverse`.
    pub const SCROLL_REVERSE: usize = 130;
    /// `set_attributes`.
    pub const SET_ATTRIBUTES: usize = 131;
    /// `set_tab`.
    pub const SET_TAB: usize = 132;
    /// `set_window`.
    pub const SET_WINDOW: usize = 133;
    /// `tab`.
    pub const TAB: usize = 134;
    /// `to_status_line`.
    pub const TO_STATUS_LINE: usize = 135;
    /// `underline_char`.
    pub const UNDERLINE_CHAR: usize = 136;
    /// `up_half_line`.
    pub const UP_HALF_LINE: usize = 137;
    /// `init_prog`.
    pub const INIT_PROG: usize = 138;
    /// `key_a1`.
    pub const KEY_A1: usize = 139;
    /// `key_a3`.
    pub const KEY_A3: usize = 140;
    /// `key_b2`.
    pub const KEY_B2: usize = 141;
    /// `key_c1`.
    pub const KEY_C1: usize = 142;
    /// `key_c3`.
    pub const KEY_C3: usize = 143;
    /// `prtr_non`.
    pub const PRTR_NON: usize = 144;
    /// `char_padding`.
    pub const CHAR_PADDING: usize = 145;
    /// `acs_chars`.
    pub const ACS_CHARS: usize = 146;
    /// `plab_norm`.
    pub const PLAB_NORM: usize = 147;
    /// `key_btab`.
    pub const KEY_BTAB: usize = 148;
    /// `enter_xon_mode`.
    pub const ENTER_XON_MODE: usize = 149;
    /// `exit_xon_mode`.
    pub const EXIT_XON_MODE: usize = 150;
    /// `enter_am_mode`.
    pub const ENTER_AM_MODE: usize = 151;
    /// `exit_am_mode`.
    pub const EXIT_AM_MODE: usize = 152;
    /// `xon_character`.
    pub const XON_CHARACTER: usize = 153;
    /// `xoff_character`.
    pub const XOFF_CHARACTER: usize = 154;
    /// `ena_acs`.
    pub const ENA_ACS: usize = 155;
    /// `label_on`.
    pub const LABEL_ON: usize = 156;
    /// `label_off`.
    pub const LABEL_OFF: usize = 157;
    /// `key_beg`.
    pub const KEY_BEG: usize = 158;
    /// `key_cancel`.
    pub const KEY_CANCEL: usize = 159;
    /// `key_close`.
    pub const KEY_CLOSE: usize = 160;
    /// `key_command`.
    pub const KEY_COMMAND: usize = 161;
    /// `key_copy`.
    pub const KEY_COPY: usize = 162;
    /// `key_create`.
    pub const KEY_CREATE: usize = 163;
    /// `key_end`.
    pub const KEY_END: usize = 164;
    /// `key_enter`.
    pub const KEY_ENTER: usize = 165;
    /// `key_exit`.
    pub const KEY_EXIT: usize = 166;
    /// `key_find`.
    pub const KEY_FIND: usize = 167;
    /// `key_help`.
    pub const KEY_HELP: usize = 168;
    /// `key_mark`.
    pub const KEY_MARK: usize = 169;
    /// `key_message`.
    pub const KEY_MESSAGE: usize = 170;
    /// `key_move`.
    pub const KEY_MOVE: usize = 171;
    /// `key_next`.
    pub const KEY_NEXT: usize = 172;
    /// `key_open`.
    pub const KEY_OPEN: usize = 173;
    /// `key_options`.
    pub const KEY_OPTIONS: usize = 174;
    /// `key_previous`.
    pub const KEY_PREVIOUS: usize = 175;
    /// `key_print`.
    pub const KEY_PRINT: usize = 176;
    /// `key_redo`.
    pub const KEY_REDO: usize = 177;
    /// `key_reference`.
    pub const KEY_REFERENCE: usize = 178;
    /// `key_refresh`.
    pub const KEY_REFRESH: usize = 179;
    /// `key_replace`.
    pub const KEY_REPLACE: usize = 180;
    /// `key_restart`.
    pub const KEY_RESTART: usize = 181;
    /// `key_resume`.
    pub const KEY_RESUME: usize = 182;
    /// `key_save`.
    pub const KEY_SAVE: usize = 183;
    /// `key_suspend`.
    pub const KEY_SUSPEND: usize = 184;
    /// `key_undo`.
    pub const KEY_UNDO: usize = 185;
    /// `key_sbeg`.
    pub const KEY_SBEG: usize = 186;
    /// `key_scancel`.
    pub const KEY_SCANCEL: usize = 187;
    /// `key_scommand`.
    pub const KEY_SCOMMAND: usize = 188;
    /// `key_scopy`.
    pub const KEY_SCOPY: usize = 189;
    /// `key_screate`.
    pub const KEY_SCREATE: usize = 190;
    /// `key_sdc`.
    pub const KEY_SDC: usize = 191;
    /// `key_sdl`.
    pub const KEY_SDL: usize = 192;
    /// `key_select`.
    pub const KEY_SELECT: usize = 193;
    /// `key_send`.
    pub const KEY_SEND: usize = 194;
    /// `key_seol`.
    pub const KEY_SEOL: usize = 195;
    /// `key_sexit`.
    pub const KEY_SEXIT: usize = 196;
    /// `key_sfind`.
    pub const KEY_SFIND: usize = 197;
    /// `key_shelp`.
    pub const KEY_SHELP: usize = 198;
    /// `key_shome`.
    pub const KEY_SHOME: usize = 199;
    /// `key_sic`.
    pub const KEY_SIC: usize = 200;
    /// `key_sleft`.
    pub const KEY_SLEFT: usize = 201;
    /// `key_smessage`.
    pub const KEY_SMESSAGE: usize = 202;
    /// `key_smove`.
    pub const KEY_SMOVE: usize = 203;
    /// `key_snext`.
    pub const KEY_SNEXT: usize = 204;
    /// `key_soptions`.
    pub const KEY_SOPTIONS: usize = 205;
    /// `key_sprevious`.
    pub const KEY_SPREVIOUS: usize = 206;
    /// `key_sprint`.
    pub const KEY_SPRINT: usize = 207;
    /// `key_sredo`.
    pub const KEY_SREDO: usize = 208;
    /// `key_sreplace`.
    pub const KEY_SREPLACE: usize = 209;
    /// `key_sright`.
    pub const KEY_SRIGHT: usize = 210;
    /// `key_srsume`.
    pub const KEY_SRSUME: usize = 211;
    /// `key_ssave`.
    pub const KEY_SSAVE: usize = 212;
    /// `key_ssuspend`.
    pub const KEY_SSUSPEND: usize = 213;
    /// `key_sundo`.
    pub const KEY_SUNDO: usize = 214;
    /// `req_for_input`.
    pub const REQ_FOR_INPUT: usize = 215;
    /// `key_f11`.
    pub const KEY_F11: usize = 216;
    /// `key_f12`.
    pub const KEY_F12: usize = 217;
    /// `key_f13`.
    pub const KEY_F13: usize = 218;
    /// `key_f14`.
    pub const KEY_F14: usize = 219;
    /// `key_f15`.
    pub const KEY_F15: usize = 220;
    /// `key_f16`.
    pub const KEY_F16: usize = 221;
    /// `key_f17`.
    pub const KEY_F17: usize = 222;
    /// `key_f18`.
    pub const KEY_F18: usize = 223;
    /// `key_f19`.
    pub const KEY_F19: usize = 224;
    /// `key_f20`.
    pub const KEY_F20: usize = 225;
    /// `key_f21`.
    pub const KEY_F21: usize = 226;
    /// `key_f22`.
    pub const KEY_F22: usize = 227;
    /// `key_f23`.
    pub const KEY_F23: usize = 228;
    /// `key_f24`.
    pub const KEY_F24: usize = 229;
    /// `key_f25`.
    pub const KEY_F25: usize = 230;
    /// `key_f26`.
    pub const KEY_F26: usize = 231;
    /// `key_f27`.
    pub const KEY_F27: usize = 232;
    /// `key_f28`.
    pub const KEY_F28: usize = 233;
    /// `key_f29`.
    pub const KEY_F29: usize = 234;
    /// `key_f30`.
    pub const KEY_F30: usize = 235;
    /// `key_f31`.
    pub const KEY_F31: usize = 236;
    /// `key_f32`.
    pub const KEY_F32: usize = 237;
    /// `key_f33`.
    pub const KEY_F33: usize = 238;
    /// `key_f34`.
    pub const KEY_F34: usize = 239;
    /// `key_f35`.
    pub const KEY_F35: usize = 240;
    /// `key_f36`.
    pub const KEY_F36: usize = 241;
    /// `key_f37`.
    pub const KEY_F37: usize = 242;
    /// `key_f38`.
    pub const KEY_F38: usize = 243;
    /// `key_f39`.
    pub const KEY_F39: usize = 244;
    /// `key_f40`.
    pub const KEY_F40: usize = 245;
    /// `key_f41`.
    pub const KEY_F41: usize = 246;
    /// `key_f42`.
    pub const KEY_F42: usize = 247;
    /// `key_f43`.
    pub const KEY_F43: usize = 248;
    /// `key_f44`.
    pub const KEY_F44: usize = 249;
    /// `key_f45`.
    pub const KEY_F45: usize = 250;
    /// `key_f46`.
    pub const KEY_F46: usize = 251;
    /// `key_f47`.
    pub const KEY_F47: usize = 252;
    /// `key_f48`.
    pub const KEY_F48: usize = 253;
    /// `key_f49`.
    pub const KEY_F49: usize = 254;
    /// `key_f50`.
    pub const KEY_F50: usize = 255;
    /// `key_f51`.
    pub const KEY_F51: usize = 256;
    /// `key_f52`.
    pub const KEY_F52: usize = 257;
    /// `key_f53`.
    pub const KEY_F53: usize = 258;
    /// `key_f54`.
    pub const KEY_F54: usize = 259;
    /// `key_f55`.
    pub const KEY_F55: usize = 260;
    /// `key_f56`.
    pub const KEY_F56: usize = 261;
    /// `key_f57`.
    pub const KEY_F57: usize = 262;
    /// `key_f58`.
    pub const KEY_F58: usize = 263;
    /// `key_f59`.
    pub const KEY_F59: usize = 264;
    /// `key_f60`.
    pub const KEY_F60: usize = 265;
    /// `key_f61`.
    pub const KEY_F61: usize = 266;
    /// `key_f62`.
    pub const KEY_F62: usize = 267;
    /// `key_f63`.
    pub const KEY_F63: usize = 268;
    /// `clr_bol`.
    pub const CLR_BOL: usize = 269;
    /// `clear_margins`.
    pub const CLEAR_MARGINS: usize = 270;
    /// `set_left_margin`.
    pub const SET_LEFT_MARGIN: usize = 271;
    /// `set_right_margin`.
    pub const SET_RIGHT_MARGIN: usize = 272;
    /// `label_format`.
    pub const LABEL_FORMAT: usize = 273;
    /// `set_clock`.
    pub const SET_CLOCK: usize = 274;
    /// `display_clock`.
    pub const DISPLAY_CLOCK: usize = 275;
    /// `remove_clock`.
    pub const REMOVE_CLOCK: usize = 276;
    /// `create_window`.
    pub const CREATE_WINDOW: usize = 277;
    /// `goto_window`.
    pub const GOTO_WINDOW: usize = 278;
    /// `hangup`.
    pub const HANGUP: usize = 279;
    /// `dial_phone`.
    pub const DIAL_PHONE: usize = 280;
    /// `quick_dial`.
    pub const QUICK_DIAL: usize = 281;
    /// `tone`.
    pub const TONE: usize = 282;
    /// `pulse`.
    pub const PULSE: usize = 283;
    /// `flash_hook`.
    pub const FLASH_HOOK: usize = 284;
    /// `fixed_pause`.
    pub const FIXED_PAUSE: usize = 285;
    /// `wait_tone`.
    pub const WAIT_TONE: usize = 286;
    /// `user0`.
    pub const USER0: usize = 287;
    /// `user1`.
    pub const USER1: usize = 288;
    /// `user2`.
    pub const USER2: usize = 289;
    /// `user3`.
    pub const USER3: usize = 290;
    /// `user4`.
    pub const USER4: usize = 291;
    /// `user5`.
    pub const USER5: usize = 292;
    /// `user6`.
    pub const USER6: usize = 293;
    /// `user7`.
    pub const USER7: usize = 294;
    /// `user8`.
    pub const USER8: usize = 295;
    /// `user9`.
    pub const USER9: usize = 296;
    /// `orig_pair`.
    pub const ORIG_PAIR: usize = 297;
    /// `orig_colors`.
    pub const ORIG_COLORS: usize = 298;
    /// `initialize_color`.
    pub const INITIALIZE_COLOR: usize = 299;
    /// `initialize_pair`.
    pub const INITIALIZE_PAIR: usize = 300;
    /// `set_color_pair`.
    pub const SET_COLOR_PAIR: usize = 301;
    /// `set_foreground`.
    pub const SET_FOREGROUND: usize = 302;
    /// `set_background`.
    pub const SET_BACKGROUND: usize = 303;
    /// `change_char_pitch`.
    pub const CHANGE_CHAR_PITCH: usize = 304;
    /// `change_line_pitch`.
    pub const CHANGE_LINE_PITCH: usize = 305;
    /// `change_res_horz`.
    pub const CHANGE_RES_HORZ: usize = 306;
    /// `change_res_vert`.
    pub const CHANGE_RES_VERT: usize = 307;
    /// `define_char`.
    pub const DEFINE_CHAR: usize = 308;
    /// `enter_doublewide_mode`.
    pub const ENTER_DOUBLEWIDE_MODE: usize = 309;
    /// `enter_draft_quality`.
    pub const ENTER_DRAFT_QUALITY: usize = 310;
    /// `enter_italics_mode`.
    pub const ENTER_ITALICS_MODE: usize = 311;
    /// `enter_leftward_mode`.
    pub const ENTER_LEFTWARD_MODE: usize = 312;
    /// `enter_micro_mode`.
    pub const ENTER_MICRO_MODE: usize = 313;
    /// `enter_near_letter_quality`.
    pub const ENTER_NEAR_LETTER_QUALITY: usize = 314;
    /// `enter_normal_quality`.
    pub const ENTER_NORMAL_QUALITY: usize = 315;
    /// `enter_shadow_mode`.
    pub const ENTER_SHADOW_MODE: usize = 316;
    /// `enter_subscript_mode`.
    pub const ENTER_SUBSCRIPT_MODE: usize = 317;
    /// `enter_superscript_mode`.
    pub const ENTER_SUPERSCRIPT_MODE: usize = 318;
    /// `enter_upward_mode`.
    pub const ENTER_UPWARD_MODE: usize = 319;
    /// `exit_doublewide_mode`.
    pub const EXIT_DOUBLEWIDE_MODE: usize = 320;
    /// `exit_italics_mode`.
    pub const EXIT_ITALICS_MODE: usize = 321;
    /// `exit_leftward_mode`.
    pub const EXIT_LEFTWARD_MODE: usize = 322;
    /// `exit_micro_mode`.
    pub const EXIT_MICRO_MODE: usize = 323;
    /// `exit_shadow_mode`.
    pub const EXIT_SHADOW_MODE: usize = 324;
    /// `exit_subscript_mode`.
    pub const EXIT_SUBSCRIPT_MODE: usize = 325;
    /// `exit_superscript_mode`.
    pub const EXIT_SUPERSCRIPT_MODE: usize = 326;
    /// `exit_upward_mode`.
    pub const EXIT_UPWARD_MODE: usize = 327;
    /// `micro_column_address`.
    pub const MICRO_COLUMN_ADDRESS: usize = 328;
    /// `micro_down`.
    pub const MICRO_DOWN: usize = 329;
    /// `micro_left`.
    pub const MICRO_LEFT: usize = 330;
    /// `micro_right`.
    pub const MICRO_RIGHT: usize = 331;
    /// `micro_row_address`.
    pub const MICRO_ROW_ADDRESS: usize = 332;
    /// `micro_up`.
    pub const MICRO_UP: usize = 333;
    /// `order_of_pins`.
    pub const ORDER_OF_PINS: usize = 334;
    /// `parm_down_micro`.
    pub const PARM_DOWN_MICRO: usize = 335;
    /// `parm_left_micro`.
    pub const PARM_LEFT_MICRO: usize = 336;
    /// `parm_right_micro`.
    pub const PARM_RIGHT_MICRO: usize = 337;
    /// `parm_up_micro`.
    pub const PARM_UP_MICRO: usize = 338;
    /// `select_char_set`.
    pub const SELECT_CHAR_SET: usize = 339;
    /// `set_bottom_margin`.
    pub const SET_BOTTOM_MARGIN: usize = 340;
    /// `set_bottom_margin_parm`.
    pub const SET_BOTTOM_MARGIN_PARM: usize = 341;
    /// `set_left_margin_parm`.
    pub const SET_LEFT_MARGIN_PARM: usize = 342;
    /// `set_right_margin_parm`.
    pub const SET_RIGHT_MARGIN_PARM: usize = 343;
    /// `set_top_margin`.
    pub const SET_TOP_MARGIN: usize = 344;
    /// `set_top_margin_parm`.
    pub const SET_TOP_MARGIN_PARM: usize = 345;
    /// `start_bit_image`.
    pub const START_BIT_IMAGE: usize = 346;
    /// `start_char_set_def`.
    pub const START_CHAR_SET_DEF: usize = 347;
    /// `stop_bit_image`.
    pub const STOP_BIT_IMAGE: usize = 348;
    /// `stop_char_set_def`.
    pub const STOP_CHAR_SET_DEF: usize = 349;
    /// `subscript_characters`.
    pub const SUBSCRIPT_CHARACTERS: usize = 350;
    /// `superscript_characters`.
    pub const SUPERSCRIPT_CHARACTERS: usize = 351;
    /// `these_cause_cr`.
    pub const THESE_CAUSE_CR: usize = 352;
    /// `zero_motion`.
    pub const ZERO_MOTION: usize = 353;
    /// `char_set_names`.
    pub const CHAR_SET_NAMES: usize = 354;
    /// `key_mouse`.
    pub const KEY_MOUSE: usize = 355;
    /// `mouse_info`.
    pub const MOUSE_INFO: usize = 356;
    /// `req_mouse_pos`.
    pub const REQ_MOUSE_POS: usize = 357;
    /// `get_mouse`.
    pub const GET_MOUSE: usize = 358;
    /// `set_a_foreground`.
    pub const SET_A_FOREGROUND: usize = 359;
    /// `set_a_background`.
    pub const SET_A_BACKGROUND: usize = 360;
    /// `pkey_plab`.
    pub const PKEY_PLAB: usize = 361;
    /// `device_type`.
    pub const DEVICE_TYPE: usize = 362;
    /// `code_set_init`.
    pub const CODE_SET_INIT: usize = 363;
    /// `set0_des_seq`.
    pub const SET0_DES_SEQ: usize = 364;
    /// `set1_des_seq`.
    pub const SET1_DES_SEQ: usize = 365;
    /// `set2_des_seq`.
    pub const SET2_DES_SEQ: usize = 366;
    /// `set3_des_seq`.
    pub const SET3_DES_SEQ: usize = 367;
    /// `set_lr_margin`.
    pub const SET_LR_MARGIN: usize = 368;
    /// `set_tb_margin`.
    pub const SET_TB_MARGIN: usize = 369;
    /// `bit_image_repeat`.
    pub const BIT_IMAGE_REPEAT: usize = 370;
    /// `bit_image_newline`.
    pub const BIT_IMAGE_NEWLINE: usize = 371;
    /// `bit_image_carriage_return`.
    pub const BIT_IMAGE_CARRIAGE_RETURN: usize = 372;
    /// `color_names`.
    pub const COLOR_NAMES: usize = 373;
    /// `define_bit_image_region`.
    pub const DEFINE_BIT_IMAGE_REGION: usize = 374;
    /// `end_bit_image_region`.
    pub const END_BIT_IMAGE_REGION: usize = 375;
    /// `set_color_band`.
    pub const SET_COLOR_BAND: usize = 376;
    /// `set_page_length`.
    pub const SET_PAGE_LENGTH: usize = 377;
    /// `display_pc_char`.
    pub const DISPLAY_PC_CHAR: usize = 378;
    /// `enter_pc_charset_mode`.
    pub const ENTER_PC_CHARSET_MODE: usize = 379;
    /// `exit_pc_charset_mode`.
    pub const EXIT_PC_CHARSET_MODE: usize = 380;
    /// `enter_scancode_mode`.
    pub const ENTER_SCANCODE_MODE: usize = 381;
    /// `exit_scancode_mode`.
    pub const EXIT_SCANCODE_MODE: usize = 382;
    /// `pc_term_options`.
    pub const PC_TERM_OPTIONS: usize = 383;
    /// `scancode_escape`.
    pub const SCANCODE_ESCAPE: usize = 384;
    /// `alt_scancode_esc`.
    pub const ALT_SCANCODE_ESC: usize = 385;
    /// `enter_horizontal_hl_mode`.
    pub const ENTER_HORIZONTAL_HL_MODE: usize = 386;
    /// `enter_left_hl_mode`.
    pub const ENTER_LEFT_HL_MODE: usize = 387;
    /// `enter_low_hl_mode`.
    pub const ENTER_LOW_HL_MODE: usize = 388;
    /// `enter_right_hl_mode`.
    pub const ENTER_RIGHT_HL_MODE: usize = 389;
    /// `enter_top_hl_mode`.
    pub const ENTER_TOP_HL_MODE: usize = 390;
    /// `enter_vertical_hl_mode`.
    pub const ENTER_VERTICAL_HL_MODE: usize = 391;
    /// `set_a_attributes`.
    pub const SET_A_ATTRIBUTES: usize = 392;
    /// `set_pglen_inch`.
    pub const SET_PGLEN_INCH: usize = 393;
    /// `termcap_init2`.
    pub const TERMCAP_INIT2: usize = 394;
    /// `termcap_reset`.
    pub const TERMCAP_RESET: usize = 395;
    /// `linefeed_if_not_lf`.
    pub const LINEFEED_IF_NOT_LF: usize = 396;
    /// `backspace_if_not_bs`.
    pub const BACKSPACE_IF_NOT_BS: usize = 397;
    /// `other_non_function_keys`.
    pub const OTHER_NON_FUNCTION_KEYS: usize = 398;
    /// `arrow_key_map`.
    pub const ARROW_KEY_MAP: usize = 399;
    /// `acs_ulcorner`.
    pub const ACS_ULCORNER: usize = 400;
    /// `acs_llcorner`.
    pub const ACS_LLCORNER: usize = 401;
    /// `acs_urcorner`.
    pub const ACS_URCORNER: usize = 402;
    /// `acs_lrcorner`.
    pub const ACS_LRCORNER: usize = 403;
    /// `acs_ltee`.
    pub const ACS_LTEE: usize = 404;
    /// `acs_rtee`.
    pub const ACS_RTEE: usize = 405;
    /// `acs_btee`.
    pub const ACS_BTEE: usize = 406;
    /// `acs_ttee`.
    pub const ACS_TTEE: usize = 407;
    /// `acs_hline`.
    pub const ACS_HLINE: usize = 408;
    /// `acs_vline`.
    pub const ACS_VLINE: usize = 409;
    /// `acs_plus`.
    pub const ACS_PLUS: usize = 410;
    /// `memory_lock`.
    pub const MEMORY_LOCK: usize = 411;
    /// `memory_unlock`.
    pub const MEMORY_UNLOCK: usize = 412;
    /// `box_chars_1`.
    pub const BOX_CHARS_1: usize = 413;
}
