//! The parser: Bison 3.0.2's `yacc.c` skeleton, driven by the tables of
//! upstream's generated `bc.c` ([`crate::tables`]), with `bc.y`'s actions,
//! numbered as `bc.c` numbers them.
//!
//! Every action reads and writes the value stack as upstream's do, the
//! mid-rule ones included (a `For` rule keeps its saved labels in the slots
//! of its own tokens). The skeleton's details are kept too: a lookahead
//! token is read only when the state needs one, a value of an empty rule is
//! whatever lies above the stack's top (nothing here reads one), and error
//! recovery pops states until one shifts `error`, then discards tokens
//! until three have been shifted.

use crate::tables::{
    YYCHECK, YYDEFACT, YYDEFGOTO, YYFINAL, YYLAST, YYMAXUTOK, YYNTOKENS, YYPACT, YYPACT_NINF,
    YYPGOTO, YYR1, YYR2, YYTABLE, YYTRANSLATE, YYUNDEFTOK,
};
use crate::{Arg, Bc, code::Kind};

/// The token numbers of `bc.y`.
pub mod token {
    #![allow(missing_docs, reason = "bc.y's token names")]
    pub const ENDOFLINE: i32 = 258;
    pub const AND: i32 = 259;
    pub const OR: i32 = 260;
    pub const NOT: i32 = 261;
    pub const STRING: i32 = 262;
    pub const NAME: i32 = 263;
    pub const NUMBER: i32 = 264;
    pub const ASSIGN_OP: i32 = 265;
    pub const REL_OP: i32 = 266;
    pub const INCR_DECR: i32 = 267;
    pub const DEFINE: i32 = 268;
    pub const BREAK: i32 = 269;
    pub const QUIT: i32 = 270;
    pub const LENGTH: i32 = 271;
    pub const RETURN: i32 = 272;
    pub const FOR: i32 = 273;
    pub const IF: i32 = 274;
    pub const WHILE: i32 = 275;
    pub const SQRT: i32 = 276;
    pub const ELSE: i32 = 277;
    pub const SCALE: i32 = 278;
    pub const IBASE: i32 = 279;
    pub const OBASE: i32 = 280;
    pub const AUTO: i32 = 281;
    pub const READ: i32 = 282;
    pub const RANDOM: i32 = 283;
    pub const WARRANTY: i32 = 284;
    pub const HALT: i32 = 285;
    pub const LAST: i32 = 286;
    pub const CONTINUE: i32 = 287;
    pub const PRINT: i32 = 288;
    pub const LIMITS: i32 = 289;
    pub const HISTORY_VAR: i32 = 291;
    pub const VOID: i32 = 292;
}

/// A semantic value: `%union`'s members, the one last stored.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Sem {
    /// Nothing stored yet.
    #[default]
    None,
    /// `i_value`.
    Int(i32),
    /// `c_value`.
    Char(u8),
    /// `s_value`.
    Str(Vec<u8>),
    /// `a_value`: an argument list, head first; empty for `NULL`.
    Args(Vec<Arg>),
}

impl Sem {
    /// `i_value`.
    fn i(&self) -> i32 {
        match self {
            Self::Int(v) => *v,
            Self::Char(c) => i32::from(*c),
            _ => 0,
        }
    }

    /// `c_value`.
    fn c(&self) -> u8 {
        match self {
            Self::Char(c) => *c,
            Self::Int(v) => v.to_le_bytes()[0],
            _ => 0,
        }
    }

    /// `s_value`.
    fn s(&self) -> Vec<u8> {
        match self {
            Self::Str(s) => s.clone(),
            _ => Vec::new(),
        }
    }

    /// `a_value`.
    fn a(&self) -> Vec<Arg> {
        match self {
            Self::Args(a) => a.clone(),
            _ => Vec::new(),
        }
    }
}

/// `EX_*`: what an expression is.
const EX_ASSGN: i32 = 0;
const EX_REG: i32 = 1;
const EX_COMP: i32 = 2;
const EX_PAREN: i32 = 4;
const EX_VOID: i32 = 8;
const EX_EMPTY: i32 = 16;

