//! YAML: tree-sitter-yaml 0.7.2 (MIT, Ika and the tree-sitter-grammars
//! contributors). `grammars/yaml/` holds its `parser.c`, its highlight query,
//! its test corpus, its `scanner.c` and the schema it includes
//! (`schema.core.c`) as published.
//!
//! In YAML the scanner is most of the grammar: indentation and the block
//! collections it opens and closes, the flow collections, the four kinds of
//! string with their escapes, tags, anchors and aliases, and every plain
//! scalar -- with what the core schema resolves it to (null, a boolean, an
//! integer, a float or a string), which is what lets `enabled: true` colour
//! `true` as a constant. It is ported below function for function under the
//! C's names, the C's macros as methods (`RET_SYM` is [`Scanner::ret`],
//! `POP_IND` is [`Scanner::pop_ind_checked`]), and the C's 16-bit
//! arithmetic kept 16-bit, wrapping where the C's does.
//!
//! # Where the port differs
//!
//! - **Saving never writes past its buffer, nor reading past a saved
//!   state.** The C checks for room before each indentation pair but writes
//!   four bytes after it, and reads pairs until it has read `length` bytes
//!   whatever they are.
//! - **The schema's state 34 is frozen, not an abort.** No transition leads
//!   to it; the C aborts there, which in a scanner the runtime calls would
//!   take the program down.

use crate::ffi::{ExternalScanner, Lexer};

super::generated!("yaml", super::Scanner);

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/yaml/highlights.scm");

/// The external tokens under scanner.c's `TokenType` names, in the grammar's
/// order: what a `valid` list is indexed by and a token is reported as.
#[allow(
    dead_code,
    reason = "the grammar's whole list, kept whole so each index is where the C has it; the \
              scanner produces most but not all of them"
)]
mod tok {
    pub(super) const END_OF_FILE: u16 = 0;
    pub(super) const S_DIR_YML_BGN: u16 = 1;
    pub(super) const R_DIR_YML_VER: u16 = 2;
    pub(super) const S_DIR_TAG_BGN: u16 = 3;
    pub(super) const R_DIR_TAG_HDL: u16 = 4;
    pub(super) const R_DIR_TAG_PFX: u16 = 5;
    pub(super) const S_DIR_RSV_BGN: u16 = 6;
    pub(super) const R_DIR_RSV_PRM: u16 = 7;
    pub(super) const S_DRS_END: u16 = 8;
    pub(super) const S_DOC_END: u16 = 9;
    pub(super) const R_BLK_SEQ_BGN: u16 = 10;
    pub(super) const BR_BLK_SEQ_BGN: u16 = 11;
    pub(super) const B_BLK_SEQ_BGN: u16 = 12;
    pub(super) const R_BLK_KEY_BGN: u16 = 13;
    pub(super) const BR_BLK_KEY_BGN: u16 = 14;
    pub(super) const B_BLK_KEY_BGN: u16 = 15;
    pub(super) const R_BLK_VAL_BGN: u16 = 16;
    pub(super) const BR_BLK_VAL_BGN: u16 = 17;
    pub(super) const B_BLK_VAL_BGN: u16 = 18;
    pub(super) const R_BLK_IMP_BGN: u16 = 19;
    pub(super) const R_BLK_LIT_BGN: u16 = 20;
    pub(super) const BR_BLK_LIT_BGN: u16 = 21;
    pub(super) const R_BLK_FLD_BGN: u16 = 22;
    pub(super) const BR_BLK_FLD_BGN: u16 = 23;
    pub(super) const BR_BLK_STR_CTN: u16 = 24;
    pub(super) const R_FLW_SEQ_BGN: u16 = 25;
    pub(super) const BR_FLW_SEQ_BGN: u16 = 26;
    pub(super) const B_FLW_SEQ_BGN: u16 = 27;
    pub(super) const R_FLW_SEQ_END: u16 = 28;
    pub(super) const BR_FLW_SEQ_END: u16 = 29;
    pub(super) const B_FLW_SEQ_END: u16 = 30;
    pub(super) const R_FLW_MAP_BGN: u16 = 31;
    pub(super) const BR_FLW_MAP_BGN: u16 = 32;
    pub(super) const B_FLW_MAP_BGN: u16 = 33;
    pub(super) const R_FLW_MAP_END: u16 = 34;
    pub(super) const BR_FLW_MAP_END: u16 = 35;
    pub(super) const B_FLW_MAP_END: u16 = 36;
    pub(super) const R_FLW_SEP_BGN: u16 = 37;
    pub(super) const BR_FLW_SEP_BGN: u16 = 38;
    pub(super) const R_FLW_KEY_BGN: u16 = 39;
    pub(super) const BR_FLW_KEY_BGN: u16 = 40;
    pub(super) const R_FLW_JSV_BGN: u16 = 41;
    pub(super) const BR_FLW_JSV_BGN: u16 = 42;
    pub(super) const R_FLW_NJV_BGN: u16 = 43;
    pub(super) const BR_FLW_NJV_BGN: u16 = 44;
    pub(super) const R_DQT_STR_BGN: u16 = 45;
    pub(super) const BR_DQT_STR_BGN: u16 = 46;
    pub(super) const B_DQT_STR_BGN: u16 = 47;
    pub(super) const R_DQT_STR_CTN: u16 = 48;
    pub(super) const BR_DQT_STR_CTN: u16 = 49;
    pub(super) const R_DQT_ESC_NWL: u16 = 50;
    pub(super) const BR_DQT_ESC_NWL: u16 = 51;
    pub(super) const R_DQT_ESC_SEQ: u16 = 52;
    pub(super) const BR_DQT_ESC_SEQ: u16 = 53;
    pub(super) const R_DQT_STR_END: u16 = 54;
    pub(super) const BR_DQT_STR_END: u16 = 55;
    pub(super) const R_SQT_STR_BGN: u16 = 56;
    pub(super) const BR_SQT_STR_BGN: u16 = 57;
    pub(super) const B_SQT_STR_BGN: u16 = 58;
    pub(super) const R_SQT_STR_CTN: u16 = 59;
    pub(super) const BR_SQT_STR_CTN: u16 = 60;
    pub(super) const R_SQT_ESC_SQT: u16 = 61;
    pub(super) const BR_SQT_ESC_SQT: u16 = 62;
    pub(super) const R_SQT_STR_END: u16 = 63;
    pub(super) const BR_SQT_STR_END: u16 = 64;
    pub(super) const R_SGL_PLN_NUL_BLK: u16 = 65;
    pub(super) const BR_SGL_PLN_NUL_BLK: u16 = 66;
    pub(super) const B_SGL_PLN_NUL_BLK: u16 = 67;
    pub(super) const R_SGL_PLN_NUL_FLW: u16 = 68;
    pub(super) const BR_SGL_PLN_NUL_FLW: u16 = 69;
    pub(super) const R_SGL_PLN_BOL_BLK: u16 = 70;
    pub(super) const BR_SGL_PLN_BOL_BLK: u16 = 71;
    pub(super) const B_SGL_PLN_BOL_BLK: u16 = 72;
    pub(super) const R_SGL_PLN_BOL_FLW: u16 = 73;
    pub(super) const BR_SGL_PLN_BOL_FLW: u16 = 74;
    pub(super) const R_SGL_PLN_INT_BLK: u16 = 75;
    pub(super) const BR_SGL_PLN_INT_BLK: u16 = 76;
    pub(super) const B_SGL_PLN_INT_BLK: u16 = 77;
    pub(super) const R_SGL_PLN_INT_FLW: u16 = 78;
    pub(super) const BR_SGL_PLN_INT_FLW: u16 = 79;
    pub(super) const R_SGL_PLN_FLT_BLK: u16 = 80;
    pub(super) const BR_SGL_PLN_FLT_BLK: u16 = 81;
    pub(super) const B_SGL_PLN_FLT_BLK: u16 = 82;
    pub(super) const R_SGL_PLN_FLT_FLW: u16 = 83;
    pub(super) const BR_SGL_PLN_FLT_FLW: u16 = 84;
    pub(super) const R_SGL_PLN_TMS_BLK: u16 = 85;
    pub(super) const BR_SGL_PLN_TMS_BLK: u16 = 86;
    pub(super) const B_SGL_PLN_TMS_BLK: u16 = 87;
    pub(super) const R_SGL_PLN_TMS_FLW: u16 = 88;
    pub(super) const BR_SGL_PLN_TMS_FLW: u16 = 89;
    pub(super) const R_SGL_PLN_STR_BLK: u16 = 90;
    pub(super) const BR_SGL_PLN_STR_BLK: u16 = 91;
    pub(super) const B_SGL_PLN_STR_BLK: u16 = 92;
    pub(super) const R_SGL_PLN_STR_FLW: u16 = 93;
    pub(super) const BR_SGL_PLN_STR_FLW: u16 = 94;
    pub(super) const R_MTL_PLN_STR_BLK: u16 = 95;
    pub(super) const BR_MTL_PLN_STR_BLK: u16 = 96;
    pub(super) const R_MTL_PLN_STR_FLW: u16 = 97;
    pub(super) const BR_MTL_PLN_STR_FLW: u16 = 98;
    pub(super) const R_TAG: u16 = 99;
    pub(super) const BR_TAG: u16 = 100;
    pub(super) const B_TAG: u16 = 101;
    pub(super) const R_ACR_BGN: u16 = 102;
    pub(super) const BR_ACR_BGN: u16 = 103;
    pub(super) const B_ACR_BGN: u16 = 104;
    pub(super) const R_ACR_CTN: u16 = 105;
    pub(super) const R_ALS_BGN: u16 = 106;
    pub(super) const BR_ALS_BGN: u16 = 107;
    pub(super) const B_ALS_BGN: u16 = 108;
    pub(super) const R_ALS_CTN: u16 = 109;
    pub(super) const BL: u16 = 110;
    pub(super) const COMMENT: u16 = 111;
    pub(super) const ERR_REC: u16 = 112;
}

