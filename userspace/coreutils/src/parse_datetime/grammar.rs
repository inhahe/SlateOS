//! The parser: Bison's `yacc.c` LALR(1) driver over [`super::tables`], and
//! upstream's semantic actions, keyed by Bison's rule numbers.
//!
//! The driver is a transcription of `yyparse` from the Bison 3.8.2 skeleton
//! that built the tables, label for label (`yynewstate`, `yybackup`,
//! `yydefault`, `yyreduce`, `yyerrlab`), including the parts this grammar never
//! exercises — error recovery pops to the bottom of the stack and aborts, since
//! no rule mentions `error` — so that the pair stays correct if the tables are
//! regenerated from a grammar that does. The one deliberate simplification is
//! that the lexer hands back grammar symbols directly, so `YYTRANSLATE` happens
//! there ([`sym::translate_char`]) rather than here.
//!
//! What matters about using the real tables and the real driver, rather than a
//! hand-written parser that accepts the same strings: the grammar has 31
//! shift/reduce conflicts, resolved by the tables, and states that reduce
//! without reading a token. Both decide *when* each rule's action runs — and
//! the actions print `--debug` output and can abort — so a parser that agreed
//! on every accepted string could still disagree on what it reported and on
//! where it stopped.

use super::lex::{MER_24, yylex};
use super::tables::{
    YYCHECK, YYDEFACT, YYDEFGOTO, YYFINAL, YYLAST, YYNTOKENS, YYPACT, YYPACT_NINF, YYPGOTO, YYR1,
    YYR2, YYTABLE,
};
use super::{
    HOUR, ParserControl, RelativeTime, TextInt, Timespec, apply_relative_time, digits_to_date_time,
    set_hhmmss, time_zone_hhmm,
};

/// Grammar symbols, numbered as Bison numbered them (`yytname`): the tokens
/// the lexer returns, and nothing else — nonterminals are only ever indices
/// into the goto tables.
pub(super) mod sym {
    /// `"end of file"`.
    pub const YYEOF: u8 = 0;
    /// `error`: the recovery token, which no rule of this grammar uses.
    pub const YYERROR: u8 = 1;
    /// `"invalid token"`: anything the grammar has no symbol for.
    pub const YYUNDEF: u8 = 2;
    pub const T_AGO: u8 = 3;
    pub const T_DST: u8 = 4;
    pub const T_YEAR_UNIT: u8 = 5;
    pub const T_MONTH_UNIT: u8 = 6;
    pub const T_HOUR_UNIT: u8 = 7;
    pub const T_MINUTE_UNIT: u8 = 8;
    pub const T_SEC_UNIT: u8 = 9;
    pub const T_DAY_UNIT: u8 = 10;
    pub const T_DAY_SHIFT: u8 = 11;
    pub const T_DAY: u8 = 12;
    pub const T_DAYZONE: u8 = 13;
    pub const T_LOCAL_ZONE: u8 = 14;
    pub const T_MERIDIAN: u8 = 15;
    pub const T_MONTH: u8 = 16;
    pub const T_ORDINAL: u8 = 17;
    pub const T_ZONE: u8 = 18;
    pub const T_SNUMBER: u8 = 19;
    pub const T_UNUMBER: u8 = 20;
    pub const T_SDECIMAL_NUMBER: u8 = 21;
    pub const T_UDECIMAL_NUMBER: u8 = 22;
    /// `'@'`.
    pub const AT: u8 = 23;
    /// `'J'`: the military zone that means local time.
    pub const J: u8 = 24;
    /// `'T'`: a military zone, and ISO 8601's date/time separator.
    pub const T: u8 = 25;
    /// `':'`.
    pub const COLON: u8 = 26;
    /// `','`.
    pub const COMMA: u8 = 27;
    /// `'/'`.
    pub const SLASH: u8 = 28;

    /// `YYTRANSLATE` for a byte the lexer returns as itself. NUL is the end
    /// of the string; every other byte without a symbol of its own is
    /// `"invalid token"`.
    pub fn translate_char(c: u8) -> u8 {
        match c {
            0 => YYEOF,
            b'@' => AT,
            b'J' => J,
            b'T' => T,
            b':' => COLON,
            b',' => COMMA,
            b'/' => SLASH,
            _ => YYUNDEF,
        }
    }
}

