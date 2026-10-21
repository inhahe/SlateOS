//! The key codes of ncurses 6.4's `curses.h`, and `init_keytry.h`'s
//! `_nc_tinfo_fkeys`: each key capability of the terminfo database and the
//! code `wgetch` returns for its sequence, in the order `make_keys` writes
//! them from `include/Caps` -- the order they go into the key tries, which
//! decides which code a sequence two capabilities share comes back as.
//! Generated from those files.

use crate::caps::string;

/// `KEY_CODE_YES`: what `get_wch` returns for a key rather than a character.
pub const KEY_CODE_YES: i32 = 0o400;
/// `KEY_MIN`: the lowest key code.
pub const KEY_MIN: i32 = 0o401;
/// `KEY_MAX`: the highest key code ncurses names; the codes of a terminal's
/// own key capabilities follow it.
pub const KEY_MAX: i32 = 0o777;
/// `KEY_BREAK`: Break key (unreliable).
pub const KEY_BREAK: i32 = 0o401;
/// `KEY_SRESET`: Soft (partial) reset (unreliable).
pub const KEY_SRESET: i32 = 0o530;
/// `KEY_RESET`: Reset or hard reset (unreliable).
pub const KEY_RESET: i32 = 0o531;
/// `KEY_DOWN`: down-arrow key.
pub const KEY_DOWN: i32 = 0o402;
/// `KEY_UP`: up-arrow key.
pub const KEY_UP: i32 = 0o403;
/// `KEY_LEFT`: left-arrow key.
pub const KEY_LEFT: i32 = 0o404;
/// `KEY_RIGHT`: right-arrow key.
pub const KEY_RIGHT: i32 = 0o405;
/// `KEY_HOME`: home key.
pub const KEY_HOME: i32 = 0o406;
/// `KEY_BACKSPACE`: backspace key.
pub const KEY_BACKSPACE: i32 = 0o407;
/// `KEY_F0`: Function keys.  Space for 64.
pub const KEY_F0: i32 = 0o410;
/// `KEY_DL`: delete-line key.
pub const KEY_DL: i32 = 0o510;
/// `KEY_IL`: insert-line key.
pub const KEY_IL: i32 = 0o511;
/// `KEY_DC`: delete-character key.
pub const KEY_DC: i32 = 0o512;
/// `KEY_IC`: insert-character key.
pub const KEY_IC: i32 = 0o513;
/// `KEY_EIC`: sent by rmir or smir in insert mode.
pub const KEY_EIC: i32 = 0o514;
/// `KEY_CLEAR`: clear-screen or erase key.
pub const KEY_CLEAR: i32 = 0o515;
/// `KEY_EOS`: clear-to-end-of-screen key.
pub const KEY_EOS: i32 = 0o516;
/// `KEY_EOL`: clear-to-end-of-line key.
pub const KEY_EOL: i32 = 0o517;
/// `KEY_SF`: scroll-forward key.
pub const KEY_SF: i32 = 0o520;
/// `KEY_SR`: scroll-backward key.
pub const KEY_SR: i32 = 0o521;
/// `KEY_NPAGE`: next-page key.
pub const KEY_NPAGE: i32 = 0o522;
/// `KEY_PPAGE`: previous-page key.
pub const KEY_PPAGE: i32 = 0o523;
/// `KEY_STAB`: set-tab key.
pub const KEY_STAB: i32 = 0o524;
/// `KEY_CTAB`: clear-tab key.
pub const KEY_CTAB: i32 = 0o525;
/// `KEY_CATAB`: clear-all-tabs key.
pub const KEY_CATAB: i32 = 0o526;
/// `KEY_ENTER`: enter/send key.
pub const KEY_ENTER: i32 = 0o527;
/// `KEY_PRINT`: print key.
pub const KEY_PRINT: i32 = 0o532;
/// `KEY_LL`: lower-left key (home down).
pub const KEY_LL: i32 = 0o533;
/// `KEY_A1`: upper left of keypad.
pub const KEY_A1: i32 = 0o534;
/// `KEY_A3`: upper right of keypad.
pub const KEY_A3: i32 = 0o535;
/// `KEY_B2`: center of keypad.
pub const KEY_B2: i32 = 0o536;
/// `KEY_C1`: lower left of keypad.
pub const KEY_C1: i32 = 0o537;
/// `KEY_C3`: lower right of keypad.
pub const KEY_C3: i32 = 0o540;
/// `KEY_BTAB`: back-tab key.
pub const KEY_BTAB: i32 = 0o541;
/// `KEY_BEG`: begin key.
pub const KEY_BEG: i32 = 0o542;
/// `KEY_CANCEL`: cancel key.
pub const KEY_CANCEL: i32 = 0o543;
/// `KEY_CLOSE`: close key.
pub const KEY_CLOSE: i32 = 0o544;
/// `KEY_COMMAND`: command key.
pub const KEY_COMMAND: i32 = 0o545;
/// `KEY_COPY`: copy key.
pub const KEY_COPY: i32 = 0o546;
/// `KEY_CREATE`: create key.
pub const KEY_CREATE: i32 = 0o547;
/// `KEY_END`: end key.
pub const KEY_END: i32 = 0o550;
/// `KEY_EXIT`: exit key.
pub const KEY_EXIT: i32 = 0o551;
/// `KEY_FIND`: find key.
pub const KEY_FIND: i32 = 0o552;
/// `KEY_HELP`: help key.
pub const KEY_HELP: i32 = 0o553;
/// `KEY_MARK`: mark key.
pub const KEY_MARK: i32 = 0o554;
/// `KEY_MESSAGE`: message key.
pub const KEY_MESSAGE: i32 = 0o555;
/// `KEY_MOVE`: move key.
pub const KEY_MOVE: i32 = 0o556;
/// `KEY_NEXT`: next key.
pub const KEY_NEXT: i32 = 0o557;
/// `KEY_OPEN`: open key.
pub const KEY_OPEN: i32 = 0o560;
/// `KEY_OPTIONS`: options key.
pub const KEY_OPTIONS: i32 = 0o561;
/// `KEY_PREVIOUS`: previous key.
pub const KEY_PREVIOUS: i32 = 0o562;
/// `KEY_REDO`: redo key.
pub const KEY_REDO: i32 = 0o563;
/// `KEY_REFERENCE`: reference key.
pub const KEY_REFERENCE: i32 = 0o564;
/// `KEY_REFRESH`: refresh key.
pub const KEY_REFRESH: i32 = 0o565;
/// `KEY_REPLACE`: replace key.
pub const KEY_REPLACE: i32 = 0o566;
/// `KEY_RESTART`: restart key.
pub const KEY_RESTART: i32 = 0o567;
/// `KEY_RESUME`: resume key.
pub const KEY_RESUME: i32 = 0o570;
/// `KEY_SAVE`: save key.
pub const KEY_SAVE: i32 = 0o571;
/// `KEY_SBEG`: shifted begin key.
pub const KEY_SBEG: i32 = 0o572;
/// `KEY_SCANCEL`: shifted cancel key.
pub const KEY_SCANCEL: i32 = 0o573;
/// `KEY_SCOMMAND`: shifted command key.
pub const KEY_SCOMMAND: i32 = 0o574;
/// `KEY_SCOPY`: shifted copy key.
pub const KEY_SCOPY: i32 = 0o575;
/// `KEY_SCREATE`: shifted create key.
pub const KEY_SCREATE: i32 = 0o576;
/// `KEY_SDC`: shifted delete-character key.
pub const KEY_SDC: i32 = 0o577;
/// `KEY_SDL`: shifted delete-line key.
pub const KEY_SDL: i32 = 0o600;
/// `KEY_SELECT`: select key.
pub const KEY_SELECT: i32 = 0o601;
/// `KEY_SEND`: shifted end key.
pub const KEY_SEND: i32 = 0o602;
/// `KEY_SEOL`: shifted clear-to-end-of-line key.
pub const KEY_SEOL: i32 = 0o603;
/// `KEY_SEXIT`: shifted exit key.
pub const KEY_SEXIT: i32 = 0o604;
/// `KEY_SFIND`: shifted find key.
pub const KEY_SFIND: i32 = 0o605;
/// `KEY_SHELP`: shifted help key.
pub const KEY_SHELP: i32 = 0o606;
/// `KEY_SHOME`: shifted home key.
pub const KEY_SHOME: i32 = 0o607;
/// `KEY_SIC`: shifted insert-character key.
pub const KEY_SIC: i32 = 0o610;
/// `KEY_SLEFT`: shifted left-arrow key.
pub const KEY_SLEFT: i32 = 0o611;
/// `KEY_SMESSAGE`: shifted message key.
pub const KEY_SMESSAGE: i32 = 0o612;
/// `KEY_SMOVE`: shifted move key.
pub const KEY_SMOVE: i32 = 0o613;
/// `KEY_SNEXT`: shifted next key.
pub const KEY_SNEXT: i32 = 0o614;
/// `KEY_SOPTIONS`: shifted options key.
pub const KEY_SOPTIONS: i32 = 0o615;
/// `KEY_SPREVIOUS`: shifted previous key.
pub const KEY_SPREVIOUS: i32 = 0o616;
/// `KEY_SPRINT`: shifted print key.
pub const KEY_SPRINT: i32 = 0o617;
/// `KEY_SREDO`: shifted redo key.
pub const KEY_SREDO: i32 = 0o620;
/// `KEY_SREPLACE`: shifted replace key.
pub const KEY_SREPLACE: i32 = 0o621;
/// `KEY_SRIGHT`: shifted right-arrow key.
pub const KEY_SRIGHT: i32 = 0o622;
/// `KEY_SRSUME`: shifted resume key.
pub const KEY_SRSUME: i32 = 0o623;
/// `KEY_SSAVE`: shifted save key.
pub const KEY_SSAVE: i32 = 0o624;
/// `KEY_SSUSPEND`: shifted suspend key.
pub const KEY_SSUSPEND: i32 = 0o625;
/// `KEY_SUNDO`: shifted undo key.
pub const KEY_SUNDO: i32 = 0o626;
/// `KEY_SUSPEND`: suspend key.
pub const KEY_SUSPEND: i32 = 0o627;
/// `KEY_UNDO`: undo key.
pub const KEY_UNDO: i32 = 0o630;
/// `KEY_MOUSE`: Mouse event has occurred.
pub const KEY_MOUSE: i32 = 0o631;
/// `KEY_RESIZE`: Terminal resize event.
pub const KEY_RESIZE: i32 = 0o632;