#[allow(
    clippy::wildcard_imports,
    reason = "the token names are the C's vocabulary, used as bare names throughout as scanner.c does"
)]
use tok::*;

/// `scn_*`'s three answers.
const SCN_SUCC: i8 = 1;
const SCN_STOP: i8 = 0;
const SCN_FAIL: i8 = -1;

/// What an indentation level is: the root, a block mapping, a block
/// sequence, a block scalar's text.
const IND_ROT: i16 = b'r' as i16;
const IND_MAP: i16 = b'm' as i16;
const IND_SEQ: i16 = b'q' as i16;
const IND_STR: i16 = b's' as i16;

/// What the core schema resolves a plain scalar to (`ResultSchema`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResultSchema {
    Str,
    Int,
    Null,
    Bool,
    Float,
}

/// The schema's state once it can no longer change its mind.
const SCH_STT_FRZ: i8 = -1;

/// Whether `c` is the character `ch`.
fn is(c: i32, ch: char) -> bool {
    c == ch as i32
}

fn in_range(c: i32, low: char, high: char) -> bool {
    (low as i32..=high as i32).contains(&c)
}

fn is_wsp(c: i32) -> bool {
    is(c, ' ') || is(c, '\t')
}

fn is_nwl(c: i32) -> bool {
    is(c, '\r') || is(c, '\n')
}

fn is_wht(c: i32) -> bool {
    is_wsp(c) || is_nwl(c) || c == 0
}

fn is_ns_dec_digit(c: i32) -> bool {
    in_range(c, '0', '9')
}

fn is_ns_hex_digit(c: i32) -> bool {
    is_ns_dec_digit(c) || in_range(c, 'a', 'f') || in_range(c, 'A', 'F')
}

fn is_ns_word_char(c: i32) -> bool {
    is(c, '-') || in_range(c, '0', '9') || in_range(c, 'a', 'z') || in_range(c, 'A', 'Z')
}

fn is_nb_json(c: i32) -> bool {
    c == 0x09 || (0x20..=0x0010_ffff).contains(&c)
}

fn is_nb_double_char(c: i32) -> bool {
    is_nb_json(c) && !is(c, '\\') && !is(c, '"')
}

fn is_nb_single_char(c: i32) -> bool {
    is_nb_json(c) && !is(c, '\'')
}

fn is_ns_char(c: i32) -> bool {
    (0x21..=0x7e).contains(&c)
        || c == 0x85
        || (0xa0..=0xd7ff).contains(&c)
        || (0xe000..=0xfefe).contains(&c)
        || (0xff00..=0xfffd).contains(&c)
        || (0x0001_0000..=0x0010_ffff).contains(&c)
}

fn is_c_indicator(c: i32) -> bool {
    "-?:,[]{}#&*!|>'\"%@`".chars().any(|i| is(c, i))
}

fn is_c_flow_indicator(c: i32) -> bool {
    ",[]{}".chars().any(|i| is(c, i))
}

fn is_plain_safe_in_block(c: i32) -> bool {
    is_ns_char(c)
}

fn is_plain_safe_in_flow(c: i32) -> bool {
    is_ns_char(c) && !is_c_flow_indicator(c)
}

fn is_ns_uri_char(c: i32) -> bool {
    is_ns_word_char(c) || "#;/?:@&=+$,_.!~*'()[]".chars().any(|i| is(c, i))
}

fn is_ns_tag_char(c: i32) -> bool {
    is_ns_word_char(c) || "#;/?:@&=+$_.~*'()".chars().any(|i| is(c, i))
}

fn is_ns_anchor_char(c: i32) -> bool {
    is_ns_char(c) && !is_c_flow_indicator(c)
}

/// One step of the schema's state machine: move to a state, having
/// resolved the scalar so far; or stay, having perhaps resolved it.
enum Step {
    Move(ResultSchema, i8),
    Stay(Option<ResultSchema>),
}

/// `adv_sch_stt` from schema.core.c: the core schema's state machine over
/// a plain scalar's characters, one at a time, setting what the scalar
/// resolves to so far and answering the next state.
#[allow(
    clippy::too_many_lines,
    clippy::match_same_arms,
    reason = "a generated state machine, ported arm for arm so it can be checked against the C"
)]
fn adv_sch_stt(sch_stt: i8, cur_chr: i32, rlt_sch: &mut ResultSchema) -> i8 {
    use ResultSchema::{Bool, Float, Int, Null, Str};
    use Step::{Move, Stay};
    let c = cur_chr;
    // `if (cond) { *rlt_sch = s; return n; }`, or nothing.
    let when = |cond: bool, schema: ResultSchema, state: i8| {
        if cond {
            Move(schema, state)
        } else {
            Stay(None)
        }
    };
    let step = match sch_stt {
        SCH_STT_FRZ => Stay(None),
        0 => {
            if is(c, '.') {
                Move(Str, 6)
            } else if is(c, '0') {
                Move(Int, 37)
            } else if is(c, 'F') {
                Move(Str, 2)
            } else if is(c, 'N') {
                Move(Str, 16)
            } else if is(c, 'T') {
                Move(Str, 13)
            } else if is(c, 'f') {
                Move(Str, 17)
            } else if is(c, 'n') {
                Move(Str, 29)
            } else if is(c, 't') {
                Move(Str, 26)
            } else if is(c, '~') {
                Move(Null, 35)
            } else if is(c, '+') || is(c, '-') {
                Move(Str, 1)
            } else {
                when(in_range(c, '1', '9'), Int, 38)
            }
        }
        1 => {
            if is(c, '.') {
                Move(Str, 7)
            } else {
                when(in_range(c, '0', '9'), Int, 38)
            }
        }
        2 => {
            if is(c, 'A') {
                Move(Str, 9)
            } else {
                when(is(c, 'a'), Str, 22)
            }
        }
        3 => when(is(c, 'A') || is(c, 'a'), Str, 12),
        4 => when(is(c, 'E'), Bool, 36),
        5 => when(is(c, 'F'), Float, 41),
        6 => {
            if is(c, 'I') {
                Move(Str, 11)
            } else if is(c, 'N') {
                Move(Str, 3)
            } else if is(c, 'i') {
                Move(Str, 24)
            } else if is(c, 'n') {
                Move(Str, 18)
            } else {
                when(in_range(c, '0', '9'), Float, 42)
            }
        }
        7 => {
            if is(c, 'I') {
                Move(Str, 11)
            } else if is(c, 'i') {
                Move(Str, 24)
            } else {
                when(in_range(c, '0', '9'), Float, 42)
            }
        }
        8 => when(is(c, 'L'), Null, 35),
        9 => when(is(c, 'L'), Str, 14),
        10 => when(is(c, 'L'), Str, 8),
        11 => {
            if is(c, 'N') {
                Move(Str, 5)
            } else {
                when(is(c, 'n'), Str, 20)
            }
        }
        12 => when(is(c, 'N'), Float, 41),
        13 => {
            if is(c, 'R') {
                Move(Str, 15)
            } else {
                when(is(c, 'r'), Str, 28)
            }
        }
        14 => when(is(c, 'S'), Str, 4),
        15 => when(is(c, 'U'), Str, 4),
        16 => {
            if is(c, 'U') {
                Move(Str, 10)
            } else {
                when(is(c, 'u'), Str, 23)
            }
        }
        17 => when(is(c, 'a'), Str, 22),
        18 => when(is(c, 'a'), Str, 25),
        19 => when(is(c, 'e'), Bool, 36),
        20 => when(is(c, 'f'), Float, 41),
        21 => when(is(c, 'l'), Null, 35),
        22 => when(is(c, 'l'), Str, 27),
        23 => when(is(c, 'l'), Str, 21),
        24 => when(is(c, 'n'), Str, 20),
        25 => when(is(c, 'n'), Float, 41),
        26 => when(is(c, 'r'), Str, 28),
        27 => when(is(c, 's'), Str, 19),
        28 => when(is(c, 'u'), Str, 19),
        29 => when(is(c, 'u'), Str, 23),
        30 => {
            if is(c, '+') || is(c, '-') {
                Move(Str, 32)
            } else {
                when(in_range(c, '0', '9'), Float, 43)
            }
        }
        31 => when(in_range(c, '0', '7'), Int, 39),
        32 => when(in_range(c, '0', '9'), Float, 43),
        33 => when(is_ns_hex_digit(c), Int, 40),
        35 => Stay(Some(Null)),
        36 => Stay(Some(Bool)),
        // An integer so far, which the character may take further.
        37 => {
            if is(c, '.') {
                Move(Float, 42)
            } else if is(c, 'o') {
                Move(Str, 31)
            } else if is(c, 'x') {
                Move(Str, 33)
            } else if is(c, 'E') || is(c, 'e') {
                Move(Str, 30)
            } else if in_range(c, '0', '9') {
                Move(Int, 38)
            } else {
                Stay(Some(Int))
            }
        }
        38 => {
            if is(c, '.') {
                Move(Float, 42)
            } else if is(c, 'E') || is(c, 'e') {
                Move(Str, 30)
            } else if in_range(c, '0', '9') {
                Move(Int, 38)
            } else {
                Stay(Some(Int))
            }
        }
        39 => {
            if in_range(c, '0', '7') {
                Move(Int, 39)
            } else {
                Stay(Some(Int))
            }
        }
        40 => {
            if is_ns_hex_digit(c) {
                Move(Int, 40)
            } else {
                Stay(Some(Int))
            }
        }
        41 => Stay(Some(Float)),
        42 => {
            if is(c, 'E') || is(c, 'e') {
                Move(Str, 30)
            } else if in_range(c, '0', '9') {
                Move(Float, 42)
            } else {
                Stay(Some(Float))
            }
        }
        43 => {
            if in_range(c, '0', '9') {
                Move(Float, 43)
            } else {
                Stay(Some(Float))
            }
        }
        // The C's `default` (and its unreachable 34, which aborts there).
        _ => {
            *rlt_sch = Str;
            return SCH_STT_FRZ;
        }
    };
    match step {
        Move(schema, state) => {
            *rlt_sch = schema;
            state
        }
        Stay(schema) => {
            if let Some(schema) = schema {
                *rlt_sch = schema;
            }
            // Every arm that did not move falls to here, as the C's
            // `break`s do: a character that ends nothing makes the scalar
            // a string.
            if !is(c, '\r') && !is(c, '\n') && !is(c, ' ') && c != 0 {
                *rlt_sch = Str;
            }
            SCH_STT_FRZ
        }
    }
}