/// A semantic value: upstream's `%union`, as the variant it holds.
///
/// A C union read as the wrong member reinterprets memory; this reads as
/// "abort the parse" instead (see [`Value::int`] and friends). With the real
/// tables and the real actions no rule ever does it, which the tests pin, so
/// the difference is only in what a porting mistake would look like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Value {
    /// No value: a character token, or a symbol no action gives one.
    None,
    /// `intval`.
    Int(i64),
    /// `textintval`.
    TextInt(TextInt),
    /// `timespec`.
    Timespec(Timespec),
    /// `rel`.
    Rel(RelativeTime),
}

/// `YYABORT`.
#[derive(Debug)]
pub(super) struct Abort;

impl Value {
    fn int(self) -> Result<i64, Abort> {
        match self {
            Value::Int(v) => Ok(v),
            _ => Err(Abort),
        }
    }

    fn textint(self) -> Result<TextInt, Abort> {
        match self {
            Value::TextInt(v) => Ok(v),
            _ => Err(Abort),
        }
    }

    fn timespec(self) -> Result<Timespec, Abort> {
        match self {
            Value::Timespec(v) => Ok(v),
            _ => Err(Abort),
        }
    }

    fn rel(self) -> Result<RelativeTime, Abort> {
        match self {
            Value::Rel(v) => Ok(v),
            _ => Err(Abort),
        }
    }
}

/// What `yyparse` returns: upstream's 0, 1 and 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    /// `YYACCEPT`.
    Accept,
    /// `YYABORT`, from a syntax error or an action.
    Abort,
    /// `YYNOMEM`: the stack outgrew `YYMAXDEPTH`.
    Exhausted,
}

/// `YYMAXDEPTH`, which upstream sets to 20 (and `YYINITDEPTH` with it, so the
/// stack never grows): plenty for a grammar that is not right-recursive.
const YYMAXDEPTH: usize = 20;

/// Upstream's `yypact_value_is_default`.
fn pact_is_default(n: i8) -> bool {
    n == YYPACT_NINF
}

/// `yytable[yyn]` if `yycheck[yyn]` says the entry belongs to `symbol` —
/// the lookup `yybackup`, `yyreduce` and `yyerrlab1` all make.
fn checked_entry(base: i32, symbol: i32) -> Option<i8> {
    let index = base.checked_add(symbol)?;
    let i = usize::try_from(index).ok().filter(|&i| i <= YYLAST)?;
    if i32::from(*YYCHECK.get(i)?) == symbol {
        YYTABLE.get(i).copied()
    } else {
        None
    }
}

/// What `yybackup`/`yydefault` decide for the current state.
enum Action {
    Shift(usize),
    Reduce(usize),
    Error,
}