/// `YYEMPTY`, `YYEOF`, `YYTERROR`, `YYMAXDEPTH`.
const YYEMPTY: i32 = -2;
const YYEOF: i32 = 0;
const YYTERROR: i32 = 1;
const YYMAXDEPTH: usize = 10_000;

/// A table entry, 0 out of range.
fn at(table: &[i16], i: i32) -> i32 {
    usize::try_from(i)
        .ok()
        .and_then(|i| table.get(i))
        .map_or(0, |&v| i32::from(v))
}

/// `YYTRANSLATE (c)`.
fn translate(c: i32) -> i32 {
    if (0..=YYMAXUTOK).contains(&c) {
        at(&YYTRANSLATE, c)
    } else {
        YYUNDEFTOK
    }
}

/// Where the skeleton goes next.
enum Go {
    NewState,
    Default,
    Reduce(i32),
    ErrLab,
    ErrLab1,
}

/// The value stack, addressed from its top as `yyvsp[k]` is.
struct Values(Vec<Sem>);

impl Values {
    /// `yyvsp[k]`, `k <= 0`.
    fn get(&self, k: i32) -> Sem {
        self.index(k)
            .and_then(|i| self.0.get(i))
            .cloned()
            .unwrap_or_default()
    }

    /// `yyvsp[k] = v`.
    fn set(&mut self, k: i32, v: Sem) {
        if let Some(slot) = self.index(k).and_then(|i| self.0.get_mut(i)) {
            *slot = v;
        }
    }

    fn index(&self, k: i32) -> Option<usize> {
        let top = self.0.len().checked_sub(1)?;
        let back = usize::try_from(k.checked_neg()?).ok()?;
        top.checked_sub(back)
    }
}

impl Bc {
    /// `yyparse ()`: 0 accepted, 1 aborted, 2 out of stack.
    #[allow(clippy::too_many_lines, reason = "the skeleton's one loop")]
    pub fn yyparse(&mut self) -> i32 {
        let mut ss: Vec<i32> = Vec::new();
        // `yyvsa[0]`, never read: the value stack is one ahead of nothing.
        let mut vs = Values(vec![Sem::None]);
        let mut yystate: i32 = 0;
        let mut yyerrstatus: i32 = 0;
        let mut yychar: i32 = YYEMPTY;
        let mut go = Go::NewState;
        loop {
            match go {
                Go::NewState => {
                    ss.push(yystate);
                    if ss.len() >= YYMAXDEPTH {
                        self.yyerror(b"memory exhausted");
                        return 2;
                    }
                    if yystate == YYFINAL {
                        return 0;
                    }
                    let mut yyn = at(&YYPACT, yystate);
                    if yyn == YYPACT_NINF {
                        go = Go::Default;
                        continue;
                    }
                    if yychar == YYEMPTY {
                        yychar = self.yylex();
                    }
                    let yytoken = if yychar <= YYEOF {
                        yychar = YYEOF;
                        YYEOF
                    } else {
                        translate(yychar)
                    };
                    yyn = yyn.saturating_add(yytoken);
                    if yyn < 0 || YYLAST < yyn || at(&YYCHECK, yyn) != yytoken {
                        go = Go::Default;
                        continue;
                    }
                    yyn = at(&YYTABLE, yyn);
                    if yyn <= 0 {
                        // `yytable_value_is_error` is 0 in bc.c: this
                        // grammar's table holds no error entries.
                        go = Go::Reduce(yyn.saturating_neg());
                        continue;
                    }
                    yyerrstatus = yyerrstatus.saturating_sub(1).max(0);
                    yychar = YYEMPTY;
                    yystate = yyn;
                    vs.0.push(self.scan.yylval.clone());
                    go = Go::NewState;
                }
                Go::Default => {
                    let yyn = at(&YYDEFACT, yystate);
                    go = if yyn == 0 {
                        Go::ErrLab
                    } else {
                        Go::Reduce(yyn)
                    };
                }
                Go::Reduce(rule) => {
                    let yylen = usize::try_from(at(&YYR2, rule)).unwrap_or(0);
                    // `yyval = yyvsp[1-yylen]`: $1, or for an empty rule
                    // what lies above the top, which no action here reads.
                    let mut yyval = if yylen > 0 {
                        vs.get(1i32.saturating_sub(i32::try_from(yylen).unwrap_or(0)))
                    } else {
                        Sem::None
                    };
                    self.action(rule, &mut vs, &mut yyval, &mut yyerrstatus);
                    for _ in 0..yylen {
                        ss.pop();
                        vs.0.pop();
                    }
                    vs.0.push(yyval);
                    let lhs = at(&YYR1, rule);
                    let top = ss.last().copied().unwrap_or(0);
                    let st = at(&YYPGOTO, lhs.saturating_sub(YYNTOKENS)).saturating_add(top);
                    yystate = if (0..=YYLAST).contains(&st) && at(&YYCHECK, st) == top {
                        at(&YYTABLE, st)
                    } else {
                        at(&YYDEFGOTO, lhs.saturating_sub(YYNTOKENS))
                    };
                    go = Go::NewState;
                }
                Go::ErrLab => {
                    if yyerrstatus == 0 {
                        self.yyerror(b"syntax error");
                    }
                    if yyerrstatus == 3 {
                        if yychar <= YYEOF {
                            if yychar == YYEOF {
                                return 1;
                            }
                        } else {
                            yychar = YYEMPTY;
                        }
                    }
                    go = Go::ErrLab1;
                }
                Go::ErrLab1 => {
                    yyerrstatus = 3;
                    let shift_to = loop {
                        let yyn = at(&YYPACT, yystate);
                        if yyn != YYPACT_NINF {
                            let n = yyn.saturating_add(YYTERROR);
                            if (0..=YYLAST).contains(&n) && at(&YYCHECK, n) == YYTERROR {
                                let t = at(&YYTABLE, n);
                                if 0 < t {
                                    break t;
                                }
                            }
                        }
                        if ss.len() <= 1 {
                            return 1;
                        }
                        ss.pop();
                        vs.0.pop();
                        yystate = ss.last().copied().unwrap_or(0);
                    };
                    vs.0.push(self.scan.yylval.clone());
                    yystate = shift_to;
                    go = Go::NewState;
                }
            }
        }
    }