/// The token a single-line plain scalar is, by what it resolved to, where
/// it began (`pos`: 0 on the same line, 1 on a later line indented deeper,
/// 2 at the block's own indentation) and whether it is in a block or a flow
/// (`SGL_PLN_SYM`). The tokens come five to a kind -- R, BR and B in a
/// block, R and BR in a flow -- null, bool, int, float, timestamp, string.
fn sgl_pln_sym(schema: ResultSchema, pos: u16, flow: bool) -> u16 {
    let kind: u16 = match schema {
        ResultSchema::Null => 0,
        ResultSchema::Bool => 1,
        ResultSchema::Int => 2,
        ResultSchema::Float => 3,
        // The core schema has no timestamps (`HAS_TIMESTAMP` 0), so a
        // scalar is never the fifth kind.
        ResultSchema::Str => 5,
    };
    let offset: u16 = if flow { 3 } else { 0 };
    R_SGL_PLN_NUL_BLK
        .saturating_add(kind.saturating_mul(5))
        .saturating_add(offset)
        .saturating_add(pos)
}

/// The scanner's state: the position it has read to, the implicit key's
/// place, and the stack of indentation levels and their kinds -- and, for
/// the length of one scan, where it is and what it has resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Scanner {
    row: i16,
    col: i16,
    blk_imp_row: i16,
    blk_imp_col: i16,
    blk_imp_tab: i16,
    ind_typ_stk: Vec<i16>,
    ind_len_stk: Vec<i16>,
    // For the length of one scan.
    end_row: i16,
    end_col: i16,
    cur_row: i16,
    cur_col: i16,
    cur_chr: i32,
    sch_stt: i8,
    rlt_sch: ResultSchema,
}

impl Default for Scanner {
    /// What the C's `create` makes: zeroed, then the empty state restored.
    fn default() -> Self {
        let mut scanner = Self {
            row: 0,
            col: 0,
            blk_imp_row: 0,
            blk_imp_col: 0,
            blk_imp_tab: 0,
            ind_typ_stk: Vec::new(),
            ind_len_stk: Vec::new(),
            end_row: 0,
            end_col: 0,
            cur_row: 0,
            cur_col: 0,
            cur_chr: 0,
            sch_stt: 0,
            rlt_sch: ResultSchema::Str,
        };
        scanner.deserialize(&[]);
        scanner
    }
}

/// The scanner's methods, which are the C's static functions under their
/// names.
impl Scanner {
    fn adv(&mut self, lexer: &mut Lexer<'_>) {
        self.cur_col = self.cur_col.wrapping_add(1);
        self.cur_chr = lexer.lookahead();
        lexer.advance();
    }

    fn adv_nwl(&mut self, lexer: &mut Lexer<'_>) {
        self.cur_row = self.cur_row.wrapping_add(1);
        self.cur_col = 0;
        self.cur_chr = lexer.lookahead();
        lexer.advance();
    }

    fn skp(&mut self, lexer: &mut Lexer<'_>) {
        self.cur_col = self.cur_col.wrapping_add(1);
        self.cur_chr = lexer.lookahead();
        lexer.skip();
    }

    fn skp_nwl(&mut self, lexer: &mut Lexer<'_>) {
        self.cur_row = self.cur_row.wrapping_add(1);
        self.cur_col = 0;
        self.cur_chr = lexer.lookahead();
        lexer.skip();
    }

    fn mrk_end(&mut self, lexer: &mut Lexer<'_>) {
        self.end_row = self.cur_row;
        self.end_col = self.cur_col;
        lexer.mark_end();
    }

    fn init(&mut self) {
        self.cur_row = self.row;
        self.cur_col = self.col;
        self.cur_chr = 0;
        self.sch_stt = 0;
        self.rlt_sch = ResultSchema::Str;
    }

    fn flush(&mut self) {
        self.row = self.end_row;
        self.col = self.end_col;
    }

    fn pop_ind(&mut self) {
        self.ind_len_stk.pop();
        self.ind_typ_stk.pop();
    }

    fn push_ind(&mut self, typ: i16, len: i16) {
        self.ind_len_stk.push(len);
        self.ind_typ_stk.push(typ);
    }

    /// `RET_SYM`: the token is `symbol`, and the scanner's position is where
    /// it ended.
    fn ret(&mut self, lexer: &mut Lexer<'_>, symbol: u16) -> bool {
        self.flush();
        lexer.set_result(symbol);
        true
    }

    /// `POP_IND`: close the innermost level -- or, with only the root left
    /// (a state error recovery left behind), answer that nothing can be.
    fn pop_ind_checked(&mut self) -> bool {
        if self.ind_typ_stk.len() == 1 {
            return false;
        }
        self.pop_ind();
        true
    }

    /// The innermost indentation's length.
    fn ind_len(&self) -> i16 {
        self.ind_len_stk.last().copied().unwrap_or(-1)
    }

    fn scn_uri_esc(&mut self, lexer: &mut Lexer<'_>) -> i8 {
        if !is(lexer.lookahead(), '%') {
            return SCN_STOP;
        }
        self.mrk_end(lexer);
        self.adv(lexer);
        if !is_ns_hex_digit(lexer.lookahead()) {
            return SCN_FAIL;
        }
        self.adv(lexer);
        if !is_ns_hex_digit(lexer.lookahead()) {
            return SCN_FAIL;
        }
        self.adv(lexer);
        SCN_SUCC
    }

    fn scn_ns_uri_char(&mut self, lexer: &mut Lexer<'_>) -> i8 {
        if is_ns_uri_char(lexer.lookahead()) {
            self.adv(lexer);
            return SCN_SUCC;
        }
        self.scn_uri_esc(lexer)
    }