/// `yyparse`.
pub(super) fn yyparse(pc: &mut ParserControl<'_, '_>) -> Outcome {
    let mut yystate: usize = 0;
    let mut yyerrstatus: u8 = 0;
    let mut ss: Vec<usize> = Vec::with_capacity(YYMAXDEPTH);
    // `yyvsp` starts on a slot that belongs to state 0 and is never read.
    let mut vs: Vec<Value> = Vec::with_capacity(YYMAXDEPTH);
    vs.push(Value::None);
    // `yychar`: `None` is `YYEMPTY`.
    let mut lookahead: Option<(u8, Value)> = None;

    loop {
        // yynewstate / yysetstate.
        ss.push(yystate);
        if ss.len() >= YYMAXDEPTH {
            return Outcome::Exhausted;
        }
        if yystate == YYFINAL {
            return Outcome::Accept;
        }

        // yybackup: decide without the lookahead if the state allows it.
        let action = match YYPACT.get(yystate).copied() {
            None => Action::Error,
            Some(pact) if pact_is_default(pact) => default_action(yystate),
            Some(pact) => {
                let (token, _) = *lookahead.get_or_insert_with(|| yylex(pc));
                match checked_entry(i32::from(pact), i32::from(token)) {
                    None => default_action(yystate),
                    // yytable_value_is_error is constant 0 for these tables.
                    Some(n) if n <= 0 => Action::Reduce(usize::from(n.unsigned_abs())),
                    Some(n) => Action::Shift(usize::from(n.unsigned_abs())),
                }
            }
        };

        match action {
            Action::Shift(next) => {
                yyerrstatus = yyerrstatus.saturating_sub(1);
                yystate = next;
                let (_, value) = lookahead.take().unwrap_or((sym::YYEOF, Value::None));
                vs.push(value);
            }
            Action::Reduce(rule) => {
                let Some(len) = YYR2.get(rule).and_then(|&n| usize::try_from(n).ok()) else {
                    return Outcome::Abort;
                };
                let Some(base) = vs.len().checked_sub(len) else {
                    return Outcome::Abort;
                };
                // `yyval = yyvsp[1-yylen]`: `$$ = $1` unless the action says
                // otherwise — and garbage for an empty rule, which none reads.
                let default = if len > 0 {
                    vs.get(base).copied().unwrap_or(Value::None)
                } else {
                    Value::None
                };
                let rhs = vs.get(base..).unwrap_or(&[]);
                let Ok(yyval) = reduce(pc, rule, rhs, default) else {
                    return Outcome::Abort;
                };
                // YYPOPSTACK (yylen), then `*++yyvsp = yyval`.
                vs.truncate(base);
                vs.push(yyval);
                let Some(keep) = ss.len().checked_sub(len) else {
                    return Outcome::Abort;
                };
                ss.truncate(keep);
                // The goto: the state the reduced nonterminal leads to from
                // the state now on top. The loop head pushes it (yynewstate).
                let Some(&top) = ss.last() else {
                    return Outcome::Abort;
                };
                let Some(lhs) = YYR1
                    .get(rule)
                    .and_then(|&n| usize::try_from(n).ok())
                    .and_then(|n| n.checked_sub(YYNTOKENS))
                else {
                    return Outcome::Abort;
                };
                let top_i32 = i32::try_from(top).unwrap_or(i32::MAX);
                let target = YYPGOTO
                    .get(lhs)
                    .and_then(|&goto| checked_entry(i32::from(goto), top_i32))
                    .or_else(|| YYDEFGOTO.get(lhs).copied());
                let Some(target) = target.and_then(|n| usize::try_from(n).ok()) else {
                    return Outcome::Abort;
                };
                yystate = target;
            }
            Action::Error => {
                // yyerrlab. `yyerror` is a no-op upstream.
                if yyerrstatus == 3 {
                    // Reusing the lookahead after an error failed: discard it,
                    // or give up at end of input.
                    match lookahead {
                        Some((sym::YYEOF, _)) => return Outcome::Abort,
                        Some(_) => lookahead = None,
                        None => {}
                    }
                }
                // yyerrlab1: pop until a state shifts the error token.
                yyerrstatus = 3;
                let shifted = loop {
                    let entry = YYPACT
                        .get(yystate)
                        .copied()
                        .filter(|&pact| !pact_is_default(pact))
                        .and_then(|pact| checked_entry(i32::from(pact), i32::from(sym::YYERROR)))
                        .filter(|&n| n > 0);
                    if let Some(n) = entry {
                        break usize::from(n.unsigned_abs());
                    }
                    if ss.len() <= 1 {
                        return Outcome::Abort;
                    }
                    ss.pop();
                    vs.pop();
                    let Some(&top) = ss.last() else {
                        return Outcome::Abort;
                    };
                    yystate = top;
                };
                // Shift the error token, whose value is the lookahead's, on
                // top of the state that accepts it.
                vs.push(lookahead.map_or(Value::None, |(_, v)| v));
                yystate = shifted;
            }
        }
    }
}

/// `yydefault`: the state's default reduction, or an error.
fn default_action(state: usize) -> Action {
    match YYDEFACT.get(state).copied() {
        Some(n) if n > 0 => Action::Reduce(usize::from(n.unsigned_abs())),
        _ => Action::Error,
    }
}

/// A fresh `relative_time`, with one field set.
fn rel_with(set: impl FnOnce(&mut RelativeTime)) -> Value {
    let mut rel = RelativeTime::default();
    set(&mut rel);
    Value::Rel(rel)
}

/// `timespec` from a whole number of seconds. Upstream checks
/// `time_overflow` first, which no `intmax_t` fails when `time_t` is 64 bits.
fn whole_seconds(value: i64) -> Value {
    Value::Timespec(Timespec {
        tv_sec: value,
        tv_nsec: 0,
    })
}

/// `if (! OK) YYABORT;`
fn check(ok: bool) -> Result<(), Abort> {
    if ok { Ok(()) } else { Err(Abort) }
}

/// `$n`: the value of the n-th symbol of the rule being reduced.
fn arg(rhs: &[Value], n: usize) -> Result<Value, Abort> {
    n.checked_sub(1)
        .and_then(|i| rhs.get(i))
        .copied()
        .ok_or(Abort)
}