    /// `generate` of a formatted string.
    fn gen_fmt(&mut self, s: &str) {
        self.generate(s.as_bytes());
    }

    /// The reduction `rule`'s action: `bc.c`'s `case rule:`.
    #[allow(clippy::too_many_lines, reason = "bc.y's actions, case for case")]
    fn action(&mut self, rule: i32, v: &mut Values, yyval: &mut Sem, yyerrstatus: &mut i32) {
        match rule {
            2 => {
                *yyval = Sem::Int(0);
                if self.interactive && !self.quiet {
                    self.show_bc_version();
                    self.welcome();
                }
            }
            4 | 5 => self.run_code(),
            6 => {
                *yyerrstatus = 0;
                self.init_gen();
            }
            8 => self.ct_warn(b"newline not allowed"),
            9 | 13 | 40 | 52 => *yyval = Sem::Int(0),
            20 => *yyval = v.get(0),
            21 => self.warranty(),
            22 => self.limits(),
            23 => {
                let e = v.get(0).i();
                if e & EX_COMP != 0 {
                    self.ct_warn(b"comparison in expression");
                }
                self.generate(if e & EX_REG != 0 { b"W" } else { b"p" });
            }
            24 => {
                *yyval = Sem::Int(0);
                self.generate(b"w");
                self.generate(&v.get(0).s());
            }
            25 => {
                if self.break_label == 0 {
                    self.yyerror(b"Break outside a for/while");
                } else {
                    self.gen_fmt(&format!("J{}:", self.break_label));
                }
            }
            26 => {
                self.ct_warn(b"Continue statement");
                if self.continue_label == 0 {
                    self.yyerror(b"Continue outside a for");
                } else {
                    self.gen_fmt(&format!("J{}:", self.continue_label));
                }
            }
            27 => self.exit(0),
            28 => self.generate(b"h"),
            29 => self.generate(b"R"),
            30 => {
                v.set(0, Sem::Int(self.break_label));
                self.break_label = self.take_label();
            }
            31 => {
                let e = v.get(-1).i();
                if e & EX_COMP != 0 {
                    self.ct_warn(b"Comparison in first for expression");
                }
                if e & EX_VOID != 0 {
                    self.yyerror(b"first expression is void");
                }
                if e & EX_EMPTY == 0 {
                    self.generate(b"p");
                }
                let l = self.take_label();
                v.set(-1, Sem::Int(l));
                self.gen_fmt(&format!("N{l}:"));
            }
            32 => {
                let e = v.get(-1).i();
                if e & EX_VOID != 0 {
                    self.yyerror(b"second expression is void");
                }
                if e & EX_EMPTY != 0 {
                    self.generate(b"1");
                }
                let l = self.take_label();
                v.set(-1, Sem::Int(l));
                self.gen_fmt(&format!("B{l}:J{}:", self.break_label));
                *yyval = Sem::Int(self.continue_label);
                self.continue_label = self.take_label();
                self.gen_fmt(&format!("N{}:", self.continue_label));
            }
            33 => {
                let e = v.get(-1).i();
                if e & EX_COMP != 0 {
                    self.ct_warn(b"Comparison in third for expression");
                }
                if e & EX_VOID != 0 {
                    self.yyerror(b"third expression is void");
                }
                let (a, b) = (v.get(-7).i(), v.get(-4).i());
                if e & EX_EMPTY != 0 {
                    self.gen_fmt(&format!("J{a}:N{b}:"));
                } else {
                    self.gen_fmt(&format!("pJ{a}:N{b}:"));
                }
            }
            34 => {
                self.gen_fmt(&format!("J{}:N{}:", self.continue_label, self.break_label));
                self.break_label = v.get(-13).i();
                self.continue_label = v.get(-5).i();
            }
            35 => {
                if v.get(-1).i() & EX_VOID != 0 {
                    self.yyerror(b"void expression");
                }
                v.set(-1, Sem::Int(self.if_label));
                self.if_label = self.take_label();
                self.gen_fmt(&format!("Z{}:", self.if_label));
            }
            36 => {
                self.gen_fmt(&format!("N{}:", self.if_label));
                self.if_label = v.get(-5).i();
            }
            37 => {
                v.set(0, Sem::Int(self.continue_label));
                self.continue_label = self.take_label();
                self.gen_fmt(&format!("N{}:", self.continue_label));
            }
            38 => {
                if v.get(0).i() & EX_VOID != 0 {
                    self.yyerror(b"void expression");
                }
                v.set(0, Sem::Int(self.break_label));
                self.break_label = self.take_label();
                self.gen_fmt(&format!("Z{}:", self.break_label));
            }
            39 => {
                self.gen_fmt(&format!("J{}:N{}:", self.continue_label, self.break_label));
                self.break_label = v.get(-4).i();
                self.continue_label = v.get(-7).i();
            }
            41 => self.ct_warn(b"print statement"),
            45 => {
                self.generate(b"O");
                self.generate(&v.get(0).s());
            }
            46 => {
                if v.get(0).i() & EX_VOID != 0 {
                    self.yyerror(b"void expression in print");
                }
                self.generate(b"P");
            }
            48 => {
                self.ct_warn(b"else clause in if statement");
                let l = self.take_label();
                v.set(0, Sem::Int(l));
                self.gen_fmt(&format!("J{l}:N{}:", self.if_label));
                self.if_label = l;
            }
            50 => {
                let params = v.get(-5).a();
                let autos = v.get(0).a();
                self.check_params(&params, &autos);
                let p = arg_str(&params);
                let a = arg_str(&autos);
                self.cur_func = self.lookup(&v.get(-7).s(), Kind::FunctDef);
                self.gen_fmt(&format!("F{},{p}.{a}[", self.cur_func));
                let void = v.get(-8).i() != 0;
                if let Some(f) = usize::try_from(self.cur_func)
                    .ok()
                    .and_then(|i| self.functions.get_mut(i))
                {
                    f.void = void;
                }
                v.set(-9, Sem::Int(self.next_label));
                self.next_label = 1;
            }
            51 => {
                self.generate(b"0R]");
                self.next_label = v.get(-12).i();
                self.cur_func = -1;
            }
            53 => {
                *yyval = Sem::Int(1);
                self.ct_warn(b"void functions");
            }
            54 | 56 | 67 => *yyval = Sem::Args(Vec::new()),
            57 | 58 => *yyval = v.get(-1),
            59 => {
                let n = self.lookup(&v.get(0).s(), Kind::Simple);
                *yyval = Sem::Args(nextarg(Vec::new(), n, false));
            }
            60 => {
                let n = self.lookup(&v.get(-2).s(), Kind::Array);
                *yyval = Sem::Args(nextarg(Vec::new(), n, false));
            }
            61 | 62 => {
                let n = self.lookup(&v.get(-2).s(), Kind::Array);
                *yyval = Sem::Args(nextarg(Vec::new(), n, true));
                self.ct_warn(b"Call by variable arrays");
            }
            63 => {
                let n = self.lookup(&v.get(0).s(), Kind::Simple);
                *yyval = Sem::Args(nextarg(v.get(-2).a(), n, false));
            }
            64 => {
                let n = self.lookup(&v.get(-2).s(), Kind::Array);
                *yyval = Sem::Args(nextarg(v.get(-4).a(), n, false));
            }
            65 | 66 => {
                let n = self.lookup(&v.get(-2).s(), Kind::Array);
                *yyval = Sem::Args(nextarg(v.get(-5).a(), n, true));
                self.ct_warn(b"Call by variable arrays");
            }
            69 | 71 => {
                let e = v.get(0).i();
                if e & EX_COMP != 0 {
                    self.ct_warn(b"comparison in argument");
                }
                if e & EX_VOID != 0 {
                    self.yyerror(b"void argument");
                }
                let list = if rule == 69 {
                    Vec::new()
                } else {
                    v.get(-2).a()
                };
                *yyval = Sem::Args(nextarg(list, 0, false));
            }
            70 | 72 => {
                let n = self.lookup(&v.get(-2).s(), Kind::Array);
                self.gen_fmt(&format!("K{}:", n.saturating_neg()));
                let list = if rule == 70 {
                    Vec::new()
                } else {
                    v.get(-4).a()
                };
                *yyval = Sem::Args(nextarg(list, 1, false));
            }
            73 => {
                *yyval = Sem::Int(EX_EMPTY);
                self.ct_warn(b"Missing expression in for statement");
            }
            75 => {
                *yyval = Sem::Int(0);
                self.generate(b"0");
                if self.cur_func == -1 {
                    self.yyerror(b"Return outside of a function.");
                }
            }
            76 => {
                let e = v.get(0).i();
                if e & EX_COMP != 0 {
                    self.ct_warn(b"comparison in return expression");
                }
                if e & EX_PAREN == 0 {
                    self.ct_warn(b"return expression requires parenthesis");
                }
                if e & EX_VOID != 0 {
                    self.yyerror(b"return requires non-void expression");
                }
                if self.cur_func == -1 {
                    self.yyerror(b"Return outside of a function.");
                } else if self.current_function_is_void() {
                    self.yyerror(b"Return expression in a void function.");
                }
            }
            77 => {
                if v.get(0).c() != b'=' {
                    let n = v.get(-1).i();
                    if n < 0 {
                        self.gen_fmt(&format!("DL{}:", n.saturating_neg()));
                    } else {
                        self.gen_fmt(&format!("l{n}:"));
                    }
                }
            }
            78 => {
                let e = v.get(0).i();
                // Upstream warns "comparison in assignment" when `e &
                // EX_ASSGN` -- and EX_ASSGN is 0, so it never does.
                if e & EX_VOID != 0 {
                    self.yyerror(b"Assignment of a void expression");
                }
                let op = v.get(-2).c();
                if op != b'=' {
                    self.generate(&[op]);
                }
                let n = v.get(-3).i();
                if n < 0 {
                    self.gen_fmt(&format!("S{}:", n.saturating_neg()));
                } else {
                    self.gen_fmt(&format!("s{n}:"));
                }
                *yyval = Sem::Int(EX_ASSGN);
            }
            79 => {
                self.ct_warn(b"&& operator");
                let l = self.take_label();
                v.set(0, Sem::Int(l));
                self.gen_fmt(&format!("DZ{l}:p"));
            }
            80 => {
                let (a, b) = (v.get(-3).i(), v.get(0).i());
                if (a & EX_VOID != 0) || (b & EX_VOID != 0) {
                    self.yyerror(b"void expression with &&");
                }
                let l = v.get(-2).i();
                self.gen_fmt(&format!("DZ{l}:p1N{l}:"));
                *yyval = Sem::Int((a | b) & !EX_PAREN);
            }
            81 => {
                self.ct_warn(b"|| operator");
                let l = self.take_label();
                v.set(0, Sem::Int(l));
                self.gen_fmt(&format!("B{l}:"));
            }
            82 => {
                let (a, b) = (v.get(-3).i(), v.get(0).i());
                if (a & EX_VOID != 0) || (b & EX_VOID != 0) {
                    self.yyerror(b"void expression with ||");
                }
                let tmplab = self.take_label();
                let l = v.get(-2).i();
                self.gen_fmt(&format!("B{l}:0J{tmplab}:N{l}:1N{tmplab}:"));
                *yyval = Sem::Int((a | b) & !EX_PAREN);
            }
            83 => {
                let e = v.get(0).i();
                if e & EX_VOID != 0 {
                    self.yyerror(b"void expression with !");
                }
                *yyval = Sem::Int(e & !EX_PAREN);
                self.ct_warn(b"! operator");
                self.generate(b"!");
            }
            84 => {
                let (a, b) = (v.get(-2).i(), v.get(0).i());
                if (a & EX_VOID != 0) || (b & EX_VOID != 0) {
                    self.yyerror(b"void expression with comparison");
                }
                *yyval = Sem::Int(EX_REG | EX_COMP);
                let op = v.get(-1).s();
                let second_eq = op.get(1) == Some(&b'=');
                match op.first() {
                    Some(b'=') => self.generate(b"="),
                    Some(b'!') => self.generate(b"#"),
                    Some(b'<') => self.generate(if second_eq { b"{" } else { b"<" }),
                    Some(b'>') => self.generate(if second_eq { b"}" } else { b">" }),
                    _ => {}
                }
            }
            85..=90 => {
                let (a, b) = (v.get(-2).i(), v.get(0).i());
                let op = b"+-*/%^"
                    .get(usize::try_from(rule.saturating_sub(85)).unwrap_or(0))
                    .copied()
                    .unwrap_or(b'+');
                if (a & EX_VOID != 0) || (b & EX_VOID != 0) {
                    let mut msg = b"void expression with ".to_vec();
                    msg.push(op);
                    self.yyerror(&msg);
                }
                self.generate(&[op]);
                *yyval = Sem::Int((a | b) & !EX_PAREN);
            }
            91 => {
                let e = v.get(0).i();
                if e & EX_VOID != 0 {
                    self.yyerror(b"void expression with unary -");
                }
                self.generate(b"n");
                *yyval = Sem::Int(e & !EX_PAREN);
            }
            92 => {
                *yyval = Sem::Int(EX_REG);
                let n = v.get(0).i();
                if n < 0 {
                    self.gen_fmt(&format!("L{}:", n.saturating_neg()));
                } else {
                    self.gen_fmt(&format!("l{n}:"));
                }
            }
            93 => {
                let s = v.get(0).s();
                *yyval = Sem::Int(EX_REG);
                if s.as_slice() == b"0" {
                    self.generate(b"0");
                } else if s.as_slice() == b"1" {
                    self.generate(b"1");
                } else {
                    self.generate(b"K");
                    self.generate(&s);
                    self.generate(b":");
                }
            }
            94 => {
                let e = v.get(-1).i();
                if e & EX_VOID != 0 {
                    self.yyerror(b"void expression in parenthesis");
                }
                *yyval = Sem::Int(e | EX_REG | EX_PAREN);
            }
            95 => {
                let f = self.lookup(&v.get(-3).s(), Kind::Funct);
                let void = usize::try_from(f)
                    .ok()
                    .and_then(|i| self.functions.get(i))
                    .is_some_and(|func| func.void);
                *yyval = Sem::Int(if void { EX_VOID } else { EX_REG });
                let args = v.get(-1).a();
                if args.is_empty() {
                    self.gen_fmt(&format!("C{f}:"));
                } else {
                    let kinds: String = args
                        .iter()
                        .map(|a| if a.name != 0 { '1' } else { '0' })
                        .collect();
                    self.gen_fmt(&format!("C{f},{kinds}:"));
                }
            }
            96 => {
                *yyval = Sem::Int(EX_REG);
                let n = v.get(0).i();
                let plus = v.get(-1).c() == b'+';
                let text = if n < 0 {
                    let m = n.saturating_neg();
                    if plus {
                        format!("DA{m}:L{m}:")
                    } else {
                        format!("DM{m}:L{m}:")
                    }
                } else if plus {
                    format!("i{n}:l{n}:")
                } else {
                    format!("d{n}:l{n}:")
                };
                self.gen_fmt(&text);
            }
            97 => {
                *yyval = Sem::Int(EX_REG);
                let n = v.get(-1).i();
                let plus = v.get(0).c() == b'+';
                if n < 0 {
                    let m = n.saturating_neg();
                    self.gen_fmt(&format!("DL{m}:x"));
                    self.gen_fmt(&if plus {
                        format!("A{m}:")
                    } else {
                        format!("M{m}:")
                    });
                } else {
                    self.gen_fmt(&format!("l{n}:"));
                    self.gen_fmt(&if plus {
                        format!("i{n}:")
                    } else {
                        format!("d{n}:")
                    });
                }
            }
            98..=100 => {
                let (word, code): (&[u8], &[u8]) = match rule {
                    98 => (b"length", b"cL"),
                    99 => (b"sqrt", b"cR"),
                    _ => (b"scale", b"cS"),
                };
                if v.get(-1).i() & EX_VOID != 0 {
                    let mut msg = b"void expression in ".to_vec();
                    msg.extend_from_slice(word);
                    msg.extend_from_slice(b"()");
                    self.yyerror(&msg);
                }
                self.generate(code);
                *yyval = Sem::Int(EX_REG);
            }
            101 => {
                self.ct_warn(b"read function");
                self.generate(b"cI");
                *yyval = Sem::Int(EX_REG);
            }
            102 => {
                self.ct_warn(b"random function");
                self.generate(b"cX");
                *yyval = Sem::Int(EX_REG);
            }
            103 => *yyval = Sem::Int(self.lookup(&v.get(0).s(), Kind::Simple)),
            104 => {
                let e = v.get(-1).i();
                if e & EX_VOID != 0 {
                    self.yyerror(b"void expression as subscript");
                }
                if e & EX_COMP != 0 {
                    self.ct_warn(b"comparison in subscript");
                }
                *yyval = Sem::Int(self.lookup(&v.get(-3).s(), Kind::Array));
            }
            105 => *yyval = Sem::Int(0),
            106 => *yyval = Sem::Int(1),
            107 => *yyval = Sem::Int(2),
            108 => {
                *yyval = Sem::Int(3);
                self.ct_warn(b"History variable");
            }
            109 => {
                *yyval = Sem::Int(4);
                self.ct_warn(b"Last variable");
            }
            110 => self.ct_warn(b"End of line required"),
            112 => self.ct_warn(b"Too many end of lines"),
            _ => {}
        }
    }

    /// `next_label++`.
    fn take_label(&mut self) -> i32 {
        let l = self.next_label;
        self.next_label = self.next_label.saturating_add(1);
        l
    }

    /// `functions[cur_func].f_void`.
    fn current_function_is_void(&self) -> bool {
        usize::try_from(self.cur_func)
            .ok()
            .and_then(|i| self.functions.get(i))
            .is_some_and(|f| f.void)
    }
}

/// `nextarg (args, val, is_var)`: a node before the list.
fn nextarg(mut args: Vec<Arg>, name: i32, is_var: bool) -> Vec<Arg> {
    args.insert(0, Arg { name, is_var });
    args
}

/// `arg_str (args)`: the list as `F` records it -- in the order written
/// (the list's tail first), comma-separated, an array by reference
/// starred.
fn arg_str(args: &[Arg]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for a in args.iter().rev() {
        parts.push(if a.is_var {
            format!("*{}", a.name)
        } else {
            a.name.to_string()
        });
    }
    parts.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_argument_string_is_in_the_order_written() {
        let list = nextarg(nextarg(nextarg(Vec::new(), 5, false), -2, true), 6, false);
        assert_eq!(arg_str(&list), "5,*-2,6");
        assert_eq!(arg_str(&[]), "");
    }
}