    fn scn_ns_tag_char(&mut self, lexer: &mut Lexer<'_>) -> i8 {
        if is_ns_tag_char(lexer.lookahead()) {
            self.adv(lexer);
            return SCN_SUCC;
        }
        self.scn_uri_esc(lexer)
    }

    /// Advance over `word`'s characters while they match, answering whether
    /// all did -- the C's nested `if`s for `YAML` and `TAG`.
    fn adv_word(&mut self, lexer: &mut Lexer<'_>, word: &str) -> bool {
        for ch in word.chars() {
            if !is(lexer.lookahead(), ch) {
                return false;
            }
            self.adv(lexer);
        }
        true
    }

    fn scn_dir_bgn(&mut self, lexer: &mut Lexer<'_>) -> bool {
        self.adv(lexer);
        if is(lexer.lookahead(), 'Y') {
            if self.adv_word(lexer, "YAML") && is_wht(lexer.lookahead()) {
                self.mrk_end(lexer);
                return self.ret(lexer, S_DIR_YML_BGN);
            }
        } else if is(lexer.lookahead(), 'T')
            && self.adv_word(lexer, "TAG")
            && is_wht(lexer.lookahead())
        {
            self.mrk_end(lexer);
            return self.ret(lexer, S_DIR_TAG_BGN);
        }
        while is_ns_char(lexer.lookahead()) {
            self.adv(lexer);
        }
        if self.cur_col > 1 && is_wht(lexer.lookahead()) {
            self.mrk_end(lexer);
            return self.ret(lexer, S_DIR_RSV_BGN);
        }
        false
    }

    fn scn_dir_yml_ver(&mut self, lexer: &mut Lexer<'_>, result_symbol: u16) -> bool {
        let mut n1: u16 = 0;
        let mut n2: u16 = 0;
        while is_ns_dec_digit(lexer.lookahead()) {
            self.adv(lexer);
            n1 = n1.wrapping_add(1);
        }
        if !is(lexer.lookahead(), '.') {
            return false;
        }
        self.adv(lexer);
        while is_ns_dec_digit(lexer.lookahead()) {
            self.adv(lexer);
            n2 = n2.wrapping_add(1);
        }
        if n1 == 0 || n2 == 0 {
            return false;
        }
        self.mrk_end(lexer);
        self.ret(lexer, result_symbol)
    }

    fn scn_tag_hdl_tal(&mut self, lexer: &mut Lexer<'_>) -> bool {
        if is(lexer.lookahead(), '!') {
            self.adv(lexer);
            return true;
        }
        let mut n: u16 = 0;
        while is_ns_word_char(lexer.lookahead()) {
            self.adv(lexer);
            n = n.wrapping_add(1);
        }
        if n == 0 {
            return true;
        }
        if is(lexer.lookahead(), '!') {
            self.adv(lexer);
            return true;
        }
        false
    }

    fn scn_dir_tag_hdl(&mut self, lexer: &mut Lexer<'_>, result_symbol: u16) -> bool {
        if is(lexer.lookahead(), '!') {
            self.adv(lexer);
            if self.scn_tag_hdl_tal(lexer) {
                self.mrk_end(lexer);
                return self.ret(lexer, result_symbol);
            }
        }
        false
    }

    fn scn_dir_tag_pfx(&mut self, lexer: &mut Lexer<'_>, result_symbol: u16) -> bool {
        if is(lexer.lookahead(), '!') {
            self.adv(lexer);
        } else if self.scn_ns_tag_char(lexer) != SCN_SUCC {
            return false;
        }
        loop {
            match self.scn_ns_uri_char(lexer) {
                SCN_STOP => {
                    self.mrk_end(lexer);
                    return self.ret(lexer, result_symbol);
                }
                SCN_FAIL => return self.ret(lexer, result_symbol),
                _ => {}
            }
        }
    }

    fn scn_dir_rsv_prm(&mut self, lexer: &mut Lexer<'_>, result_symbol: u16) -> bool {
        if !is_ns_char(lexer.lookahead()) {
            return false;
        }
        self.adv(lexer);
        while is_ns_char(lexer.lookahead()) {
            self.adv(lexer);
        }
        self.mrk_end(lexer);
        self.ret(lexer, result_symbol)
    }

    fn scn_tag(&mut self, lexer: &mut Lexer<'_>, result_symbol: u16) -> bool {
        if !is(lexer.lookahead(), '!') {
            return false;
        }
        self.adv(lexer);
        if is_wht(lexer.lookahead()) {
            self.mrk_end(lexer);
            return self.ret(lexer, result_symbol);
        }
        if is(lexer.lookahead(), '<') {
            self.adv(lexer);
            if self.scn_ns_uri_char(lexer) != SCN_SUCC {
                return false;
            }
            loop {
                match self.scn_ns_uri_char(lexer) {
                    SCN_STOP => {
                        if is(lexer.lookahead(), '>') {
                            self.adv(lexer);
                            self.mrk_end(lexer);
                            return self.ret(lexer, result_symbol);
                        }
                        // The C falls through to `SCN_FAIL`.
                        return false;
                    }
                    SCN_FAIL => return false,
                    _ => {}
                }
            }
        }
        if self.scn_tag_hdl_tal(lexer) && self.scn_ns_tag_char(lexer) != SCN_SUCC {
            return false;
        }
        loop {
            match self.scn_ns_tag_char(lexer) {
                SCN_STOP => {
                    self.mrk_end(lexer);
                    return self.ret(lexer, result_symbol);
                }
                SCN_FAIL => return self.ret(lexer, result_symbol),
                _ => {}
            }
        }
    }

    /// `scn_acr_bgn` and `scn_als_bgn`: `&` or `*`, and an anchor's first
    /// character after it.
    fn scn_anchor_bgn(&mut self, lexer: &mut Lexer<'_>, sigil: char, result_symbol: u16) -> bool {
        if !is(lexer.lookahead(), sigil) {
            return false;
        }
        self.adv(lexer);
        if !is_ns_anchor_char(lexer.lookahead()) {
            return false;
        }
        self.mrk_end(lexer);
        self.ret(lexer, result_symbol)
    }

    /// `scn_acr_ctn` and `scn_als_ctn`: the rest of an anchor's name.
    fn scn_anchor_ctn(&mut self, lexer: &mut Lexer<'_>, result_symbol: u16) -> bool {
        while is_ns_anchor_char(lexer.lookahead()) {
            self.adv(lexer);
        }
        self.mrk_end(lexer);
        self.ret(lexer, result_symbol)
    }

    fn scn_dqt_esc_seq(&mut self, lexer: &mut Lexer<'_>, result_symbol: u16) -> bool {
        let c = lexer.lookahead();
        let hex_digits = if is(c, 'U') {
            8
        } else if is(c, 'u') {
            4
        } else if is(c, 'x') {
            2
        } else if "0abt\tnvref \"/\\N_LP".chars().any(|e| is(c, e)) {
            0
        } else {
            return false;
        };
        self.adv(lexer);
        for _ in 0..hex_digits {
            if !is_ns_hex_digit(lexer.lookahead()) {
                return false;
            }
            self.adv(lexer);
        }
        self.mrk_end(lexer);
        self.ret(lexer, result_symbol)
    }

    /// Whether a line starts `---` or `...` followed by a blank: a document's
    /// start or end.
    fn scn_drs_doc_end(&mut self, lexer: &mut Lexer<'_>) -> bool {
        let delimiter = lexer.lookahead();
        if !is(delimiter, '-') && !is(delimiter, '.') {
            return false;
        }
        self.adv(lexer);
        if lexer.lookahead() == delimiter {
            self.adv(lexer);
            if lexer.lookahead() == delimiter {
                self.adv(lexer);
                if is_wht(lexer.lookahead()) {
                    return true;
                }
            }
        }
        self.mrk_end(lexer);
        false
    }

    /// `scn_dqt_str_cnt` and `scn_sqt_str_cnt`: a quoted string's text,
    /// which a document marker at the start of a line ends instead.
    fn scn_qt_str_cnt(
        &mut self,
        lexer: &mut Lexer<'_>,
        is_char: fn(i32) -> bool,
        result_symbol: u16,
    ) -> bool {
        if !is_char(lexer.lookahead()) {
            return false;
        }
        if self.cur_col == 0 && self.scn_drs_doc_end(lexer) {
            self.mrk_end(lexer);
            let marker = if is(self.cur_chr, '-') {
                S_DRS_END
            } else {
                S_DOC_END
            };
            return self.ret(lexer, marker);
        }
        self.adv(lexer);
        while is_char(lexer.lookahead()) {
            self.adv(lexer);
        }
        self.mrk_end(lexer);
        self.ret(lexer, result_symbol)
    }