/// `KEY_F (n)`: function key `n`.
#[must_use]
pub const fn key_f(n: i32) -> i32 {
    KEY_F0.wrapping_add(n)
}

/// `_nc_tinfo_fkeys`: each key's string capability, and its code.
pub(crate) const TINFO_FKEYS: [(usize, i32); 150] = [
    (string::KEY_BACKSPACE, KEY_BACKSPACE), // kbs
    (string::KEY_CATAB, KEY_CATAB),         // ktbc
    (string::KEY_CLEAR, KEY_CLEAR),         // kclr
    (string::KEY_CTAB, KEY_CTAB),           // kctab
    (string::KEY_DC, KEY_DC),               // kdch1
    (string::KEY_DL, KEY_DL),               // kdl1
    (string::KEY_DOWN, KEY_DOWN),           // kcud1
    (string::KEY_EIC, KEY_EIC),             // krmir
    (string::KEY_EOL, KEY_EOL),             // kel
    (string::KEY_EOS, KEY_EOS),             // ked
    (string::KEY_F0, key_f(0)),             // kf0
    (string::KEY_F1, key_f(1)),             // kf1
    (string::KEY_F10, key_f(10)),           // kf10
    (string::KEY_F2, key_f(2)),             // kf2
    (string::KEY_F3, key_f(3)),             // kf3
    (string::KEY_F4, key_f(4)),             // kf4
    (string::KEY_F5, key_f(5)),             // kf5
    (string::KEY_F6, key_f(6)),             // kf6
    (string::KEY_F7, key_f(7)),             // kf7
    (string::KEY_F8, key_f(8)),             // kf8
    (string::KEY_F9, key_f(9)),             // kf9
    (string::KEY_HOME, KEY_HOME),           // khome
    (string::KEY_IC, KEY_IC),               // kich1
    (string::KEY_IL, KEY_IL),               // kil1
    (string::KEY_LEFT, KEY_LEFT),           // kcub1
    (string::KEY_LL, KEY_LL),               // kll
    (string::KEY_NPAGE, KEY_NPAGE),         // knp
    (string::KEY_PPAGE, KEY_PPAGE),         // kpp
    (string::KEY_RIGHT, KEY_RIGHT),         // kcuf1
    (string::KEY_SF, KEY_SF),               // kind
    (string::KEY_SR, KEY_SR),               // kri
    (string::KEY_STAB, KEY_STAB),           // khts
    (string::KEY_UP, KEY_UP),               // kcuu1
    (string::KEY_A1, KEY_A1),               // ka1
    (string::KEY_A3, KEY_A3),               // ka3
    (string::KEY_B2, KEY_B2),               // kb2
    (string::KEY_C1, KEY_C1),               // kc1
    (string::KEY_C3, KEY_C3),               // kc3
    (string::KEY_BTAB, KEY_BTAB),           // kcbt
    (string::KEY_BEG, KEY_BEG),             // kbeg
    (string::KEY_CANCEL, KEY_CANCEL),       // kcan
    (string::KEY_CLOSE, KEY_CLOSE),         // kclo
    (string::KEY_COMMAND, KEY_COMMAND),     // kcmd
    (string::KEY_COPY, KEY_COPY),           // kcpy
    (string::KEY_CREATE, KEY_CREATE),       // kcrt
    (string::KEY_END, KEY_END),             // kend
    (string::KEY_ENTER, KEY_ENTER),         // kent
    (string::KEY_EXIT, KEY_EXIT),           // kext
    (string::KEY_FIND, KEY_FIND),           // kfnd
    (string::KEY_HELP, KEY_HELP),           // khlp
    (string::KEY_MARK, KEY_MARK),           // kmrk
    (string::KEY_MESSAGE, KEY_MESSAGE),     // kmsg
    (string::KEY_MOVE, KEY_MOVE),           // kmov
    (string::KEY_NEXT, KEY_NEXT),           // knxt
    (string::KEY_OPEN, KEY_OPEN),           // kopn
    (string::KEY_OPTIONS, KEY_OPTIONS),     // kopt
    (string::KEY_PREVIOUS, KEY_PREVIOUS),   // kprv
    (string::KEY_PRINT, KEY_PRINT),         // kprt
    (string::KEY_REDO, KEY_REDO),           // krdo
    (string::KEY_REFERENCE, KEY_REFERENCE), // kref
    (string::KEY_REFRESH, KEY_REFRESH),     // krfr
    (string::KEY_REPLACE, KEY_REPLACE),     // krpl
    (string::KEY_RESTART, KEY_RESTART),     // krst
    (string::KEY_RESUME, KEY_RESUME),       // kres
    (string::KEY_SAVE, KEY_SAVE),           // ksav
    (string::KEY_SUSPEND, KEY_SUSPEND),     // kspd
    (string::KEY_UNDO, KEY_UNDO),           // kund
    (string::KEY_SBEG, KEY_SBEG),           // kBEG
    (string::KEY_SCANCEL, KEY_SCANCEL),     // kCAN
    (string::KEY_SCOMMAND, KEY_SCOMMAND),   // kCMD
    (string::KEY_SCOPY, KEY_SCOPY),         // kCPY
    (string::KEY_SCREATE, KEY_SCREATE),     // kCRT
    (string::KEY_SDC, KEY_SDC),             // kDC
    (string::KEY_SDL, KEY_SDL),             // kDL
    (string::KEY_SELECT, KEY_SELECT),       // kslt
    (string::KEY_SEND, KEY_SEND),           // kEND
    (string::KEY_SEOL, KEY_SEOL),           // kEOL
    (string::KEY_SEXIT, KEY_SEXIT),         // kEXT
    (string::KEY_SFIND, KEY_SFIND),         // kFND
    (string::KEY_SHELP, KEY_SHELP),         // kHLP
    (string::KEY_SHOME, KEY_SHOME),         // kHOM
    (string::KEY_SIC, KEY_SIC),             // kIC
    (string::KEY_SLEFT, KEY_SLEFT),         // kLFT
    (string::KEY_SMESSAGE, KEY_SMESSAGE),   // kMSG
    (string::KEY_SMOVE, KEY_SMOVE),         // kMOV
    (string::KEY_SNEXT, KEY_SNEXT),         // kNXT
    (string::KEY_SOPTIONS, KEY_SOPTIONS),   // kOPT
    (string::KEY_SPREVIOUS, KEY_SPREVIOUS), // kPRV
    (string::KEY_SPRINT, KEY_SPRINT),       // kPRT
    (string::KEY_SREDO, KEY_SREDO),         // kRDO
    (string::KEY_SREPLACE, KEY_SREPLACE),   // kRPL
    (string::KEY_SRIGHT, KEY_SRIGHT),       // kRIT
    (string::KEY_SRSUME, KEY_SRSUME),       // kRES
    (string::KEY_SSAVE, KEY_SSAVE),         // kSAV
    (string::KEY_SSUSPEND, KEY_SSUSPEND),   // kSPD
    (string::KEY_SUNDO, KEY_SUNDO),         // kUND
    (string::KEY_F11, key_f(11)),           // kf11
    (string::KEY_F12, key_f(12)),           // kf12
    (string::KEY_F13, key_f(13)),           // kf13
    (string::KEY_F14, key_f(14)),           // kf14
    (string::KEY_F15, key_f(15)),           // kf15
    (string::KEY_F16, key_f(16)),           // kf16
    (string::KEY_F17, key_f(17)),           // kf17
    (string::KEY_F18, key_f(18)),           // kf18
    (string::KEY_F19, key_f(19)),           // kf19
    (string::KEY_F20, key_f(20)),           // kf20
    (string::KEY_F21, key_f(21)),           // kf21
    (string::KEY_F22, key_f(22)),           // kf22
    (string::KEY_F23, key_f(23)),           // kf23
    (string::KEY_F24, key_f(24)),           // kf24
    (string::KEY_F25, key_f(25)),           // kf25
    (string::KEY_F26, key_f(26)),           // kf26
    (string::KEY_F27, key_f(27)),           // kf27
    (string::KEY_F28, key_f(28)),           // kf28
    (string::KEY_F29, key_f(29)),           // kf29
    (string::KEY_F30, key_f(30)),           // kf30
    (string::KEY_F31, key_f(31)),           // kf31
    (string::KEY_F32, key_f(32)),           // kf32
    (string::KEY_F33, key_f(33)),           // kf33
    (string::KEY_F34, key_f(34)),           // kf34
    (string::KEY_F35, key_f(35)),           // kf35
    (string::KEY_F36, key_f(36)),           // kf36
    (string::KEY_F37, key_f(37)),           // kf37
    (string::KEY_F38, key_f(38)),           // kf38
    (string::KEY_F39, key_f(39)),           // kf39
    (string::KEY_F40, key_f(40)),           // kf40
    (string::KEY_F41, key_f(41)),           // kf41
    (string::KEY_F42, key_f(42)),           // kf42
    (string::KEY_F43, key_f(43)),           // kf43
    (string::KEY_F44, key_f(44)),           // kf44
    (string::KEY_F45, key_f(45)),           // kf45
    (string::KEY_F46, key_f(46)),           // kf46
    (string::KEY_F47, key_f(47)),           // kf47
    (string::KEY_F48, key_f(48)),           // kf48
    (string::KEY_F49, key_f(49)),           // kf49
    (string::KEY_F50, key_f(50)),           // kf50
    (string::KEY_F51, key_f(51)),           // kf51
    (string::KEY_F52, key_f(52)),           // kf52
    (string::KEY_F53, key_f(53)),           // kf53
    (string::KEY_F54, key_f(54)),           // kf54
    (string::KEY_F55, key_f(55)),           // kf55
    (string::KEY_F56, key_f(56)),           // kf56
    (string::KEY_F57, key_f(57)),           // kf57
    (string::KEY_F58, key_f(58)),           // kf58
    (string::KEY_F59, key_f(59)),           // kf59
    (string::KEY_F60, key_f(60)),           // kf60
    (string::KEY_F61, key_f(61)),           // kf61
    (string::KEY_F62, key_f(62)),           // kf62
    (string::KEY_F63, key_f(63)),           // kf63
    (string::KEY_MOUSE, KEY_MOUSE),         // kmous
];