/// `pc->time_zone = V`: an `intmax_t` narrowed into `int`, as upstream's
/// assignment does. Every value that reaches here comes from a table, so none
/// is out of range.
fn narrow(value: i64) -> i32 {
    value as i32
}

/// The semantic action of `rule`, with `rhs` its symbols' values; returns
/// `$$`, which starts as `default` (`$1`).
///
/// The rule numbers and the code are upstream's `switch (yyn)` in
/// `parse-datetime.c`, case for case.
#[allow(clippy::too_many_lines, reason = "one arm per grammar rule")]
fn reduce(
    pc: &mut ParserControl<'_, '_>,
    rule: usize,
    rhs: &[Value],
    default: Value,
) -> Result<Value, Abort> {
    let mut yyval = default;
    match rule {
        // timespec: '@' seconds
        4 => {
            pc.seconds = arg(rhs, 2)?.timespec()?;
            pc.timespec_seen = true;
            pc.debug_print_current_time("number of seconds");
        }
        // item: datetime
        7 => {
            pc.times_seen = pc.times_seen.saturating_add(1);
            pc.dates_seen = pc.dates_seen.saturating_add(1);
            pc.debug_print_current_time("datetime");
        }
        // item: time
        8 => {
            pc.times_seen = pc.times_seen.saturating_add(1);
            pc.debug_print_current_time("time");
        }
        // item: local_zone
        9 => {
            pc.local_zones_seen = pc.local_zones_seen.saturating_add(1);
            pc.debug_print_current_time("local_zone");
        }
        // item: 'J'
        10 => {
            pc.j_zones_seen = pc.j_zones_seen.saturating_add(1);
            pc.debug_print_current_time("J");
        }
        // item: zone
        11 => {
            pc.zones_seen = pc.zones_seen.saturating_add(1);
            pc.debug_print_current_time("zone");
        }
        // item: date
        12 => {
            pc.dates_seen = pc.dates_seen.saturating_add(1);
            pc.debug_print_current_time("date");
        }
        // item: day
        13 => {
            pc.days_seen = pc.days_seen.saturating_add(1);
            pc.debug_print_current_time("day");
        }
        // item: rel
        14 => pc.debug_print_relative_time("relative"),
        // item: number
        15 => pc.debug_print_current_time("number"),
        // item: hybrid
        16 => pc.debug_print_relative_time("hybrid"),
        // time: tUNUMBER tMERIDIAN
        19 => {
            set_hhmmss(pc, arg(rhs, 1)?.textint()?.value, 0, 0, 0);
            pc.meridian = arg(rhs, 2)?.int()?;
        }
        // time: tUNUMBER ':' tUNUMBER tMERIDIAN
        20 => {
            let h = arg(rhs, 1)?.textint()?.value;
            let m = arg(rhs, 3)?.textint()?.value;
            set_hhmmss(pc, h, m, 0, 0);
            pc.meridian = arg(rhs, 4)?.int()?;
        }
        // time: tUNUMBER ':' tUNUMBER ':' unsigned_seconds tMERIDIAN
        21 => {
            let h = arg(rhs, 1)?.textint()?.value;
            let m = arg(rhs, 3)?.textint()?.value;
            let s = arg(rhs, 5)?.timespec()?;
            set_hhmmss(pc, h, m, s.tv_sec, s.tv_nsec);
            pc.meridian = arg(rhs, 6)?.int()?;
        }
        // iso_8601_time: tUNUMBER zone_offset
        23 => {
            set_hhmmss(pc, arg(rhs, 1)?.textint()?.value, 0, 0, 0);
            pc.meridian = MER_24;
        }
        // iso_8601_time: tUNUMBER ':' tUNUMBER o_zone_offset
        24 => {
            let h = arg(rhs, 1)?.textint()?.value;
            let m = arg(rhs, 3)?.textint()?.value;
            set_hhmmss(pc, h, m, 0, 0);
            pc.meridian = MER_24;
        }
        // iso_8601_time: tUNUMBER ':' tUNUMBER ':' unsigned_seconds o_zone_offset
        25 => {
            let h = arg(rhs, 1)?.textint()?.value;
            let m = arg(rhs, 3)?.textint()?.value;
            let s = arg(rhs, 5)?.timespec()?;
            set_hhmmss(pc, h, m, s.tv_sec, s.tv_nsec);
            pc.meridian = MER_24;
        }
        // zone_offset: tSNUMBER o_colon_minutes
        28 => {
            pc.zones_seen = pc.zones_seen.saturating_add(1);
            check(time_zone_hhmm(
                pc,
                arg(rhs, 1)?.textint()?,
                arg(rhs, 2)?.int()?,
            ))?;
        }
        // local_zone: tLOCAL_ZONE
        29 => pc.local_isdst = narrow(arg(rhs, 1)?.int()?),
        // local_zone: tLOCAL_ZONE tDST
        30 => {
            pc.local_isdst = 1;
            pc.dsts_seen = pc.dsts_seen.saturating_add(1);
        }
        // zone: tZONE
        31 => pc.time_zone = narrow(arg(rhs, 1)?.int()?),
        // zone: 'T'
        32 => pc.time_zone = narrow(-HOUR * 7),
        // zone: tZONE relunit_snumber
        33 => {
            pc.time_zone = narrow(arg(rhs, 1)?.int()?);
            check(apply_relative_time(pc, arg(rhs, 2)?.rel()?, 1))?;
            pc.debug_print_relative_time("relative");
        }
        // zone: 'T' relunit_snumber
        34 => {
            pc.time_zone = narrow(-HOUR * 7);
            check(apply_relative_time(pc, arg(rhs, 2)?.rel()?, 1))?;
            pc.debug_print_relative_time("relative");
        }
        // zone: tZONE tSNUMBER o_colon_minutes
        35 => {
            check(time_zone_hhmm(
                pc,
                arg(rhs, 2)?.textint()?,
                arg(rhs, 3)?.int()?,
            ))?;
            // `ckd_add` into an `int`: the sum is taken exactly, then must fit.
            let sum = i64::from(pc.time_zone)
                .checked_add(arg(rhs, 1)?.int()?)
                .ok_or(Abort)?;
            pc.time_zone = i32::try_from(sum).map_err(|_| Abort)?;
        }
        // zone: tDAYZONE
        36 => pc.time_zone = narrow(arg(rhs, 1)?.int()?.saturating_add(HOUR)),
        // zone: tZONE tDST
        37 => pc.time_zone = narrow(arg(rhs, 1)?.int()?.saturating_add(HOUR)),
        // day: tDAY
        38 => {
            pc.day_ordinal = 0;
            pc.day_number = narrow(arg(rhs, 1)?.int()?);
        }
        // day: tDAY ','
        39 => {
            pc.day_ordinal = 0;
            pc.day_number = narrow(arg(rhs, 1)?.int()?);
        }
        // day: tORDINAL tDAY
        40 => {
            pc.day_ordinal = arg(rhs, 1)?.int()?;
            pc.day_number = narrow(arg(rhs, 2)?.int()?);
            pc.debug_ordinal_day_seen = true;
        }
        // day: tUNUMBER tDAY
        41 => {
            pc.day_ordinal = arg(rhs, 1)?.textint()?.value;
            pc.day_number = narrow(arg(rhs, 2)?.int()?);
            pc.debug_ordinal_day_seen = true;
        }
        // date: tUNUMBER '/' tUNUMBER
        42 => {
            pc.month = arg(rhs, 1)?.textint()?.value;
            pc.day = arg(rhs, 3)?.textint()?.value;
        }
        // date: tUNUMBER '/' tUNUMBER '/' tUNUMBER
        43 => {
            // YYYY/MM/DD if the first number has four or more digits,
            // otherwise MM/DD/YY. The former exists for machine-generated
            // dates such as an RCS log's.
            let first = arg(rhs, 1)?.textint()?;
            let second = arg(rhs, 3)?.textint()?;
            let third = arg(rhs, 5)?.textint()?;
            if 4 <= first.digits {
                if pc.debugging() {
                    pc.dbg_printf(
                        format!(
                            "warning: value {} has {} digits. Assuming YYYY/MM/DD\n",
                            first.value, first.digits
                        )
                        .as_bytes(),
                    );
                }
                pc.year = first;
                pc.month = second.value;
                pc.day = third.value;
            } else {
                if pc.debugging() {
                    pc.dbg_printf(
                        format!(
                            "warning: value {} has less than 4 digits. Assuming MM/DD/YY[YY]\n",
                            first.value
                        )
                        .as_bytes(),
                    );
                }
                pc.month = first.value;
                pc.day = second.value;
                pc.year = third;
            }
        }
        // date: tUNUMBER tMONTH tSNUMBER  (17-JUN-1992)
        44 => {
            pc.day = arg(rhs, 1)?.textint()?.value;
            pc.month = arg(rhs, 2)?.int()?;
            let year = arg(rhs, 3)?.textint()?;
            pc.year.value = 0i64.checked_sub(year.value).ok_or(Abort)?;
            pc.year.digits = year.digits;
        }
        // date: tMONTH tSNUMBER tSNUMBER  (JUN-17-1992)
        45 => {
            pc.month = arg(rhs, 1)?.int()?;
            pc.day = 0i64
                .checked_sub(arg(rhs, 2)?.textint()?.value)
                .ok_or(Abort)?;
            let year = arg(rhs, 3)?.textint()?;
            pc.year.value = 0i64.checked_sub(year.value).ok_or(Abort)?;
            pc.year.digits = year.digits;
        }
        // date: tMONTH tUNUMBER
        46 => {
            pc.month = arg(rhs, 1)?.int()?;
            pc.day = arg(rhs, 2)?.textint()?.value;
        }
        // date: tMONTH tUNUMBER ',' tUNUMBER
        47 => {
            pc.month = arg(rhs, 1)?.int()?;
            pc.day = arg(rhs, 2)?.textint()?.value;
            pc.year = arg(rhs, 4)?.textint()?;
        }
        // date: tUNUMBER tMONTH
        48 => {
            pc.day = arg(rhs, 1)?.textint()?.value;
            pc.month = arg(rhs, 2)?.int()?;
        }
        // date: tUNUMBER tMONTH tUNUMBER
        49 => {
            pc.day = arg(rhs, 1)?.textint()?.value;
            pc.month = arg(rhs, 2)?.int()?;
            pc.year = arg(rhs, 3)?.textint()?;
        }
        // iso_8601_date: tUNUMBER tSNUMBER tSNUMBER  (YYYY-MM-DD)
        51 => {
            pc.year = arg(rhs, 1)?.textint()?;
            pc.month = 0i64
                .checked_sub(arg(rhs, 2)?.textint()?.value)
                .ok_or(Abort)?;
            pc.day = 0i64
                .checked_sub(arg(rhs, 3)?.textint()?.value)
                .ok_or(Abort)?;
        }
        // rel: relunit tAGO
        52 => check(apply_relative_time(
            pc,
            arg(rhs, 1)?.rel()?,
            arg(rhs, 2)?.int()?,
        ))?,
        // rel: relunit | rel: dayshift
        53 | 54 => check(apply_relative_time(pc, arg(rhs, 1)?.rel()?, 1))?,
        // relunit: tORDINAL tYEAR_UNIT
        55 => {
            let n = arg(rhs, 1)?.int()?;
            yyval = rel_with(|r| r.year = n);
        }
        // relunit: tUNUMBER tYEAR_UNIT
        56 => {
            let n = arg(rhs, 1)?.textint()?.value;
            yyval = rel_with(|r| r.year = n);
        }
        // relunit: tYEAR_UNIT
        57 => yyval = rel_with(|r| r.year = 1),
        // relunit: tORDINAL tMONTH_UNIT
        58 => {
            let n = arg(rhs, 1)?.int()?;
            yyval = rel_with(|r| r.month = n);
        }
        // relunit: tUNUMBER tMONTH_UNIT
        59 => {
            let n = arg(rhs, 1)?.textint()?.value;
            yyval = rel_with(|r| r.month = n);
        }
        // relunit: tMONTH_UNIT
        60 => yyval = rel_with(|r| r.month = 1),
        // relunit: tORDINAL tDAY_UNIT
        61 => {
            let n = arg(rhs, 1)?.int()?;
            let days = n.checked_mul(arg(rhs, 2)?.int()?).ok_or(Abort)?;
            yyval = rel_with(|r| r.day = days);
        }
        // relunit: tUNUMBER tDAY_UNIT
        62 => {
            let n = arg(rhs, 1)?.textint()?.value;
            let days = n.checked_mul(arg(rhs, 2)?.int()?).ok_or(Abort)?;
            yyval = rel_with(|r| r.day = days);
        }
        // relunit: tDAY_UNIT
        63 => {
            let n = arg(rhs, 1)?.int()?;
            yyval = rel_with(|r| r.day = n);
        }
        // relunit: tORDINAL tHOUR_UNIT
        64 => {
            let n = arg(rhs, 1)?.int()?;
            yyval = rel_with(|r| r.hour = n);
        }
        // relunit: tUNUMBER tHOUR_UNIT
        65 => {
            let n = arg(rhs, 1)?.textint()?.value;
            yyval = rel_with(|r| r.hour = n);
        }
        // relunit: tHOUR_UNIT
        66 => yyval = rel_with(|r| r.hour = 1),
        // relunit: tORDINAL tMINUTE_UNIT
        67 => {
            let n = arg(rhs, 1)?.int()?;
            yyval = rel_with(|r| r.minutes = n);
        }
        // relunit: tUNUMBER tMINUTE_UNIT
        68 => {
            let n = arg(rhs, 1)?.textint()?.value;
            yyval = rel_with(|r| r.minutes = n);
        }
        // relunit: tMINUTE_UNIT
        69 => yyval = rel_with(|r| r.minutes = 1),
        // relunit: tORDINAL tSEC_UNIT
        70 => {
            let n = arg(rhs, 1)?.int()?;
            yyval = rel_with(|r| r.seconds = n);
        }
        // relunit: tUNUMBER tSEC_UNIT
        71 => {
            let n = arg(rhs, 1)?.textint()?.value;
            yyval = rel_with(|r| r.seconds = n);
        }
        // relunit: tSDECIMAL_NUMBER tSEC_UNIT | tUDECIMAL_NUMBER tSEC_UNIT
        72 | 73 => {
            let t = arg(rhs, 1)?.timespec()?;
            yyval = rel_with(|r| {
                r.seconds = t.tv_sec;
                r.ns = t.tv_nsec;
            });
        }
        // relunit: tSEC_UNIT
        74 => yyval = rel_with(|r| r.seconds = 1),
        // relunit_snumber: tSNUMBER tYEAR_UNIT
        76 => {
            let n = arg(rhs, 1)?.textint()?.value;
            yyval = rel_with(|r| r.year = n);
        }
        // relunit_snumber: tSNUMBER tMONTH_UNIT
        77 => {
            let n = arg(rhs, 1)?.textint()?.value;
            yyval = rel_with(|r| r.month = n);
        }
        // relunit_snumber: tSNUMBER tDAY_UNIT
        78 => {
            let n = arg(rhs, 1)?.textint()?.value;
            let days = n.checked_mul(arg(rhs, 2)?.int()?).ok_or(Abort)?;
            yyval = rel_with(|r| r.day = days);
        }
        // relunit_snumber: tSNUMBER tHOUR_UNIT
        79 => {
            let n = arg(rhs, 1)?.textint()?.value;
            yyval = rel_with(|r| r.hour = n);
        }
        // relunit_snumber: tSNUMBER tMINUTE_UNIT
        80 => {
            let n = arg(rhs, 1)?.textint()?.value;
            yyval = rel_with(|r| r.minutes = n);
        }
        // relunit_snumber: tSNUMBER tSEC_UNIT
        81 => {
            let n = arg(rhs, 1)?.textint()?.value;
            yyval = rel_with(|r| r.seconds = n);
        }
        // dayshift: tDAY_SHIFT
        82 => {
            let n = arg(rhs, 1)?.int()?;
            yyval = rel_with(|r| r.day = n);
        }
        // signed_seconds: tSNUMBER | unsigned_seconds: tUNUMBER
        86 | 88 => yyval = whole_seconds(arg(rhs, 1)?.textint()?.value),
        // number: tUNUMBER
        89 => digits_to_date_time(pc, arg(rhs, 1)?.textint()?),
        // hybrid: tUNUMBER relunit_snumber
        90 => {
            // All digits and then a relative offset, so that "YYYYMMDD +N
            // days" is accepted as well as "YYYYMMDD N days".
            digits_to_date_time(pc, arg(rhs, 1)?.textint()?);
            check(apply_relative_time(pc, arg(rhs, 2)?.rel()?, 1))?;
        }
        // o_colon_minutes: %empty
        91 => yyval = Value::Int(-1),
        // o_colon_minutes: ':' tUNUMBER
        92 => yyval = Value::Int(arg(rhs, 2)?.textint()?.value),
        // Every other rule has no action: `$$ = $1`.
        _ => {}
    }
    Ok(yyval)
}