    fn scn_blk_str_bgn(&mut self, lexer: &mut Lexer<'_>, result_symbol: u16) -> bool {
        if !is(lexer.lookahead(), '|') && !is(lexer.lookahead(), '>') {
            return false;
        }
        self.adv(lexer);
        let cur_ind = self.ind_len();
        let mut ind: i16 = -1;
        let digit = |c: i32| in_range(c, '1', '9');
        let indicator = |c: i32| is(c, '+') || is(c, '-');
        let as_ind = |c: i32| i16::try_from(c.saturating_sub(i32::from(b'1'))).unwrap_or(-1);
        if digit(lexer.lookahead()) {
            ind = as_ind(lexer.lookahead());
            self.adv(lexer);
            if indicator(lexer.lookahead()) {
                self.adv(lexer);
            }
        } else if indicator(lexer.lookahead()) {
            self.adv(lexer);
            if digit(lexer.lookahead()) {
                ind = as_ind(lexer.lookahead());
                self.adv(lexer);
            }
        }
        if !is_wht(lexer.lookahead()) {
            return false;
        }
        self.mrk_end(lexer);
        if ind == -1 {
            ind = cur_ind;
            while is_wsp(lexer.lookahead()) {
                self.adv(lexer);
            }
            if is(lexer.lookahead(), '#') {
                self.adv(lexer);
                while !is_nwl(lexer.lookahead()) && lexer.lookahead() != 0 {
                    self.adv(lexer);
                }
            }
            if is_nwl(lexer.lookahead()) {
                self.adv_nwl(lexer);
            }
            while lexer.lookahead() != 0 {
                let before = i32::from(self.cur_col).saturating_sub(1);
                if is(lexer.lookahead(), ' ') {
                    self.adv(lexer);
                } else if is_nwl(lexer.lookahead()) {
                    if before < i32::from(ind) {
                        break;
                    }
                    ind = self.cur_col.wrapping_sub(1);
                    self.adv_nwl(lexer);
                } else {
                    if before > i32::from(ind) {
                        ind = self.cur_col.wrapping_sub(1);
                    }
                    break;
                }
            }
        } else {
            ind = ind.wrapping_add(cur_ind);
        }
        self.push_ind(IND_STR, ind);
        self.ret(lexer, result_symbol)
    }

    fn scn_blk_str_cnt(&mut self, lexer: &mut Lexer<'_>, result_symbol: u16) -> bool {
        if !is_ns_char(lexer.lookahead()) {
            return false;
        }
        if self.cur_col == 0 && self.scn_drs_doc_end(lexer) {
            if !self.pop_ind_checked() {
                return false;
            }
            return self.ret(lexer, BL);
        }
        self.adv(lexer);
        self.mrk_end(lexer);
        loop {
            if is_ns_char(lexer.lookahead()) {
                self.adv(lexer);
                while is_ns_char(lexer.lookahead()) {
                    self.adv(lexer);
                }
                self.mrk_end(lexer);
            }
            if is_wsp(lexer.lookahead()) {
                self.adv(lexer);
                while is_wsp(lexer.lookahead()) {
                    self.adv(lexer);
                }
            } else {
                break;
            }
        }
        self.ret(lexer, result_symbol)
    }

    /// `MAY_UPD_IMP_COL`: note where an implicit key would begin, if this
    /// line has not noted one yet.
    fn may_upd_imp_col(&mut self, bgn_row: i16, bgn_col: i16, has_tab_ind: bool) {
        if self.blk_imp_row != bgn_row {
            self.blk_imp_row = bgn_row;
            self.blk_imp_col = bgn_col;
            self.blk_imp_tab = i16::from(has_tab_ind);
        }
    }

    /// A one-character token -- `[`, `]`, `{`, `}`, `,`, a quote -- noting
    /// an implicit key's place first when `bgn` is given.
    fn single(
        &mut self,
        lexer: &mut Lexer<'_>,
        symbol: u16,
        bgn: Option<(i16, i16, bool)>,
    ) -> bool {
        if let Some((row, col, tab)) = bgn {
            self.may_upd_imp_col(row, col, tab);
        }
        self.adv(lexer);
        self.mrk_end(lexer);
        self.ret(lexer, symbol)
    }

    /// Step the schema over the character just taken.
    fn adv_sch(&mut self) {
        self.sch_stt = adv_sch_stt(self.sch_stt, self.cur_chr, &mut self.rlt_sch);
    }

    fn scn_pln_cnt(&mut self, lexer: &mut Lexer<'_>, is_plain_safe: fn(i32) -> bool) -> i8 {
        let mut is_cur_saf = is_plain_safe(self.cur_chr);
        let mut is_lka_wsp = is_wsp(lexer.lookahead());
        let mut is_lka_saf = is_plain_safe(lexer.lookahead());
        if !is_lka_saf && !is_lka_wsp {
            return SCN_STOP;
        }
        loop {
            let c = lexer.lookahead();
            // The C's first two branches, which do the same: a safe
            // character that is neither `#` nor `:`, or a `#` straight after
            // a safe character (not a comment, then).
            if (is_lka_saf && !is(c, '#') && !is(c, ':')) || (is_cur_saf && is(c, '#')) {
                self.adv(lexer);
                self.mrk_end(lexer);
                self.adv_sch();
            } else if is_lka_wsp {
                self.adv(lexer);
                self.adv_sch();
            } else if is(c, ':') {
                // Checked below, once the character after it is known.
                self.adv(lexer);
            } else {
                break;
            }
            is_cur_saf = is_lka_saf;
            is_lka_wsp = is_wsp(lexer.lookahead());
            is_lka_saf = is_plain_safe(lexer.lookahead());
            if is(self.cur_chr, ':') {
                if is_lka_saf {
                    self.mrk_end(lexer);
                    self.adv_sch();
                } else {
                    return SCN_FAIL;
                }
            }
        }
        SCN_SUCC
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "scanner.c's `scan`, ported branch for branch so it can be checked against the C"
    )]
    fn scan(&mut self, lexer: &mut Lexer<'_>, valid_symbols: &[bool]) -> bool {
        let valid = |t: u16| valid_symbols.get(usize::from(t)).copied().unwrap_or(false);
        self.init();
        self.mrk_end(lexer);

        let allow_comment = !(valid(R_DQT_STR_CTN)
            || valid(BR_DQT_STR_CTN)
            || valid(R_SQT_STR_CTN)
            || valid(BR_SQT_STR_CTN));
        let cur_ind = self.ind_len();
        let prt_ind = match self.ind_len_stk.len().checked_sub(2) {
            Some(i) => self.ind_len_stk.get(i).copied().unwrap_or(-1),
            None => -1,
        };
        let cur_ind_typ = self.ind_typ_stk.last().copied().unwrap_or(IND_ROT);

        let mut has_tab_ind = false;
        let mut leading_spaces: i16 = 0;

        loop {
            let c = lexer.lookahead();
            if is(c, ' ') {
                if !has_tab_ind {
                    leading_spaces = leading_spaces.wrapping_add(1);
                }
                self.skp(lexer);
            } else if is(c, '\t') {
                has_tab_ind = true;
                self.skp(lexer);
            } else if is_nwl(c) {
                has_tab_ind = false;
                leading_spaces = 0;
                self.skp_nwl(lexer);
            } else if allow_comment && is(c, '#') {
                if valid(BR_BLK_STR_CTN) && valid(BL) && self.cur_col <= cur_ind {
                    if !self.pop_ind_checked() {
                        return false;
                    }
                    return self.ret(lexer, BL);
                }
                let comment_here = if valid(BR_BLK_STR_CTN) {
                    self.cur_row == self.row
                } else {
                    self.cur_col == 0 || self.cur_row != self.row || self.cur_col > self.col
                };
                if !comment_here {
                    break;
                }
                self.adv(lexer);
                while !is_nwl(lexer.lookahead()) && lexer.lookahead() != 0 {
                    self.adv(lexer);
                }
                self.mrk_end(lexer);
                return self.ret(lexer, COMMENT);
            } else {
                break;
            }
        }

        if lexer.lookahead() == 0 {
            if valid(BL) {
                self.mrk_end(lexer);
                if !self.pop_ind_checked() {
                    return false;
                }
                return self.ret(lexer, BL);
            }
            if valid(END_OF_FILE) {
                self.mrk_end(lexer);
                return self.ret(lexer, END_OF_FILE);
            }
            return false;
        }

        let bgn_row = self.cur_row;
        let bgn_col = self.cur_col;
        let bgn_chr = lexer.lookahead();

        if valid(BL) && bgn_col <= cur_ind && !has_tab_ind {
            let close = if cur_ind == prt_ind && cur_ind_typ == IND_SEQ {
                bgn_col < cur_ind || !is(lexer.lookahead(), '-')
            } else {
                bgn_col <= prt_ind || cur_ind_typ == IND_STR
            };
            if close {
                if !self.pop_ind_checked() {
                    return false;
                }
                return self.ret(lexer, BL);
            }
        }

        let has_nwl = self.cur_row > self.row;
        let is_r = !has_nwl;
        let is_br = has_nwl && leading_spaces > cur_ind;
        let is_b = has_nwl && leading_spaces == cur_ind && !has_tab_ind;
        let is_s = bgn_col == 0;

        // Where this token begins, for `MAY_UPD_IMP_COL`.
        let bgn = (bgn_row, bgn_col, has_tab_ind);

        if valid(R_DIR_YML_VER) && is_r {
            return self.scn_dir_yml_ver(lexer, R_DIR_YML_VER);
        }
        if valid(R_DIR_TAG_HDL) && is_r {
            return self.scn_dir_tag_hdl(lexer, R_DIR_TAG_HDL);
        }
        if valid(R_DIR_TAG_PFX) && is_r {
            return self.scn_dir_tag_pfx(lexer, R_DIR_TAG_PFX);
        }
        if valid(R_DIR_RSV_PRM) && is_r {
            return self.scn_dir_rsv_prm(lexer, R_DIR_RSV_PRM);
        }
        if valid(BR_BLK_STR_CTN) && is_br && self.scn_blk_str_cnt(lexer, BR_BLK_STR_CTN) {
            return true;
        }
        if (valid(R_DQT_STR_CTN)
            && is_r
            && self.scn_qt_str_cnt(lexer, is_nb_double_char, R_DQT_STR_CTN))
            || (valid(BR_DQT_STR_CTN)
                && is_br
                && self.scn_qt_str_cnt(lexer, is_nb_double_char, BR_DQT_STR_CTN))
        {
            return true;
        }
        if (valid(R_SQT_STR_CTN)
            && is_r
            && self.scn_qt_str_cnt(lexer, is_nb_single_char, R_SQT_STR_CTN))
            || (valid(BR_SQT_STR_CTN)
                && is_br
                && self.scn_qt_str_cnt(lexer, is_nb_single_char, BR_SQT_STR_CTN))
        {
            return true;
        }
        if valid(R_ACR_CTN) && is_r {
            return self.scn_anchor_ctn(lexer, R_ACR_CTN);
        }
        if valid(R_ALS_CTN) && is_r {
            return self.scn_anchor_ctn(lexer, R_ALS_CTN);
        }

        let c = lexer.lookahead();
        if is(c, '%') {
            if valid(S_DIR_YML_BGN) && is_s {
                return self.scn_dir_bgn(lexer);
            }
        } else if is(c, '*') || is(c, '&') {
            let (r, br, b) = if is(c, '*') {
                (R_ALS_BGN, BR_ALS_BGN, B_ALS_BGN)
            } else {
                (R_ACR_BGN, BR_ACR_BGN, B_ACR_BGN)
            };
            let sigil = if is(c, '*') { '*' } else { '&' };
            for (symbol, here) in [(r, is_r), (br, is_br), (b, is_b)] {
                if valid(symbol) && here {
                    self.may_upd_imp_col(bgn_row, bgn_col, has_tab_ind);
                    return self.scn_anchor_bgn(lexer, sigil, symbol);
                }
            }
        } else if is(c, '!') {
            for (symbol, here) in [(R_TAG, is_r), (BR_TAG, is_br), (B_TAG, is_b)] {
                if valid(symbol) && here {
                    self.may_upd_imp_col(bgn_row, bgn_col, has_tab_ind);
                    return self.scn_tag(lexer, symbol);
                }
            }
        } else if is(c, '[') {
            for (symbol, here) in [
                (R_FLW_SEQ_BGN, is_r),
                (BR_FLW_SEQ_BGN, is_br),
                (B_FLW_SEQ_BGN, is_b),
            ] {
                if valid(symbol) && here {
                    return self.single(lexer, symbol, Some(bgn));
                }
            }
        } else if is(c, ']') {
            // The C reports the B case as BR, as it has it.
            for (asked, symbol, here) in [
                (R_FLW_SEQ_END, R_FLW_SEQ_END, is_r),
                (BR_FLW_SEQ_END, BR_FLW_SEQ_END, is_br),
                (B_FLW_SEQ_END, BR_FLW_SEQ_END, is_b),
            ] {
                if valid(asked) && here {
                    return self.single(lexer, symbol, None);
                }
            }
        } else if is(c, '{') {
            for (symbol, here) in [
                (R_FLW_MAP_BGN, is_r),
                (BR_FLW_MAP_BGN, is_br),
                (B_FLW_MAP_BGN, is_b),
            ] {
                if valid(symbol) && here {
                    return self.single(lexer, symbol, Some(bgn));
                }
            }
        } else if is(c, '}') {
            // As `]`: the B case is reported as BR.
            for (asked, symbol, here) in [
                (R_FLW_MAP_END, R_FLW_MAP_END, is_r),
                (BR_FLW_MAP_END, BR_FLW_MAP_END, is_br),
                (B_FLW_MAP_END, BR_FLW_MAP_END, is_b),
            ] {
                if valid(asked) && here {
                    return self.single(lexer, symbol, None);
                }
            }
        } else if is(c, ',') {
            for (symbol, here) in [(R_FLW_SEP_BGN, is_r), (BR_FLW_SEP_BGN, is_br)] {
                if valid(symbol) && here {
                    return self.single(lexer, symbol, None);
                }
            }
        } else if is(c, '"') {
            for (symbol, here) in [
                (R_DQT_STR_BGN, is_r),
                (BR_DQT_STR_BGN, is_br),
                (B_DQT_STR_BGN, is_b),
            ] {
                if valid(symbol) && here {
                    return self.single(lexer, symbol, Some(bgn));
                }
            }
            for (symbol, here) in [(R_DQT_STR_END, is_r), (BR_DQT_STR_END, is_br)] {
                if valid(symbol) && here {
                    return self.single(lexer, symbol, None);
                }
            }
        } else if is(c, '\'') {
            for (symbol, here) in [
                (R_SQT_STR_BGN, is_r),
                (BR_SQT_STR_BGN, is_br),
                (B_SQT_STR_BGN, is_b),
            ] {
                if valid(symbol) && here {
                    return self.single(lexer, symbol, Some(bgn));
                }
            }
            for (end, escape, here) in [
                (R_SQT_STR_END, R_SQT_ESC_SQT, is_r),
                (BR_SQT_STR_END, BR_SQT_ESC_SQT, is_br),
            ] {
                if valid(end) && here {
                    self.adv(lexer);
                    // `''` inside a single-quoted string is a quote.
                    if is(lexer.lookahead(), '\'') {
                        self.adv(lexer);
                        self.mrk_end(lexer);
                        return self.ret(lexer, escape);
                    }
                    self.mrk_end(lexer);
                    return self.ret(lexer, end);
                }
            }
        } else if is(c, '?') {
            let is_r_blk_key_bgn = valid(R_BLK_KEY_BGN) && is_r;
            let is_br_blk_key_bgn = valid(BR_BLK_KEY_BGN) && is_br;
            let is_b_blk_key_bgn = valid(B_BLK_KEY_BGN) && is_b;
            let is_r_flw_key_bgn = valid(R_FLW_KEY_BGN) && is_r;
            let is_br_flw_key_bgn = valid(BR_FLW_KEY_BGN) && is_br;
            if is_r_blk_key_bgn
                || is_br_blk_key_bgn
                || is_b_blk_key_bgn
                || is_r_flw_key_bgn
                || is_br_flw_key_bgn
            {
                self.adv(lexer);
                if is_wht(lexer.lookahead()) {
                    self.mrk_end(lexer);
                    if is_r_blk_key_bgn || is_br_blk_key_bgn {
                        // `PUSH_BGN_IND`.
                        if has_tab_ind {
                            return false;
                        }
                        self.push_ind(IND_MAP, bgn_col);
                        let symbol = if is_r_blk_key_bgn {
                            R_BLK_KEY_BGN
                        } else {
                            BR_BLK_KEY_BGN
                        };
                        return self.ret(lexer, symbol);
                    }
                    if is_b_blk_key_bgn {
                        return self.ret(lexer, B_BLK_KEY_BGN);
                    }
                    if is_r_flw_key_bgn {
                        return self.ret(lexer, R_FLW_KEY_BGN);
                    }
                    if is_br_flw_key_bgn {
                        return self.ret(lexer, BR_FLW_KEY_BGN);
                    }
                }
            }
        } else if is(c, ':') {
            if valid(R_FLW_JSV_BGN) && is_r {
                return self.single(lexer, R_FLW_JSV_BGN, None);
            }
            if valid(BR_FLW_JSV_BGN) && is_br {
                return self.single(lexer, BR_FLW_JSV_BGN, None);
            }
            let is_r_blk_val_bgn = valid(R_BLK_VAL_BGN) && is_r;
            let is_br_blk_val_bgn = valid(BR_BLK_VAL_BGN) && is_br;
            let is_b_blk_val_bgn = valid(B_BLK_VAL_BGN) && is_b;
            let is_r_blk_imp_bgn = valid(R_BLK_IMP_BGN) && is_r;
            let is_r_flw_njv_bgn = valid(R_FLW_NJV_BGN) && is_r;
            let is_br_flw_njv_bgn = valid(BR_FLW_NJV_BGN) && is_br;
            if is_r_blk_val_bgn
                || is_br_blk_val_bgn
                || is_b_blk_val_bgn
                || is_r_blk_imp_bgn
                || is_r_flw_njv_bgn
                || is_br_flw_njv_bgn
            {
                self.adv(lexer);
                let is_lka_wht = is_wht(lexer.lookahead());
                if is_lka_wht {
                    if is_r_blk_val_bgn || is_br_blk_val_bgn {
                        // `PUSH_BGN_IND`.
                        if has_tab_ind {
                            return false;
                        }
                        self.push_ind(IND_MAP, bgn_col);
                        self.mrk_end(lexer);
                        let symbol = if is_r_blk_val_bgn {
                            R_BLK_VAL_BGN
                        } else {
                            BR_BLK_VAL_BGN
                        };
                        return self.ret(lexer, symbol);
                    }
                    if is_b_blk_val_bgn {
                        self.mrk_end(lexer);
                        return self.ret(lexer, B_BLK_VAL_BGN);
                    }
                    if is_r_blk_imp_bgn {
                        // `MAY_PUSH_IMP_IND`.
                        if cur_ind != self.blk_imp_col {
                            if self.blk_imp_tab != 0 {
                                return false;
                            }
                            let col = self.blk_imp_col;
                            self.push_ind(IND_MAP, col);
                        }
                        self.mrk_end(lexer);
                        return self.ret(lexer, R_BLK_IMP_BGN);
                    }
                }
                let la = lexer.lookahead();
                if is_lka_wht || is(la, ',') || is(la, ']') || is(la, '}') {
                    if is_r_flw_njv_bgn {
                        self.mrk_end(lexer);
                        return self.ret(lexer, R_FLW_NJV_BGN);
                    }
                    if is_br_flw_njv_bgn {
                        self.mrk_end(lexer);
                        return self.ret(lexer, BR_FLW_NJV_BGN);
                    }
                }
            }
        } else if is(c, '-') {
            let is_r_blk_seq_bgn = valid(R_BLK_SEQ_BGN) && is_r;
            let is_br_blk_seq_bgn = valid(BR_BLK_SEQ_BGN) && is_br;
            let is_b_blk_seq_bgn = valid(B_BLK_SEQ_BGN) && is_b;
            let is_s_drs_end = is_s;
            if is_r_blk_seq_bgn || is_br_blk_seq_bgn || is_b_blk_seq_bgn || is_s_drs_end {
                self.adv(lexer);
                if is_wht(lexer.lookahead()) {
                    if is_r_blk_seq_bgn || is_br_blk_seq_bgn {
                        // `PUSH_BGN_IND`.
                        if has_tab_ind {
                            return false;
                        }
                        self.push_ind(IND_SEQ, bgn_col);
                        self.mrk_end(lexer);
                        let symbol = if is_r_blk_seq_bgn {
                            R_BLK_SEQ_BGN
                        } else {
                            BR_BLK_SEQ_BGN
                        };
                        return self.ret(lexer, symbol);
                    }
                    if is_b_blk_seq_bgn {
                        // `MAY_PUSH_SPC_SEQ_IND`.
                        if cur_ind_typ == IND_MAP {
                            self.push_ind(IND_SEQ, bgn_col);
                        }
                        self.mrk_end(lexer);
                        return self.ret(lexer, B_BLK_SEQ_BGN);
                    }
                } else if is(lexer.lookahead(), '-') && is_s_drs_end {
                    self.adv(lexer);
                    if is(lexer.lookahead(), '-') {
                        self.adv(lexer);
                        if is_wht(lexer.lookahead()) {
                            if valid(BL) {
                                if !self.pop_ind_checked() {
                                    return false;
                                }
                                return self.ret(lexer, BL);
                            }
                            self.mrk_end(lexer);
                            return self.ret(lexer, S_DRS_END);
                        }
                    }
                }
            }
        } else if is(c, '.') {
            if is_s && self.adv_word(lexer, "...") && is_wht(lexer.lookahead()) {
                if valid(BL) {
                    if !self.pop_ind_checked() {
                        return false;
                    }
                    return self.ret(lexer, BL);
                }
                self.mrk_end(lexer);
                return self.ret(lexer, S_DOC_END);
            }
        } else if is(c, '\\') {
            let is_r_dqt_esc_nwl = valid(R_DQT_ESC_NWL) && is_r;
            let is_br_dqt_esc_nwl = valid(BR_DQT_ESC_NWL) && is_br;
            let is_r_dqt_esc_seq = valid(R_DQT_ESC_SEQ) && is_r;
            let is_br_dqt_esc_seq = valid(BR_DQT_ESC_SEQ) && is_br;
            if is_r_dqt_esc_nwl || is_br_dqt_esc_nwl || is_r_dqt_esc_seq || is_br_dqt_esc_seq {
                self.adv(lexer);
                if is_nwl(lexer.lookahead()) {
                    if is_r_dqt_esc_nwl {
                        self.mrk_end(lexer);
                        return self.ret(lexer, R_DQT_ESC_NWL);
                    }
                    if is_br_dqt_esc_nwl {
                        self.mrk_end(lexer);
                        return self.ret(lexer, BR_DQT_ESC_NWL);
                    }
                }
                if is_r_dqt_esc_seq {
                    return self.scn_dqt_esc_seq(lexer, R_DQT_ESC_SEQ);
                }
                if is_br_dqt_esc_seq {
                    return self.scn_dqt_esc_seq(lexer, BR_DQT_ESC_SEQ);
                }
                return false;
            }
        } else if is(c, '|') {
            if valid(R_BLK_LIT_BGN) && is_r {
                return self.scn_blk_str_bgn(lexer, R_BLK_LIT_BGN);
            }
            if valid(BR_BLK_LIT_BGN) && is_br {
                return self.scn_blk_str_bgn(lexer, BR_BLK_LIT_BGN);
            }
        } else if is(c, '>') {
            if valid(R_BLK_FLD_BGN) && is_r {
                return self.scn_blk_str_bgn(lexer, R_BLK_FLD_BGN);
            }
            if valid(BR_BLK_FLD_BGN) && is_br {
                return self.scn_blk_str_bgn(lexer, BR_BLK_FLD_BGN);
            }
        }

        let maybe_sgl_pln_blk = (valid(R_SGL_PLN_STR_BLK) && is_r)
            || (valid(BR_SGL_PLN_STR_BLK) && is_br)
            || (valid(B_SGL_PLN_STR_BLK) && is_b);
        let maybe_sgl_pln_flw =
            (valid(R_SGL_PLN_STR_FLW) && is_r) || (valid(BR_SGL_PLN_STR_FLW) && is_br);
        let maybe_mtl_pln_blk =
            (valid(R_MTL_PLN_STR_BLK) && is_r) || (valid(BR_MTL_PLN_STR_BLK) && is_br);
        let maybe_mtl_pln_flw =
            (valid(R_MTL_PLN_STR_FLW) && is_r) || (valid(BR_MTL_PLN_STR_FLW) && is_br);

        if maybe_sgl_pln_blk || maybe_sgl_pln_flw || maybe_mtl_pln_blk || maybe_mtl_pln_flw {
            let is_in_blk = maybe_sgl_pln_blk || maybe_mtl_pln_blk;
            let is_plain_safe: fn(i32) -> bool = if is_in_blk {
                is_plain_safe_in_block
            } else {
                is_plain_safe_in_flow
            };
            let moved = |s: &Self| i32::from(s.cur_col).saturating_sub(i32::from(bgn_col));
            if moved(self) == 0 {
                self.adv(lexer);
            }
            if moved(self) == 1 {
                let is_plain_first = (is_ns_char(bgn_chr) && !is_c_indicator(bgn_chr))
                    || ((is(bgn_chr, '-') || is(bgn_chr, '?') || is(bgn_chr, ':'))
                        && is_plain_safe(lexer.lookahead()));
                if !is_plain_first {
                    return false;
                }
                self.adv_sch();
            } else {
                // `..X`, `...X`, `--X`, `---X`: a string whatever follows.
                self.sch_stt = SCH_STT_FRZ;
            }

            self.mrk_end(lexer);

            loop {
                if !is_nwl(lexer.lookahead()) && self.scn_pln_cnt(lexer, is_plain_safe) != SCN_SUCC
                {
                    break;
                }
                if lexer.lookahead() == 0 || !is_nwl(lexer.lookahead()) {
                    break;
                }
                loop {
                    if is_nwl(lexer.lookahead()) {
                        self.adv_nwl(lexer);
                    } else if is_wsp(lexer.lookahead()) {
                        self.adv(lexer);
                    } else {
                        break;
                    }
                }
                if lexer.lookahead() == 0 || self.cur_col <= cur_ind {
                    break;
                }
                if self.cur_col == 0 && self.scn_drs_doc_end(lexer) {
                    break;
                }
            }

            if self.end_row == bgn_row {
                if maybe_sgl_pln_blk {
                    self.may_upd_imp_col(bgn_row, bgn_col, has_tab_ind);
                    let pos = if is_r {
                        0
                    } else if is_br {
                        1
                    } else {
                        2
                    };
                    return self.ret(lexer, sgl_pln_sym(self.rlt_sch, pos, false));
                }
                if maybe_sgl_pln_flw {
                    let pos = if is_r { 0 } else { 1 };
                    return self.ret(lexer, sgl_pln_sym(self.rlt_sch, pos, true));
                }
            } else {
                if maybe_mtl_pln_blk {
                    self.may_upd_imp_col(bgn_row, bgn_col, has_tab_ind);
                    let symbol = if is_r {
                        R_MTL_PLN_STR_BLK
                    } else {
                        BR_MTL_PLN_STR_BLK
                    };
                    return self.ret(lexer, symbol);
                }
                if maybe_mtl_pln_flw {
                    let symbol = if is_r {
                        R_MTL_PLN_STR_FLW
                    } else {
                        BR_MTL_PLN_STR_FLW
                    };
                    return self.ret(lexer, symbol);
                }
            }
            return false;
        }

        // As the C: true outside error recovery, with nothing said about
        // which token.
        !valid(ERR_REC)
    }
}

impl ExternalScanner for Scanner {
    const TOKENS: &'static [&'static str] = &[
        "_eof",
        "_s_dir_yml_bgn",
        "_r_dir_yml_ver",
        "_s_dir_tag_bgn",
        "_r_dir_tag_hdl",
        "_r_dir_tag_pfx",
        "_s_dir_rsv_bgn",
        "_r_dir_rsv_prm",
        "_s_drs_end",
        "_s_doc_end",
        "_r_blk_seq_bgn",
        "_br_blk_seq_bgn",
        "_b_blk_seq_bgn",
        "_r_blk_key_bgn",
        "_br_blk_key_bgn",
        "_b_blk_key_bgn",
        "_r_blk_val_bgn",
        "_br_blk_val_bgn",
        "_b_blk_val_bgn",
        "_r_blk_imp_bgn",
        "_r_blk_lit_bgn",
        "_br_blk_lit_bgn",
        "_r_blk_fld_bgn",
        "_br_blk_fld_bgn",
        "_br_blk_str_ctn",
        "_r_flw_seq_bgn",
        "_br_flw_seq_bgn",
        "_b_flw_seq_bgn",
        "_r_flw_seq_end",
        "_br_flw_seq_end",
        "_b_flw_seq_end",
        "_r_flw_map_bgn",
        "_br_flw_map_bgn",
        "_b_flw_map_bgn",
        "_r_flw_map_end",
        "_br_flw_map_end",
        "_b_flw_map_end",
        "_r_flw_sep_bgn",
        "_br_flw_sep_bgn",
        "_r_flw_key_bgn",
        "_br_flw_key_bgn",
        "_r_flw_jsv_bgn",
        "_br_flw_jsv_bgn",
        "_r_flw_njv_bgn",
        "_br_flw_njv_bgn",
        "_r_dqt_str_bgn",
        "_br_dqt_str_bgn",
        "_b_dqt_str_bgn",
        "_r_dqt_str_ctn",
        "_br_dqt_str_ctn",
        "_r_dqt_esc_nwl",
        "_br_dqt_esc_nwl",
        "_r_dqt_esc_seq",
        "_br_dqt_esc_seq",
        "_r_dqt_str_end",
        "_br_dqt_str_end",
        "_r_sqt_str_bgn",
        "_br_sqt_str_bgn",
        "_b_sqt_str_bgn",
        "_r_sqt_str_ctn",
        "_br_sqt_str_ctn",
        "_r_sqt_esc_sqt",
        "_br_sqt_esc_sqt",
        "_r_sqt_str_end",
        "_br_sqt_str_end",
        "_r_sgl_pln_nul_blk",
        "_br_sgl_pln_nul_blk",
        "_b_sgl_pln_nul_blk",
        "_r_sgl_pln_nul_flw",
        "_br_sgl_pln_nul_flw",
        "_r_sgl_pln_bol_blk",
        "_br_sgl_pln_bol_blk",
        "_b_sgl_pln_bol_blk",
        "_r_sgl_pln_bol_flw",
        "_br_sgl_pln_bol_flw",
        "_r_sgl_pln_int_blk",
        "_br_sgl_pln_int_blk",
        "_b_sgl_pln_int_blk",
        "_r_sgl_pln_int_flw",
        "_br_sgl_pln_int_flw",
        "_r_sgl_pln_flt_blk",
        "_br_sgl_pln_flt_blk",
        "_b_sgl_pln_flt_blk",
        "_r_sgl_pln_flt_flw",
        "_br_sgl_pln_flt_flw",
        "_r_sgl_pln_tms_blk",
        "_br_sgl_pln_tms_blk",
        "_b_sgl_pln_tms_blk",
        "_r_sgl_pln_tms_flw",
        "_br_sgl_pln_tms_flw",
        "_r_sgl_pln_str_blk",
        "_br_sgl_pln_str_blk",
        "_b_sgl_pln_str_blk",
        "_r_sgl_pln_str_flw",
        "_br_sgl_pln_str_flw",
        "_r_mtl_pln_str_blk",
        "_br_mtl_pln_str_blk",
        "_r_mtl_pln_str_flw",
        "_br_mtl_pln_str_flw",
        "_r_tag",
        "_br_tag",
        "_b_tag",
        "_r_acr_bgn",
        "_br_acr_bgn",
        "_b_acr_bgn",
        "_r_acr_ctn",
        "_r_als_bgn",
        "_br_als_bgn",
        "_b_als_bgn",
        "_r_als_ctn",
        "_bl",
        "comment",
        "_err_rec",
    ];

    fn scan(&mut self, lexer: &mut Lexer<'_>, valid: &[bool]) -> bool {
        Scanner::scan(self, lexer, valid)
    }

    fn serialize(&self, buffer: &mut [u8]) -> usize {
        let capacity = buffer.len();
        let mut size = 0usize;
        let mut put = |value: i16, size: &mut usize| -> bool {
            let Some(slot) = buffer.get_mut(*size..size.saturating_add(2)) else {
                return false;
            };
            slot.copy_from_slice(&value.to_le_bytes());
            *size = size.saturating_add(2);
            true
        };
        for value in [
            self.row,
            self.col,
            self.blk_imp_row,
            self.blk_imp_col,
            self.blk_imp_tab,
        ] {
            if !put(value, &mut size) {
                return size;
            }
        }
        // Every level but the root, which the empty state restores.
        for (typ, len) in self.ind_typ_stk.iter().zip(&self.ind_len_stk).skip(1) {
            // A pair whole or not at all.
            if size.saturating_add(4) > capacity {
                break;
            }
            put(*typ, &mut size);
            put(*len, &mut size);
        }
        size
    }

    fn deserialize(&mut self, bytes: &[u8]) {
        self.row = 0;
        self.col = 0;
        self.blk_imp_row = -1;
        self.blk_imp_col = -1;
        self.blk_imp_tab = 0;
        self.ind_typ_stk.clear();
        self.ind_typ_stk.push(IND_ROT);
        self.ind_len_stk.clear();
        self.ind_len_stk.push(-1);
        if bytes.is_empty() {
            return;
        }
        let mut values = bytes.chunks_exact(2).map(|pair| {
            i16::from_le_bytes([
                pair.first().copied().unwrap_or(0),
                pair.get(1).copied().unwrap_or(0),
            ])
        });
        let mut next = || values.next().unwrap_or(0);
        self.row = next();
        self.col = next();
        self.blk_imp_row = next();
        self.blk_imp_col = next();
        self.blk_imp_tab = next();
        let rest = bytes.get(10..).unwrap_or_default();
        for pair in rest.chunks_exact(4) {
            if let [a, b, c, d] = pair {
                self.ind_typ_stk.push(i16::from_le_bytes([*a, *b]));
                self.ind_len_stk.push(i16::from_le_bytes([*c, *d]));
            }
        }
    }
}
