//! Slate OS Calculator
//!
//! Graphical scientific calculator with:
//! - Standard and Scientific modes (toggle between them)
//! - Proper operator precedence via recursive descent parser
//! - History of last 20 calculations
//! - Memory operations (M+, M-, MR, MC, MS)
//! - Degree/Radian toggle for trigonometric functions
//! - Keyboard shortcuts (numpad, Enter=equals, Escape=clear)
//! - Comprehensive error handling (division by zero, overflow, invalid input)
//!
//! The window is drawn as a [`Frame`] rather than assembled from widgets. A
//! keypad is nothing but hit boxes: every button here is identified by the
//! label it draws, which is also the string [`handle_button`] dispatches on, so
//! a key's picture, its clickable area and its meaning are one fact instead of
//! three that could drift apart.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::scroll_window;
use guitk::style::CornerRadii;
use guitk::text;
use guitk::wheel;
use oswindow::app::{self, App, Response};

use std::collections::VecDeque;
use std::f64::consts::{E, PI};
use std::process::ExitCode;

// ============================================================================
// Calculator modes
// ============================================================================

/// The calculator can operate in Standard or Scientific mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalcMode {
    Standard,
    Scientific,
}

/// Angle unit for trigonometric functions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AngleUnit {
    Degrees,
    Radians,
}

// ============================================================================
// Expression parser — token types
// ============================================================================

/// Tokens produced by the lexer for the expression parser.
#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f64),
    Plus,
    Minus,
    Multiply,
    Divide,
    Modulo,
    Power,
    LeftParen,
    RightParen,
    Func(MathFunc),
}

/// Built-in mathematical functions recognized by the parser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MathFunc {
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Ln,
    Log10,
    Sqrt,
    Abs,
    Floor,
    Ceil,
    Exp,
    Factorial,
}

// ============================================================================
// Lexer
// ============================================================================

/// Tokenize an expression string into a sequence of tokens.
///
/// Returns `None` if the input contains unrecognized characters or
/// malformed numbers.
fn tokenize(input: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    // A peekable scanner rather than an index into a `Vec<char>`. The two
    // multi-character tokens below -- numbers and identifiers -- are where an
    // index walk goes wrong: every branch has to remember to advance the cursor
    // by exactly the right amount, and one that advances twice silently eats a
    // character while one that forgets loops forever. `peek` lets each token
    // consume precisely what belongs to it and leave the rest where it is.
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.peek().copied() {
        // Skip whitespace.
        if ch.is_ascii_whitespace() {
            chars.next();
            continue;
        }

        // Numbers (including decimals). Built up character by character rather
        // than sliced out afterwards, so there are no offsets to get wrong.
        if ch.is_ascii_digit() || ch == '.' {
            let mut number = String::new();
            let mut has_dot = false;
            while let Some(c) = chars.peek().copied() {
                if c.is_ascii_digit() {
                    number.push(c);
                } else if c == '.' && !has_dot {
                    // A second dot ends the number rather than joining it:
                    // `1.2.3` is two numbers, not one unparseable one.
                    has_dot = true;
                    number.push(c);
                } else {
                    break;
                }
                chars.next();
            }
            // A bare `.` reaches here and fails to parse, which is the right
            // answer: it is not a number.
            tokens.push(Token::Number(number.parse::<f64>().ok()?));
            continue;
        }

        // Identifiers: function names and constants.
        if ch.is_ascii_alphabetic() {
            let mut word = String::new();
            while let Some(c) = chars.peek().copied() {
                if !c.is_ascii_alphabetic() {
                    break;
                }
                word.push(c);
                chars.next();
            }
            tokens.push(match word.as_str() {
                "sin" => Token::Func(MathFunc::Sin),
                "cos" => Token::Func(MathFunc::Cos),
                "tan" => Token::Func(MathFunc::Tan),
                "asin" => Token::Func(MathFunc::Asin),
                "acos" => Token::Func(MathFunc::Acos),
                "atan" => Token::Func(MathFunc::Atan),
                "ln" => Token::Func(MathFunc::Ln),
                "log" => Token::Func(MathFunc::Log10),
                "sqrt" => Token::Func(MathFunc::Sqrt),
                "abs" => Token::Func(MathFunc::Abs),
                "floor" => Token::Func(MathFunc::Floor),
                "ceil" => Token::Func(MathFunc::Ceil),
                "exp" => Token::Func(MathFunc::Exp),
                "fact" => Token::Func(MathFunc::Factorial),
                "pi" => Token::Number(PI),
                "e" => Token::Number(E),
                _ => return None, // Unknown identifier.
            });
            continue;
        }

        // Operators and parentheses: one character each.
        let token = match ch {
            '+' => Token::Plus,
            '-' => Token::Minus,
            '*' => Token::Multiply,
            '/' => Token::Divide,
            '%' => Token::Modulo,
            '^' => Token::Power,
            '(' => Token::LeftParen,
            ')' => Token::RightParen,
            _ => return None, // Unrecognized character.
        };
        chars.next();
        tokens.push(token);
    }

    Some(tokens)
}

// ============================================================================
// Recursive descent parser
// ============================================================================

/// Parser state: consumes tokens left-to-right and builds an evaluated result.
///
/// Grammar (by descending precedence):
///
/// ```text
/// expr     = term (('+' | '-') term)*
/// term     = power (('*' | '/' | '%') power)*
/// power    = unary ('^' power)?          // right-associative
/// unary    = ('-' | '+') unary | call
/// call     = FUNC '(' expr ')' | primary
/// primary  = NUMBER | '(' expr ')'
/// ```
struct Parser {
    /// The token stream, consumed left to right.
    ///
    /// A cursor rather than a `Vec` plus an index: a position that can be read
    /// without being advanced, or advanced twice, is the one bug a recursive
    /// descent parser cannot survive -- and with the index gone there is
    /// nowhere for it to happen.
    tokens: std::iter::Peekable<std::vec::IntoIter<Token>>,
    angle_unit: AngleUnit,
}

impl Parser {
    fn new(tokens: Vec<Token>, angle_unit: AngleUnit) -> Self {
        Self {
            tokens: tokens.into_iter().peekable(),
            angle_unit,
        }
    }

    /// Peek at the current token without consuming it.
    fn peek(&mut self) -> Option<&Token> {
        self.tokens.peek()
    }

    /// Consume the current token and advance.
    fn advance(&mut self) -> Option<Token> {
        self.tokens.next()
    }

    /// Evaluate the entire expression, returning the result or an error string.
    fn parse(&mut self) -> Result<f64, &'static str> {
        let result = self.expr()?;
        // Anything left over means the grammar stopped early -- `2 + 3 )` --
        // and reporting the value found so far would be answering a question
        // the user did not ask.
        if self.tokens.peek().is_some() {
            return Err("Unexpected token");
        }
        Ok(result)
    }

    /// expr = term (('+' | '-') term)*
    fn expr(&mut self) -> Result<f64, &'static str> {
        let mut left = self.term()?;

        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.advance();
                    let right = self.term()?;
                    left += right;
                }
                Some(Token::Minus) => {
                    self.advance();
                    let right = self.term()?;
                    left -= right;
                }
                _ => break,
            }
        }

        Ok(left)
    }

    /// term = power (('*' | '/' | '%') power)*
    fn term(&mut self) -> Result<f64, &'static str> {
        let mut left = self.power()?;

        loop {
            match self.peek() {
                Some(Token::Multiply) => {
                    self.advance();
                    let right = self.power()?;
                    left *= right;
                }
                Some(Token::Divide) => {
                    self.advance();
                    let right = self.power()?;
                    if right == 0.0 {
                        return Err("Division by zero");
                    }
                    left /= right;
                }
                Some(Token::Modulo) => {
                    self.advance();
                    let right = self.power()?;
                    if right == 0.0 {
                        return Err("Division by zero");
                    }
                    left %= right;
                }
                _ => break,
            }
        }

        Ok(left)
    }

    /// power = unary ('^' power)?   — right-associative
    fn power(&mut self) -> Result<f64, &'static str> {
        let base = self.unary()?;

        if matches!(self.peek(), Some(Token::Power)) {
            self.advance();
            let exponent = self.power()?; // Right-associative recursion.
            let result = base.powf(exponent);
            if result.is_infinite() || result.is_nan() {
                return Err("Overflow");
            }
            Ok(result)
        } else {
            Ok(base)
        }
    }

    /// unary = ('-' | '+') unary | call
    fn unary(&mut self) -> Result<f64, &'static str> {
        match self.peek() {
            Some(Token::Minus) => {
                // Distinguish unary minus from subtraction by checking if this is
                // the start of the expression or follows an operator/left-paren.
                self.advance();
                let val = self.unary()?;
                Ok(-val)
            }
            Some(Token::Plus) => {
                self.advance();
                self.unary()
            }
            _ => self.call(),
        }
    }

    /// call = FUNC '(' expr ')' | primary
    fn call(&mut self) -> Result<f64, &'static str> {
        if let Some(Token::Func(func)) = self.peek().cloned() {
            self.advance();
            // Expect '(' after function name.
            if !matches!(self.peek(), Some(Token::LeftParen)) {
                return Err("Expected '(' after function");
            }
            self.advance(); // consume '('
            let arg = self.expr()?;
            if !matches!(self.peek(), Some(Token::RightParen)) {
                return Err("Missing ')'");
            }
            self.advance(); // consume ')'
            self.apply_func(func, arg)
        } else {
            self.primary()
        }
    }

    /// primary = NUMBER | '(' expr ')'
    fn primary(&mut self) -> Result<f64, &'static str> {
        match self.peek().cloned() {
            Some(Token::Number(n)) => {
                self.advance();
                Ok(n)
            }
            Some(Token::LeftParen) => {
                self.advance(); // consume '('
                let val = self.expr()?;
                if !matches!(self.peek(), Some(Token::RightParen)) {
                    return Err("Missing ')'");
                }
                self.advance(); // consume ')'
                Ok(val)
            }
            Some(_) => Err("Unexpected token"),
            None => Err("Unexpected end of expression"),
        }
    }

    /// Apply a mathematical function to its argument.
    fn apply_func(&self, func: MathFunc, arg: f64) -> Result<f64, &'static str> {
        let result = match func {
            MathFunc::Sin => {
                let a = self.to_radians(arg);
                a.sin()
            }
            MathFunc::Cos => {
                let a = self.to_radians(arg);
                a.cos()
            }
            MathFunc::Tan => {
                let a = self.to_radians(arg);
                let cos_val = a.cos();
                if cos_val.abs() < 1e-15 {
                    return Err("Undefined (tan)");
                }
                a.tan()
            }
            MathFunc::Asin => {
                if !(-1.0..=1.0).contains(&arg) {
                    return Err("Domain error (asin)");
                }
                self.radians_to_user_unit(arg.asin())
            }
            MathFunc::Acos => {
                if !(-1.0..=1.0).contains(&arg) {
                    return Err("Domain error (acos)");
                }
                self.radians_to_user_unit(arg.acos())
            }
            MathFunc::Atan => self.radians_to_user_unit(arg.atan()),
            MathFunc::Ln => {
                if arg <= 0.0 {
                    return Err("Domain error (ln)");
                }
                arg.ln()
            }
            MathFunc::Log10 => {
                if arg <= 0.0 {
                    return Err("Domain error (log)");
                }
                arg.log10()
            }
            MathFunc::Sqrt => {
                if arg < 0.0 {
                    return Err("Domain error (sqrt)");
                }
                arg.sqrt()
            }
            MathFunc::Abs => arg.abs(),
            MathFunc::Floor => arg.floor(),
            MathFunc::Ceil => arg.ceil(),
            MathFunc::Exp => {
                let r = arg.exp();
                if r.is_infinite() {
                    return Err("Overflow");
                }
                r
            }
            MathFunc::Factorial => {
                if arg < 0.0 || arg.fract() != 0.0 {
                    return Err("Domain error (fact)");
                }
                let n = arg as u64;
                if n > 170 {
                    return Err("Overflow (fact)");
                }
                factorial(n)
            }
        };

        if result.is_nan() || result.is_infinite() {
            return Err("Math error");
        }
        Ok(result)
    }

    /// Convert an angle from the user's selected unit to radians.
    fn to_radians(&self, angle: f64) -> f64 {
        match self.angle_unit {
            AngleUnit::Radians => angle,
            AngleUnit::Degrees => angle.to_radians(),
        }
    }

    /// Convert an angle from radians to the user's selected unit.
    // Renamed from `from_radians` to satisfy `wrong_self_convention`
    // (from_* should not take `&self`).
    fn radians_to_user_unit(&self, radians: f64) -> f64 {
        match self.angle_unit {
            AngleUnit::Radians => radians,
            AngleUnit::Degrees => radians.to_degrees(),
        }
    }
}

/// Compute n! for non-negative integers.
fn factorial(n: u64) -> f64 {
    let mut result: f64 = 1.0;
    for i in 2..=n {
        result *= i as f64;
    }
    result
}

/// Public entry point: parse and evaluate an expression string.
///
/// Returns either the computed `f64` result or an error message.
fn evaluate(expression: &str, angle_unit: AngleUnit) -> Result<f64, &'static str> {
    let trimmed = expression.trim();
    if trimmed.is_empty() {
        return Err("Empty expression");
    }

    let tokens = tokenize(trimmed).ok_or("Invalid input")?;
    if tokens.is_empty() {
        return Err("Empty expression");
    }

    let mut parser = Parser::new(tokens, angle_unit);
    parser.parse()
}

// ============================================================================
// History entry
// ============================================================================

/// A record of one completed calculation.
#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub expression: String,
    pub result: String,
}

// ============================================================================
// Calculator state
// ============================================================================

/// Maximum number of history entries kept.
const MAX_HISTORY: usize = 20;

/// Complete calculator application state.
pub struct Calculator {
    /// Current expression being built.
    pub expression: String,
    /// Display text (result or error after pressing '=').
    pub display: String,
    /// Whether the display is showing a result (next digit input resets).
    pub showing_result: bool,
    /// Current mode: Standard or Scientific.
    pub mode: CalcMode,
    /// Angle unit for trig functions.
    pub angle_unit: AngleUnit,
    /// Memory register.
    pub memory: f64,
    /// Whether memory contains a stored value.
    pub memory_set: bool,
    /// Calculation history (newest first).
    pub history: VecDeque<HistoryEntry>,
    /// Whether the history panel is visible.
    pub show_history: bool,
    /// How many parentheses are open and not yet closed.
    ///
    /// Unsigned, and every change to it saturates. A depth is a count of things
    /// on a stack, and there is no such thing as minus one of them: the signed
    /// version let [`Calculator::input_backspace`] drive it negative by deleting
    /// a `(` that nothing had counted -- the one [`Calculator::input_negate`]
    /// writes -- after which `calculate` would stop auto-closing the
    /// parentheses that really were open.
    pub paren_depth: u32,
}

impl Default for Calculator {
    fn default() -> Self {
        Self::new()
    }
}

impl Calculator {
    /// Create a new calculator in Standard mode.
    pub fn new() -> Self {
        Self {
            expression: String::new(),
            display: String::from("0"),
            showing_result: false,
            mode: CalcMode::Standard,
            angle_unit: AngleUnit::Degrees,
            memory: 0.0,
            memory_set: false,
            history: VecDeque::new(),
            show_history: false,
            paren_depth: 0,
        }
    }

    // ======================================================================
    // Input actions
    // ======================================================================

    /// Append a digit character ('0'-'9') to the expression.
    pub fn input_digit(&mut self, digit: char) {
        if self.showing_result {
            self.expression.clear();
            self.display.clear();
            self.showing_result = false;
        }
        self.expression.push(digit);
        self.update_display();
    }

    /// Append a decimal point.
    pub fn input_decimal(&mut self) {
        if self.showing_result {
            self.expression.clear();
            self.expression.push('0');
            self.showing_result = false;
        }
        // Avoid double decimal in the current number token.
        let last_number = self.current_number_token();
        if !last_number.contains('.') {
            self.expression.push('.');
        }
        self.update_display();
    }

    /// Append an operator (+, -, *, /).
    pub fn input_operator(&mut self, op: char) {
        self.showing_result = false;
        // Replace trailing operator if the user changes their mind.
        let trimmed = self.expression.trim_end();
        if let Some(last) = trimmed.chars().last()
            && "+-*/%".contains(last)
            && op != '-'
        {
            // Replace the last operator (but allow unary minus after another op).
            // `saturating_sub` rather than `-`: the length of a string is
            // never less than the length of its own last character, but a
            // subtraction that says so out loud cannot underflow if that ever
            // stops being true.
            let end = trimmed.len().saturating_sub(last.len_utf8());
            self.expression.truncate(end);
        }
        self.expression.push(' ');
        self.expression.push(op);
        self.expression.push(' ');
        self.update_display();
    }

    /// Append a named function call (e.g., "sin(").
    pub fn input_function(&mut self, name: &str) {
        if self.showing_result {
            // Wrap the previous result so user can do sin(prev_result).
            let prev = self.display.clone();
            self.expression.clear();
            self.expression.push_str(name);
            self.expression.push('(');
            self.expression.push_str(&prev);
            self.showing_result = false;
        } else {
            self.expression.push_str(name);
            self.expression.push('(');
        }
        self.paren_depth = self.paren_depth.saturating_add(1);
        self.update_display();
    }

    /// Insert a constant value (pi or e).
    pub fn input_constant(&mut self, name: &str) {
        if self.showing_result {
            self.expression.clear();
            self.showing_result = false;
        }
        self.expression.push_str(name);
        self.update_display();
    }

    /// Open a parenthesis.
    pub fn input_open_paren(&mut self) {
        if self.showing_result {
            self.expression.clear();
            self.showing_result = false;
        }
        self.expression.push('(');
        self.paren_depth = self.paren_depth.saturating_add(1);
        self.update_display();
    }

    /// Close a parenthesis (only if one is open).
    pub fn input_close_paren(&mut self) {
        if self.paren_depth > 0 {
            self.expression.push(')');
            self.paren_depth = self.paren_depth.saturating_sub(1);
            self.update_display();
        }
    }

    /// Negate the current value (toggle sign).
    pub fn input_negate(&mut self) {
        if self.showing_result {
            // Negate the displayed result.
            if self.display.starts_with('-') {
                self.display.remove(0);
                self.expression = self.display.clone();
            } else if self.display != "0" {
                self.display.insert(0, '-');
                self.expression = self.display.clone();
            }
            self.showing_result = false;
        } else {
            // Wrap the current expression fragment in negation.
            // Simple approach: prepend "(-" and add a ")" later when evaluated.
            let current = self.expression.clone();
            self.expression.clear();
            self.expression.push_str("-(");
            self.expression.push_str(&current);
            self.expression.push(')');
        }
        self.update_display();
    }

    /// Compute a percentage of the accumulated value.
    pub fn input_percent(&mut self) {
        // Evaluate what we have so far and divide by 100.
        match evaluate(&self.expression, self.angle_unit) {
            Ok(val) => {
                let pct = val / 100.0;
                self.expression = format_result(pct);
                self.display = format_result(pct);
            }
            Err(_) => {
                self.display = String::from("Error");
            }
        }
        self.showing_result = true;
        self.update_display();
    }

    /// Delete the last character (backspace).
    pub fn input_backspace(&mut self) {
        if self.showing_result {
            return; // Backspace does nothing on a result.
        }
        if let Some(ch) = self.expression.pop() {
            if ch == '(' {
                // Saturating, not wrapping: the `(` may be one of the pair
                // `input_negate` writes, which was never counted.
                self.paren_depth = self.paren_depth.saturating_sub(1);
            } else if ch == ')' {
                self.paren_depth = self.paren_depth.saturating_add(1);
            }
            // Also trim trailing whitespace left by operator spacing.
            while self.expression.ends_with(' ') {
                self.expression.pop();
            }
        }
        self.update_display();
    }

    /// Clear the current entry (CE) without clearing history.
    pub fn clear_entry(&mut self) {
        self.expression.clear();
        self.display = String::from("0");
        self.showing_result = false;
        self.paren_depth = 0;
    }

    /// Clear everything (C).
    pub fn clear_all(&mut self) {
        self.clear_entry();
    }

    /// Evaluate the current expression and display the result.
    pub fn calculate(&mut self) {
        if self.expression.trim().is_empty() {
            return;
        }

        // Auto-close any open parentheses.
        while self.paren_depth > 0 {
            self.expression.push(')');
            self.paren_depth = self.paren_depth.saturating_sub(1);
        }

        let expr_display = self.expression.clone();
        match evaluate(&self.expression, self.angle_unit) {
            Ok(result) => {
                let formatted = format_result(result);
                self.display = formatted.clone();
                self.expression = formatted;
                self.showing_result = true;

                // Add to history.
                self.history.push_front(HistoryEntry {
                    expression: expr_display,
                    result: self.display.clone(),
                });
                if self.history.len() > MAX_HISTORY {
                    self.history.pop_back();
                }
            }
            Err(msg) => {
                self.display = format!("Error: {msg}");
                self.showing_result = true;
            }
        }
    }

    // ======================================================================
    // Memory operations
    // ======================================================================

    /// Store current value in memory (MS).
    pub fn memory_store(&mut self) {
        if let Ok(val) = evaluate(&self.expression, self.angle_unit) {
            self.memory = val;
            self.memory_set = true;
        }
    }

    /// Recall memory value (MR).
    pub fn memory_recall(&mut self) {
        if self.memory_set {
            let s = format_result(self.memory);
            if self.showing_result {
                self.expression.clear();
                self.showing_result = false;
            }
            self.expression.push_str(&s);
            self.update_display();
        }
    }

    /// Add current value to memory (M+).
    pub fn memory_add(&mut self) {
        if let Ok(val) = evaluate(&self.expression, self.angle_unit) {
            self.memory += val;
            self.memory_set = true;
        }
    }

    /// Subtract current value from memory (M-).
    pub fn memory_subtract(&mut self) {
        if let Ok(val) = evaluate(&self.expression, self.angle_unit) {
            self.memory -= val;
            self.memory_set = true;
        }
    }

    /// Clear memory (MC).
    pub fn memory_clear(&mut self) {
        self.memory = 0.0;
        self.memory_set = false;
    }

    // ======================================================================
    // Mode toggles
    // ======================================================================

    /// Toggle between Standard and Scientific modes.
    pub fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            CalcMode::Standard => CalcMode::Scientific,
            CalcMode::Scientific => CalcMode::Standard,
        };
    }

    /// Toggle between Degrees and Radians.
    pub fn toggle_angle_unit(&mut self) {
        self.angle_unit = match self.angle_unit {
            AngleUnit::Degrees => AngleUnit::Radians,
            AngleUnit::Radians => AngleUnit::Degrees,
        };
    }

    /// Toggle the history panel.
    pub fn toggle_history(&mut self) {
        self.show_history = !self.show_history;
    }

    /// Bring the result of history entry `index` back into the expression.
    ///
    /// The *result*, not the expression that produced it: the panel shows both,
    /// but what a user reaches back for is the number, to carry on calculating
    /// with it. Recalling the expression instead would re-open a calculation
    /// they had already finished, and the two are only interchangeable when the
    /// expression happens to be a bare literal.
    ///
    /// Modelled on [`Calculator::memory_recall`], down to clearing a displayed
    /// result first: appending to a result would silently concatenate digits
    /// onto the last answer.
    ///
    /// Returns `false` when there is no such entry, so a caller can tell a
    /// click that did nothing from one that changed the expression.
    pub fn recall_history(&mut self, index: usize) -> bool {
        let Some(entry) = self.history.get(index) else {
            return false;
        };
        let value = entry.result.clone();
        // An error is kept in history so the user can see what went wrong, but
        // it is not a number and pasting "Error: Division by zero" into the
        // expression would produce a second, more confusing error.
        if value.starts_with("Error:") {
            return false;
        }
        if self.showing_result {
            self.expression.clear();
            self.showing_result = false;
        }
        self.expression.push_str(&value);
        self.update_display();
        true
    }

    // ======================================================================
    // Helpers
    // ======================================================================

    /// Update the display text to reflect the current expression.
    fn update_display(&mut self) {
        if self.expression.is_empty() {
            self.display = String::from("0");
        } else {
            self.display = self.expression.clone();
        }
    }

    /// Extract the last number token being typed (for decimal-point checking).
    fn current_number_token(&self) -> String {
        let mut num = String::new();
        for ch in self.expression.chars().rev() {
            if ch.is_ascii_digit() || ch == '.' {
                num.push(ch);
            } else {
                break;
            }
        }
        num.chars().rev().collect()
    }
}

/// Format a floating-point result for display.
///
/// Uses up to 10 significant digits, strips trailing zeros, and handles
/// very large/small numbers with scientific notation.
fn format_result(value: f64) -> String {
    if value.is_nan() {
        return String::from("NaN");
    }
    if value.is_infinite() {
        return if value > 0.0 {
            String::from("Infinity")
        } else {
            String::from("-Infinity")
        };
    }

    // Check if the value is effectively an integer.
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return format!("{}", value as i64);
    }

    // General formatting: up to 10 significant digits.
    let s = format!("{:.10}", value);
    // Trim trailing zeros after the decimal point.
    let trimmed = s.trim_end_matches('0');
    let trimmed = trimmed.trim_end_matches('.');
    String::from(trimmed)
}

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const COLOR_BASE: Color = Color::from_hex(0x1E1E2E);
const COLOR_MANTLE: Color = Color::from_hex(0x181825);
const COLOR_SURFACE0: Color = Color::from_hex(0x313244);
const COLOR_SURFACE1: Color = Color::from_hex(0x45475A);
const COLOR_TEXT: Color = Color::from_hex(0xCDD6F4);
const COLOR_SUBTEXT: Color = Color::from_hex(0xA6ADC8);
const COLOR_BLUE: Color = Color::from_hex(0x89B4FA);
const COLOR_GREEN: Color = Color::from_hex(0xA6E3A1);
const COLOR_RED: Color = Color::from_hex(0xF38BA8);
const COLOR_PEACH: Color = Color::from_hex(0xFAB387);
const COLOR_MAUVE: Color = Color::from_hex(0xCBA6F7);
const COLOR_TEAL: Color = Color::from_hex(0x94E2D5);

// ============================================================================
// Layout constants
// ============================================================================

/// The window the calculator asks for.
///
/// Tall enough for the *most* it can ever show -- Scientific mode with the
/// history panel open -- rather than for the Standard keypad it opens with.
/// There is no way for a window to ask to be resized once it is open, so a
/// window sized to the opening state would crowd ten rows of keys into the
/// space made for six the instant the user pressed "Scientific". Opening
/// larger costs a roomier Standard keypad; opening smaller costs a Scientific
/// keypad the user has to resize the window to use.
const WINDOW_WIDTH: f32 = 340.0;
const WINDOW_HEIGHT: f32 = 600.0;

/// Margin between the window edge and everything in it.
const PADDING: f32 = 6.0;
/// Space between two adjacent keys, and between two stacked bands.
const GAP: f32 = 3.0;
const STATUS_HEIGHT: f32 = 24.0;
const DISPLAY_HEIGHT: f32 = 70.0;
const HISTORY_HEIGHT: f32 = 150.0;
const HISTORY_TITLE_HEIGHT: f32 = 18.0;
const HISTORY_ROW_HEIGHT: f32 = 30.0;
const CORNER_RADIUS: f32 = 4.0;

const FONT_SIZE_KEY: f32 = 16.0;
const FONT_SIZE_SMALL_KEY: f32 = 11.0;
const FONT_SIZE_RESULT: f32 = 26.0;
const FONT_SIZE_EXPR: f32 = 12.0;
const FONT_SIZE_STATUS: f32 = 11.0;
const FONT_SIZE_HISTORY_EXPR: f32 = 10.0;
const FONT_SIZE_HISTORY_RESULT: f32 = 13.0;

// ============================================================================
// The keys
// ============================================================================

/// The memory row, present in both modes.
const MEMORY_ROW: [&str; 5] = ["MC", "MR", "M+", "M-", "MS"];

/// The four rows Scientific mode adds above the keypad.
const SCIENTIFIC_ROWS: [[&str; 5]; 4] = [
    ["sin", "cos", "tan", "(", ")"],
    ["asin", "acos", "atan", "x^y", "mod"],
    ["ln", "log", "sqrt", "exp", "n!"],
    ["abs", "floor", "ceil", "pi", "e"],
];

/// The numeric keypad, present in both modes.
const KEYPAD_ROWS: [[&str; 4]; 5] = [
    ["CE", "C", "\u{232B}", "/"],
    ["7", "8", "9", "*"],
    ["4", "5", "6", "-"],
    ["1", "2", "3", "+"],
    ["\u{00B1}", "0", ".", "="],
];

/// Every key row, top to bottom, for the given mode.
///
/// One function rather than a count in the layout and a list in the renderer:
/// those are the two things that must agree about how many rows there are, and
/// making them the same call means they cannot disagree.
fn key_rows(mode: CalcMode) -> Vec<&'static [&'static str]> {
    let mut rows: Vec<&'static [&'static str]> = vec![&MEMORY_ROW];
    if mode == CalcMode::Scientific {
        rows.extend(SCIENTIFIC_ROWS.iter().map(<[&str; 5]>::as_slice));
    }
    rows.extend(KEYPAD_ROWS.iter().map(<[&str; 4]>::as_slice));
    rows
}

/// The background and label colours for a key, chosen by what it does.
///
/// Keyed on the label because the label *is* the key's identity here -- the
/// same string [`Target::Key`] carries and [`handle_button`] dispatches on.
fn key_colors(label: &str) -> (Color, Color) {
    match label {
        "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "." => {
            (COLOR_SURFACE0, COLOR_TEXT)
        }
        "=" => (COLOR_BLUE, COLOR_BASE),
        "C" | "CE" => (COLOR_SURFACE1, COLOR_RED),
        "MC" | "MR" | "M+" | "M-" | "MS" => (COLOR_SURFACE1, COLOR_GREEN),
        "pi" | "e" => (COLOR_SURFACE1, COLOR_MAUVE),
        "+" | "-" | "*" | "/" | "mod" | "x^y" | "%" | "(" | ")" | "\u{232B}" | "\u{00B1}" => {
            (COLOR_SURFACE1, COLOR_PEACH)
        }
        // Everything else on a key row is a function: sin, log, n! and friends.
        _ => (COLOR_SURFACE1, COLOR_TEAL),
    }
}

/// How big to draw a key's label.
///
/// Long names -- `floor`, `asin` -- get the smaller size so they fit a cell
/// sized for `7`. Measured in characters rather than pixels because the cell
/// width is not known until the window size is, and a key whose font changed
/// as the window resized would be worse than one that is merely small.
fn key_font_size(label: &str) -> f32 {
    if label.chars().count() <= 2 {
        FONT_SIZE_KEY
    } else {
        FONT_SIZE_SMALL_KEY
    }
}

// ============================================================================
// Targets
// ============================================================================

/// Everything in the window that answers a click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A key, named by the label it draws.
    ///
    /// The label is the identity rather than a separate enum variant per key,
    /// because [`handle_button`] already dispatches on the label: a second name
    /// would be a second thing to keep in step with the first, and a key whose
    /// two names disagreed would be a key that lit up and did nothing.
    Key(&'static str),
    /// A row of the history panel, by its index into [`Calculator::history`].
    HistoryRow(usize),
}

/// The frame this window draws into, with its hit boxes.
pub type Frame = guitk::frame::Frame<Target>;

// ============================================================================
// Layout
// ============================================================================

/// A window size that can be laid out against: never negative, never NaN.
fn sane(v: f32) -> f32 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

/// Divide `span` into `count` cells separated by [`GAP`], and return the size
/// of one cell.
///
/// Never negative. A window too narrow for the gaps alone yields zero-sized
/// cells, and a zero-sized cell records a hit box that no point is inside --
/// which is the right answer for a key that cannot be seen.
#[allow(clippy::cast_precision_loss)]
fn cell_size(span: f32, count: usize) -> f32 {
    if count == 0 {
        return 0.0;
    }
    let n = count as f32;
    ((span - GAP * (n - 1.0)) / n).max(0.0)
}

/// The `index`th of `count` cells laid across `span` starting at `origin`.
#[allow(clippy::cast_precision_loss)]
fn cell_at(origin: f32, span: f32, count: usize, index: usize) -> (f32, f32) {
    let size = cell_size(span, count);
    (origin + (index as f32) * (size + GAP), size)
}

/// Take a band `want` tall off the top of what is left below `y`, and advance
/// `y` past it and the gap that follows it.
///
/// The band **shrinks** to what remains rather than being clamped to a minimum.
/// [`Frame`] does not clip to the window, so a band held at a minimum height in
/// a window too short for it would record hit boxes below the bottom edge --
/// controls that cannot be seen but can be pressed.
fn take_band(y: &mut f32, limit: f32, x: f32, width: f32, want: f32) -> Rect {
    let h = want.min((limit - *y).max(0.0));
    let band = Rect::new(x, *y, width, h);
    *y += h + GAP;
    band
}

/// Where every band goes, derived from the live window size.
///
/// Built fresh on every frame and never stored. The size a window *is* and the
/// size it was last told to be are two different things for exactly one frame
/// -- the first one, which arrives before any `Event::Resize` -- and that is
/// the frame in which a remembered layout is wrong.
#[derive(Clone, Debug)]
struct Layout {
    /// The whole window.
    window: Rect,
    /// The mode / angle / memory / history strip along the top.
    status: Rect,
    /// The expression-and-result panel below it.
    display: Rect,
    /// One rect per row of keys, in the order [`key_rows`] gives them.
    rows: Vec<Rect>,
    /// The history panel along the bottom, when it is showing.
    history: Option<Rect>,
}

impl Layout {
    fn new(width: f32, height: f32, mode: CalcMode, show_history: bool) -> Self {
        let width = sane(width);
        let height = sane(height);
        let window = Rect::new(0.0, 0.0, width, height);

        // Padding shrinks with the window rather than being subtracted from it,
        // so a 4px-wide window gets a 4px-wide content area instead of a
        // negative one.
        let pad = PADDING.min(width / 2.0).min(height / 2.0);
        let content_w = (width - pad * 2.0).max(0.0);
        let content_bottom = (height - pad).max(pad);

        let mut y = pad;
        let status = take_band(&mut y, content_bottom, pad, content_w, STATUS_HEIGHT);
        let display = take_band(&mut y, content_bottom, pad, content_w, DISPLAY_HEIGHT);

        // The history panel is taken off the *bottom* before the key rows share
        // what is left, so opening it shrinks the keypad rather than pushing it
        // off the window -- where, since the frame does not clip to the window,
        // the keys would still be clickable.
        let (history, rows_bottom) = if show_history {
            let h = HISTORY_HEIGHT.min((content_bottom - y).max(0.0));
            let top = content_bottom - h;
            (Some(Rect::new(pad, top, content_w, h)), (top - GAP).max(y))
        } else {
            (None, content_bottom)
        };

        let count = key_rows(mode).len();
        let span = (rows_bottom - y).max(0.0);
        let rows = (0..count)
            .map(|i| {
                let (top, h) = cell_at(y, span, count, i);
                Rect::new(pad, top, content_w, h)
            })
            .collect();

        Self {
            window,
            status,
            display,
            rows,
            history,
        }
    }
}

// ============================================================================
// The window
// ============================================================================

/// The calculator plus the window it is drawn in.
///
/// The window size lives here and nowhere else, and is only ever *recorded* --
/// every rectangle is recomputed from it on each frame, so there is no cached
/// geometry that a resize could leave stale.
pub struct CalculatorUi {
    /// The calculator proper: expression, history, memory, mode.
    pub calc: Calculator,
    window_width: f32,
    window_height: f32,
    /// Index of the first history row drawn, for the wheel.
    history_scroll: usize,
    /// Banks fractions of a wheel notch so a trackpad's small deltas add up.
    wheel: wheel::Accumulator,
}

impl Default for CalculatorUi {
    fn default() -> Self {
        Self::new()
    }
}

impl CalculatorUi {
    /// A new calculator window, at the size it will ask the desktop for.
    #[must_use]
    pub fn new() -> Self {
        Self {
            calc: Calculator::new(),
            window_width: WINDOW_WIDTH,
            window_height: WINDOW_HEIGHT,
            history_scroll: 0,
            wheel: wheel::Accumulator::default(),
        }
    }

    /// Record a new window size.
    fn resize(&mut self, width: f32, height: f32) {
        self.window_width = sane(width);
        self.window_height = sane(height);
    }

    /// Draw the whole window at `width` x `height`.
    ///
    /// The size is passed in and never stored, so a resize cannot leave a stale
    /// rectangle behind for the hit test to consult. Hit boxes are recorded by
    /// the same code that paints, which is what keeps a key's clickable area
    /// and its visible area from drifting apart.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let layout = Layout::new(width, height, self.calc.mode, self.calc.show_history);
        let mut frame = Frame::new(layout.window.w, layout.window.h);

        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: layout.window.w,
            height: layout.window.h,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        self.render_status(&mut frame, layout.status);
        self.render_display(&mut frame, layout.display);
        self.render_keys(&mut frame, &layout);
        if let Some(panel) = layout.history {
            self.render_history(&mut frame, panel);
        }

        frame
    }

    /// What a click at `(x, y)` would land on, given the current window size.
    ///
    /// Answered by drawing the frame and asking it, rather than by a parallel
    /// set of rectangles: one geometry, so a key that moves takes its clickable
    /// area with it.
    #[must_use]
    pub fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.window_width, self.window_height)
            .hit_test(x, y)
    }

    /// Draw one key: its background, its centred label, and its hit box.
    fn render_key(frame: &mut Frame, cell: Rect, label: &'static str, font_size: f32) {
        if cell.w <= 0.0 || cell.h <= 0.0 {
            return;
        }
        let (bg, fg) = key_colors(label);
        frame.push(RenderCommand::FillRect {
            x: cell.x,
            y: cell.y,
            width: cell.w,
            height: cell.h,
            color: bg,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        let (centre_x, _) = cell.centre();
        frame.push(RenderCommand::Text {
            x: text::center_x(label, centre_x, font_size, FontWeightHint::Regular).max(cell.x),
            y: cell.y + (cell.h - font_size) / 2.0,
            text: String::from(label),
            color: fg,
            font_size,
            font_weight: FontWeightHint::Regular,
            max_width: Some(cell.w),
            overflow: TextOverflow::Ellipsis,
        });
        frame.hit(Target::Key(label), cell);
    }

    /// The mode / angle / memory / history strip.
    ///
    /// Four equal cells rather than four intrinsic widths: a strip whose
    /// buttons were as wide as their labels would have "Scientific" and "Hist"
    /// overlap in a narrow window, and two hit boxes on the same pixel mean the
    /// user presses whichever was drawn last.
    fn render_status(&self, frame: &mut Frame, band: Rect) {
        if band.h <= 0.0 {
            return;
        }
        let mode_label = match self.calc.mode {
            CalcMode::Standard => "Standard",
            CalcMode::Scientific => "Scientific",
        };
        let angle_label = match self.calc.angle_unit {
            AngleUnit::Degrees => "DEG",
            AngleUnit::Radians => "RAD",
        };

        for (index, label) in [mode_label, angle_label].into_iter().enumerate() {
            let (x, w) = cell_at(band.x, band.w, 4, index);
            Self::render_status_button(frame, Rect::new(x, band.y, w, band.h), label);
        }

        // The memory cell is a readout, not a button: it names what is stored
        // so that MR is not a guess, and it records no hit box because there is
        // nothing for a click on it to do.
        let (mem_x, mem_w) = cell_at(band.x, band.w, 4, 2);
        if self.calc.memory_set {
            frame.push(RenderCommand::Text {
                x: mem_x,
                y: band.y + (band.h - FONT_SIZE_STATUS) / 2.0,
                text: format!("M {}", format_result(self.calc.memory)),
                color: COLOR_GREEN,
                font_size: FONT_SIZE_STATUS,
                font_weight: FontWeightHint::Bold,
                max_width: Some(mem_w),
                overflow: TextOverflow::Ellipsis,
            });
        }

        let (hist_x, hist_w) = cell_at(band.x, band.w, 4, 3);
        Self::render_status_button(frame, Rect::new(hist_x, band.y, hist_w, band.h), "Hist");
    }

    fn render_status_button(frame: &mut Frame, cell: Rect, label: &'static str) {
        if cell.w <= 0.0 || cell.h <= 0.0 {
            return;
        }
        frame.push(RenderCommand::FillRect {
            x: cell.x,
            y: cell.y,
            width: cell.w,
            height: cell.h,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        let (centre_x, _) = cell.centre();
        frame.push(RenderCommand::Text {
            x: text::center_x(label, centre_x, FONT_SIZE_STATUS, FontWeightHint::Regular)
                .max(cell.x),
            y: cell.y + (cell.h - FONT_SIZE_STATUS) / 2.0,
            text: String::from(label),
            color: COLOR_BLUE,
            font_size: FONT_SIZE_STATUS,
            font_weight: FontWeightHint::Regular,
            max_width: Some(cell.w),
            overflow: TextOverflow::Ellipsis,
        });
        frame.hit(Target::Key(label), cell);
    }

    /// The expression above, the result below, both right-aligned.
    fn render_display(&self, frame: &mut Frame, band: Rect) {
        if band.h <= 0.0 {
            return;
        }
        frame.push(RenderCommand::FillRect {
            x: band.x,
            y: band.y,
            width: band.w,
            height: band.h,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        frame.push(RenderCommand::StrokeRect {
            x: band.x,
            y: band.y,
            width: band.w,
            height: band.h,
            color: COLOR_SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        let right = band.right() - PADDING;
        let inner_w = (band.w - PADDING * 2.0).max(0.0);

        // Unclosed parentheses are shown as a count on the left. Without it the
        // only sign that a `(` is still open is the expression itself, which is
        // exactly what a user who has lost count is already staring at.
        if self.calc.paren_depth > 0 {
            frame.push(RenderCommand::Text {
                x: band.x + PADDING,
                y: band.y + PADDING,
                text: format!("({}", self.calc.paren_depth),
                color: COLOR_PEACH,
                font_size: FONT_SIZE_EXPR,
                font_weight: FontWeightHint::Bold,
                max_width: Some(inner_w),
                overflow: TextOverflow::Ellipsis,
            });
        }

        frame.push(RenderCommand::Text {
            x: text::right_x(
                &self.calc.expression,
                right,
                FONT_SIZE_EXPR,
                FontWeightHint::Regular,
            )
            .max(band.x + PADDING),
            y: band.y + PADDING,
            text: self.calc.expression.clone(),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_EXPR,
            font_weight: FontWeightHint::Regular,
            max_width: Some(inner_w),
            overflow: TextOverflow::Ellipsis,
        });

        // An error is red. It is the one thing in the display that is not a
        // number, and reading it in the same colour as one is how a user comes
        // to believe the calculator answered.
        let result_color = if self.calc.display.starts_with("Error:") {
            COLOR_RED
        } else {
            COLOR_TEXT
        };
        frame.push(RenderCommand::Text {
            x: text::right_x(
                &self.calc.display,
                right,
                FONT_SIZE_RESULT,
                FontWeightHint::Bold,
            )
            .max(band.x + PADDING),
            y: band.bottom() - PADDING - FONT_SIZE_RESULT,
            text: self.calc.display.clone(),
            color: result_color,
            font_size: FONT_SIZE_RESULT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(inner_w),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_keys(&self, frame: &mut Frame, layout: &Layout) {
        for (labels, band) in key_rows(self.calc.mode).into_iter().zip(&layout.rows) {
            for (index, label) in labels.iter().enumerate() {
                let (x, w) = cell_at(band.x, band.w, labels.len(), index);
                Self::render_key(
                    frame,
                    Rect::new(x, band.y, w, band.h),
                    label,
                    key_font_size(label),
                );
            }
        }
    }

    /// How many history rows fit in the panel at this window size.
    fn history_capacity(&self) -> usize {
        let Some(panel) = Layout::new(
            self.window_width,
            self.window_height,
            self.calc.mode,
            self.calc.show_history,
        )
        .history
        else {
            return 0;
        };
        scroll_window::capacity(HISTORY_ROW_HEIGHT, panel.h - HISTORY_TITLE_HEIGHT)
    }

    fn render_history(&self, frame: &mut Frame, panel: Rect) {
        if panel.h <= 0.0 {
            return;
        }
        frame.push(RenderCommand::FillRect {
            x: panel.x,
            y: panel.y,
            width: panel.w,
            height: panel.h,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        frame.push(RenderCommand::Text {
            x: panel.x + PADDING,
            y: panel.y + 2.0,
            text: String::from("History"),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_STATUS,
            font_weight: FontWeightHint::Bold,
            max_width: Some(panel.w),
            overflow: TextOverflow::Ellipsis,
        });

        let list = Rect::new(
            panel.x,
            panel.y + HISTORY_TITLE_HEIGHT,
            panel.w,
            (panel.h - HISTORY_TITLE_HEIGHT).max(0.0),
        );

        if self.calc.history.is_empty() {
            frame.push(RenderCommand::Text {
                x: list.x + PADDING,
                y: list.y + 2.0,
                text: String::from("No history yet"),
                color: COLOR_SURFACE1,
                font_size: FONT_SIZE_HISTORY_EXPR,
                font_weight: FontWeightHint::Regular,
                max_width: Some(list.w),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }

        // Clipping the list also clips its hit boxes: `Frame::hit` trims to the
        // innermost clip and drops a box left with no area, so a row scrolled
        // past the bottom stops being clickable without the click handler
        // needing a bounds check of its own.
        frame.clip(list);
        let rows = scroll_window::visible(
            self.calc.history.len(),
            HISTORY_ROW_HEIGHT,
            list.h,
            self.history_scroll,
        );
        for offset in 0..rows.count {
            let index = rows.start.saturating_add(offset);
            let Some(entry) = self.calc.history.get(index) else {
                break;
            };
            #[allow(clippy::cast_precision_loss)]
            let top = list.y + (offset as f32) * HISTORY_ROW_HEIGHT;
            let row = Rect::new(list.x, top, list.w, HISTORY_ROW_HEIGHT);
            frame.push(RenderCommand::Text {
                x: row.x + PADDING,
                y: row.y + 1.0,
                text: entry.expression.clone(),
                color: COLOR_SUBTEXT,
                font_size: FONT_SIZE_HISTORY_EXPR,
                font_weight: FontWeightHint::Regular,
                max_width: Some((row.w - PADDING * 2.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            frame.push(RenderCommand::Text {
                x: row.x + PADDING,
                y: row.y + 13.0,
                text: format!("= {}", entry.result),
                color: COLOR_TEXT,
                font_size: FONT_SIZE_HISTORY_RESULT,
                font_weight: FontWeightHint::Bold,
                max_width: Some((row.w - PADDING * 2.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            frame.hit(Target::HistoryRow(index), row);
        }
        frame.unclip();
    }

    // ======================================================================
    // Input
    // ======================================================================

    /// Act on a left click. Returns `true` when something changed.
    fn handle_click(&mut self, x: f32, y: f32) -> bool {
        match self.target_at(x, y) {
            Some(Target::Key(label)) => {
                let was_showing = self.calc.show_history;
                handle_button(&mut self.calc, label);
                // A history panel that has just been opened shows the newest
                // entries; one re-opened at yesterday's scroll position would
                // show whatever the user happened to be looking at last time.
                if self.calc.show_history != was_showing {
                    self.history_scroll = 0;
                }
                true
            }
            Some(Target::HistoryRow(index)) => self.calc.recall_history(index),
            None => false,
        }
    }

    /// Act on a key press. Returns `true` when something changed.
    fn handle_key_event(&mut self, key: &KeyEvent) -> bool {
        handle_key(&mut self.calc, key)
    }

    /// Scroll the history panel. Returns `true` when it actually moved.
    fn handle_scroll(&mut self, dy: f32) -> bool {
        if !self.calc.show_history {
            return false;
        }
        let rows = self.wheel.rows(dy);
        if rows == 0 {
            return false;
        }
        let capacity = self.history_capacity();
        let max_start = self.calc.history.len().saturating_sub(capacity);
        let moved = scroll_window::shift(self.history_scroll, rows).min(max_start);
        if moved == self.history_scroll {
            return false;
        }
        self.history_scroll = moved;
        true
    }
}

// ============================================================================
// Event dispatch — map button clicks and key presses to calculator actions
// ============================================================================

/// Handle a button press by its label text.
pub fn handle_button(calc: &mut Calculator, label: &str) {
    match label {
        "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" => {
            if let Some(digit) = label.chars().next() {
                calc.input_digit(digit);
            }
        }
        "." => calc.input_decimal(),
        "+" => calc.input_operator('+'),
        "-" => calc.input_operator('-'),
        "*" => calc.input_operator('*'),
        "/" => calc.input_operator('/'),
        "mod" => calc.input_operator('%'),
        "=" => calc.calculate(),
        "C" => calc.clear_all(),
        "CE" => calc.clear_entry(),
        "\u{232B}" => calc.input_backspace(), // Backspace symbol
        "\u{00B1}" => calc.input_negate(),    // Plus-minus sign
        "%" => calc.input_percent(),
        "(" => calc.input_open_paren(),
        ")" => calc.input_close_paren(),
        "sin" => calc.input_function("sin"),
        "cos" => calc.input_function("cos"),
        "tan" => calc.input_function("tan"),
        "asin" => calc.input_function("asin"),
        "acos" => calc.input_function("acos"),
        "atan" => calc.input_function("atan"),
        "ln" => calc.input_function("ln"),
        "log" => calc.input_function("log"),
        "sqrt" => calc.input_function("sqrt"),
        "exp" => calc.input_function("exp"),
        "abs" => calc.input_function("abs"),
        "floor" => calc.input_function("floor"),
        "ceil" => calc.input_function("ceil"),
        "n!" => calc.input_function("fact"),
        "x^y" => calc.input_operator('^'),
        "pi" => calc.input_constant("pi"),
        "e" => calc.input_constant("e"),
        "MC" => calc.memory_clear(),
        "MR" => calc.memory_recall(),
        "M+" => calc.memory_add(),
        "M-" => calc.memory_subtract(),
        "MS" => calc.memory_store(),
        "Standard" | "Scientific" => calc.toggle_mode(),
        "DEG" | "RAD" => calc.toggle_angle_unit(),
        "Hist" => calc.toggle_history(),
        _ => {} // Unknown button — ignore.
    }
}

/// Handle a keyboard event and translate it to calculator actions.
///
/// Returns `true` if the key was handled.
pub fn handle_key(calc: &mut Calculator, key: &KeyEvent) -> bool {
    if !key.pressed {
        return false;
    }

    // Digit keys (both main keyboard and numpad produce text events).
    // `single_char`, not every character typed: each arm below is a *command*
    // -- press this button -- and a keystroke that produced two characters
    // named no single button.
    if let Some(ch) = key.single_char() {
        match ch {
            '0'..='9' => {
                calc.input_digit(ch);
                return true;
            }
            '.' => {
                calc.input_decimal();
                return true;
            }
            '+' => {
                calc.input_operator('+');
                return true;
            }
            '-' => {
                calc.input_operator('-');
                return true;
            }
            '*' => {
                calc.input_operator('*');
                return true;
            }
            '/' => {
                calc.input_operator('/');
                return true;
            }
            '%' => {
                calc.input_percent();
                return true;
            }
            '^' => {
                calc.input_operator('^');
                return true;
            }
            '(' => {
                calc.input_open_paren();
                return true;
            }
            ')' => {
                calc.input_close_paren();
                return true;
            }
            _ => {}
        }
    }

    match key.key {
        Key::Enter => {
            calc.calculate();
            true
        }
        Key::Escape => {
            calc.clear_all();
            true
        }
        Key::Backspace => {
            calc.input_backspace();
            true
        }
        Key::Delete => {
            calc.clear_entry();
            true
        }
        _ => false,
    }
}

// ============================================================================
// The window
// ============================================================================

/// Turn a window event into a change of state.
///
/// Free rather than a method so the whole event vocabulary can be read in one
/// place, and so the `App` impl below is the thin adapter it should be.
fn handle_event(ui: &mut CalculatorUi, event: &Event) -> EventResult {
    /// `true` means the state changed and the window needs repainting.
    fn result(changed: bool) -> EventResult {
        if changed {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    match event {
        Event::Mouse(m) => match m.kind {
            MouseEventKind::Press(MouseButton::Left) => result(ui.handle_click(m.x, m.y)),
            MouseEventKind::Scroll { dy, .. } => result(ui.handle_scroll(dy)),
            _ => EventResult::Ignored,
        },
        Event::Key(k) => result(ui.handle_key_event(k)),
        Event::Resize { width, height } => {
            // Consumed unconditionally: the size was recorded, which is a
            // change even when nothing visible moved.
            #[allow(clippy::cast_precision_loss)]
            ui.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for CalculatorUi {
    fn title(&self) -> String {
        String::from("Calculator")
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// No tick.
    ///
    /// Nothing here changes on its own: a calculator showing `7` shows `7`
    /// until someone presses something. A timer would repaint an identical
    /// picture forever and keep the machine awake to do it.
    fn tick_interval(&self) -> Option<std::time::Duration> {
        None
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match handle_event(self, event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The renderer draws at the size it is given, but `target_at` has only
        // what it was last told. Recording it here means the two agree even if
        // the platform ever draws at a size it did not send a Resize for.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for CalculatorUi {
    type Target = Target;
    type Outcome = EventResult;

    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        handle_event(
            self,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }),
        )
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        handle_event(self, &Event::Key(key.clone()))
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() -> ExitCode {
    let mut ui = CalculatorUi::new();
    app::launch("calculator", &mut ui)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the line
    // that did it -- that is the diagnosis. The defensive lints exist to keep
    // panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use guitk::event::Modifiers;
    use guitk::probe;

    // ----------------------------------------------------------------
    // Expression evaluator tests
    // ----------------------------------------------------------------

    #[test]
    fn test_basic_addition() {
        let result = evaluate("2 + 3", AngleUnit::Degrees);
        assert_eq!(result, Ok(5.0));
    }

    #[test]
    fn test_operator_precedence() {
        // 2 + 3 * 4 = 14 (not 20).
        let result = evaluate("2 + 3 * 4", AngleUnit::Degrees);
        assert_eq!(result, Ok(14.0));
    }

    #[test]
    fn test_operator_precedence_complex() {
        // 1 + 2 * 3 + 4 = 1 + 6 + 4 = 11
        let result = evaluate("1 + 2 * 3 + 4", AngleUnit::Degrees);
        assert_eq!(result, Ok(11.0));
    }

    #[test]
    fn test_parentheses() {
        let result = evaluate("(2 + 3) * 4", AngleUnit::Degrees);
        assert_eq!(result, Ok(20.0));
    }

    #[test]
    fn test_nested_parentheses() {
        let result = evaluate("((2 + 3) * (4 - 1))", AngleUnit::Degrees);
        assert_eq!(result, Ok(15.0));
    }

    #[test]
    fn test_division() {
        let result = evaluate("10 / 4", AngleUnit::Degrees);
        assert_eq!(result, Ok(2.5));
    }

    #[test]
    fn test_division_by_zero() {
        let result = evaluate("5 / 0", AngleUnit::Degrees);
        assert_eq!(result, Err("Division by zero"));
    }

    #[test]
    fn test_modulo() {
        let result = evaluate("17 % 5", AngleUnit::Degrees);
        assert_eq!(result, Ok(2.0));
    }

    #[test]
    fn test_modulo_by_zero() {
        let result = evaluate("5 % 0", AngleUnit::Degrees);
        assert_eq!(result, Err("Division by zero"));
    }

    #[test]
    fn test_power() {
        let result = evaluate("2 ^ 10", AngleUnit::Degrees);
        assert_eq!(result, Ok(1024.0));
    }

    #[test]
    fn test_power_right_associative() {
        // 2^3^2 should be 2^(3^2) = 2^9 = 512, not (2^3)^2 = 64
        let result = evaluate("2 ^ 3 ^ 2", AngleUnit::Degrees);
        assert_eq!(result, Ok(512.0));
    }

    #[test]
    fn test_unary_minus() {
        let result = evaluate("-5", AngleUnit::Degrees);
        assert_eq!(result, Ok(-5.0));
    }

    #[test]
    fn test_unary_minus_in_expression() {
        let result = evaluate("3 + -2", AngleUnit::Degrees);
        assert_eq!(result, Ok(1.0));
    }

    #[test]
    fn test_double_negation() {
        let result = evaluate("--5", AngleUnit::Degrees);
        assert_eq!(result, Ok(5.0));
    }

    #[test]
    fn test_pi_constant() {
        let result = evaluate("pi", AngleUnit::Degrees);
        assert_eq!(result, Ok(PI));
    }

    #[test]
    fn test_e_constant() {
        let result = evaluate("e", AngleUnit::Degrees);
        assert_eq!(result, Ok(E));
    }

    // ----------------------------------------------------------------
    // Trigonometric function tests
    // ----------------------------------------------------------------

    #[test]
    fn test_sin_degrees() {
        let result = evaluate("sin(90)", AngleUnit::Degrees);
        assert!((result.expect("should succeed") - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_cos_radians() {
        let result = evaluate("cos(0)", AngleUnit::Radians);
        assert_eq!(result, Ok(1.0));
    }

    #[test]
    fn test_tan_degrees() {
        let result = evaluate("tan(45)", AngleUnit::Degrees);
        assert!((result.expect("should succeed") - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_asin() {
        let result = evaluate("asin(1)", AngleUnit::Degrees);
        assert!((result.expect("should succeed") - 90.0).abs() < 1e-10);
    }

    #[test]
    fn test_asin_domain_error() {
        let result = evaluate("asin(2)", AngleUnit::Degrees);
        assert_eq!(result, Err("Domain error (asin)"));
    }

    // ----------------------------------------------------------------
    // Logarithmic and exponential tests
    // ----------------------------------------------------------------

    #[test]
    fn test_ln() {
        let result = evaluate("ln(1)", AngleUnit::Degrees);
        assert_eq!(result, Ok(0.0));
    }

    #[test]
    fn test_ln_e() {
        let result = evaluate("ln(e)", AngleUnit::Degrees);
        assert!((result.expect("should succeed") - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_ln_domain_error() {
        let result = evaluate("ln(0)", AngleUnit::Degrees);
        assert_eq!(result, Err("Domain error (ln)"));
    }

    #[test]
    fn test_log10() {
        let result = evaluate("log(100)", AngleUnit::Degrees);
        assert!((result.expect("should succeed") - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_sqrt() {
        let result = evaluate("sqrt(144)", AngleUnit::Degrees);
        assert_eq!(result, Ok(12.0));
    }

    #[test]
    fn test_sqrt_domain_error() {
        let result = evaluate("sqrt(-1)", AngleUnit::Degrees);
        assert_eq!(result, Err("Domain error (sqrt)"));
    }

    #[test]
    fn test_exp() {
        let result = evaluate("exp(0)", AngleUnit::Degrees);
        assert_eq!(result, Ok(1.0));
    }

    #[test]
    fn test_abs() {
        let result = evaluate("abs(-42)", AngleUnit::Degrees);
        assert_eq!(result, Ok(42.0));
    }

    #[test]
    fn test_floor() {
        let result = evaluate("floor(3.7)", AngleUnit::Degrees);
        assert_eq!(result, Ok(3.0));
    }

    #[test]
    fn test_ceil() {
        let result = evaluate("ceil(3.2)", AngleUnit::Degrees);
        assert_eq!(result, Ok(4.0));
    }

    // ----------------------------------------------------------------
    // Factorial tests
    // ----------------------------------------------------------------

    #[test]
    fn test_factorial_zero() {
        let result = evaluate("fact(0)", AngleUnit::Degrees);
        assert_eq!(result, Ok(1.0));
    }

    #[test]
    fn test_factorial_five() {
        let result = evaluate("fact(5)", AngleUnit::Degrees);
        assert_eq!(result, Ok(120.0));
    }

    #[test]
    fn test_factorial_negative() {
        let result = evaluate("fact(-1)", AngleUnit::Degrees);
        assert_eq!(result, Err("Domain error (fact)"));
    }

    #[test]
    fn test_factorial_non_integer() {
        let result = evaluate("fact(3.5)", AngleUnit::Degrees);
        assert_eq!(result, Err("Domain error (fact)"));
    }

    #[test]
    fn test_factorial_overflow() {
        let result = evaluate("fact(171)", AngleUnit::Degrees);
        assert_eq!(result, Err("Overflow (fact)"));
    }

    // ----------------------------------------------------------------
    // Nested function tests
    // ----------------------------------------------------------------

    #[test]
    fn test_nested_functions() {
        // sqrt(abs(-16)) = sqrt(16) = 4
        let result = evaluate("sqrt(abs(-16))", AngleUnit::Degrees);
        assert_eq!(result, Ok(4.0));
    }

    #[test]
    fn test_function_with_expression_arg() {
        // sqrt(3 + 1) = sqrt(4) = 2
        let result = evaluate("sqrt(3 + 1)", AngleUnit::Degrees);
        assert_eq!(result, Ok(2.0));
    }

    // ----------------------------------------------------------------
    // Calculator state tests
    // ----------------------------------------------------------------

    #[test]
    fn test_calculator_digit_input() {
        let mut calc = Calculator::new();
        calc.input_digit('1');
        calc.input_digit('2');
        calc.input_digit('3');
        assert_eq!(calc.expression, "123");
        assert_eq!(calc.display, "123");
    }

    #[test]
    fn test_calculator_expression_with_ops() {
        let mut calc = Calculator::new();
        calc.input_digit('2');
        calc.input_operator('+');
        calc.input_digit('3');
        calc.input_operator('*');
        calc.input_digit('4');
        calc.calculate();
        assert_eq!(calc.display, "14"); // Correct precedence.
    }

    #[test]
    fn test_calculator_clear() {
        let mut calc = Calculator::new();
        calc.input_digit('5');
        calc.input_digit('5');
        calc.clear_all();
        assert_eq!(calc.display, "0");
        assert_eq!(calc.expression, "");
    }

    #[test]
    fn test_calculator_backspace() {
        let mut calc = Calculator::new();
        calc.input_digit('1');
        calc.input_digit('2');
        calc.input_digit('3');
        calc.input_backspace();
        assert_eq!(calc.expression, "12");
    }

    #[test]
    fn test_calculator_memory() {
        let mut calc = Calculator::new();
        calc.input_digit('4');
        calc.input_digit('2');
        calc.memory_store();
        assert!(calc.memory_set);
        assert_eq!(calc.memory, 42.0);

        calc.clear_all();
        calc.memory_recall();
        assert_eq!(calc.expression, "42");

        calc.clear_all();
        calc.input_digit('8');
        calc.memory_add();
        assert_eq!(calc.memory, 50.0);

        calc.clear_all();
        calc.input_digit('5');
        calc.memory_subtract();
        assert_eq!(calc.memory, 45.0);

        calc.memory_clear();
        assert!(!calc.memory_set);
        assert_eq!(calc.memory, 0.0);
    }

    #[test]
    fn test_calculator_history() {
        let mut calc = Calculator::new();
        calc.input_digit('2');
        calc.input_operator('+');
        calc.input_digit('2');
        calc.calculate();
        assert_eq!(calc.history.len(), 1);
        assert_eq!(calc.history.front().map(|h| h.result.as_str()), Some("4"));
    }

    #[test]
    fn test_calculator_history_max() {
        let mut calc = Calculator::new();
        for i in 0..(MAX_HISTORY + 5) {
            calc.expression = format!("{i}");
            calc.calculate();
        }
        assert_eq!(calc.history.len(), MAX_HISTORY);
    }

    #[test]
    fn test_calculator_mode_toggle() {
        let mut calc = Calculator::new();
        assert_eq!(calc.mode, CalcMode::Standard);
        calc.toggle_mode();
        assert_eq!(calc.mode, CalcMode::Scientific);
        calc.toggle_mode();
        assert_eq!(calc.mode, CalcMode::Standard);
    }

    #[test]
    fn test_calculator_angle_toggle() {
        let mut calc = Calculator::new();
        assert_eq!(calc.angle_unit, AngleUnit::Degrees);
        calc.toggle_angle_unit();
        assert_eq!(calc.angle_unit, AngleUnit::Radians);
        calc.toggle_angle_unit();
        assert_eq!(calc.angle_unit, AngleUnit::Degrees);
    }

    #[test]
    fn test_calculator_decimal() {
        let mut calc = Calculator::new();
        calc.input_digit('3');
        calc.input_decimal();
        calc.input_digit('1');
        calc.input_digit('4');
        assert_eq!(calc.expression, "3.14");
        // Double decimal should be ignored.
        calc.input_decimal();
        assert_eq!(calc.expression, "3.14");
    }

    #[test]
    fn test_calculator_result_resets_on_digit() {
        let mut calc = Calculator::new();
        calc.input_digit('5');
        calc.calculate();
        assert!(calc.showing_result);
        calc.input_digit('3');
        assert!(!calc.showing_result);
        assert_eq!(calc.expression, "3");
    }

    // ----------------------------------------------------------------
    // Format tests
    // ----------------------------------------------------------------

    #[test]
    fn test_format_integer() {
        assert_eq!(format_result(42.0), "42");
    }

    #[test]
    fn test_format_decimal() {
        assert_eq!(format_result(3.25), "3.25");
    }

    #[test]
    fn test_format_negative() {
        assert_eq!(format_result(-7.0), "-7");
    }

    #[test]
    fn test_format_nan() {
        assert_eq!(format_result(f64::NAN), "NaN");
    }

    #[test]
    fn test_format_infinity() {
        assert_eq!(format_result(f64::INFINITY), "Infinity");
    }

    // ----------------------------------------------------------------
    // Window tests
    // ----------------------------------------------------------------

    /// Every key on screen, in the order it was drawn.
    fn keys_on_screen(ui: &CalculatorUi) -> Vec<&'static str> {
        let (w, h) = <CalculatorUi as Probe>::SIZE;
        ui.frame(w, h)
            .hits()
            .iter()
            .filter_map(|(target, _)| match target {
                Target::Key(label) => Some(*label),
                Target::HistoryRow(_) => None,
            })
            .collect()
    }

    #[test]
    fn the_standard_keypad_offers_every_key_it_names() {
        let ui = CalculatorUi::new();
        let keys = keys_on_screen(&ui);
        // The digits and the four operators are the whole point of the window;
        // naming them here means a row silently dropped from the layout fails
        // the test rather than merely looking wrong.
        for expected in [
            "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", ".", "+", "-", "*", "/", "=", "C",
            "CE", "MC", "MR", "M+", "M-", "MS", "Standard", "DEG", "Hist",
        ] {
            assert!(keys.contains(&expected), "no key for {expected}");
        }
        // Scientific keys must not be reachable in Standard mode: a hit box for
        // a key nobody can see is a key the user presses by accident.
        for absent in ["sin", "log", "n!", "pi"] {
            assert!(!keys.contains(&absent), "{absent} should not be on screen");
        }
    }

    #[test]
    fn switching_to_scientific_mode_brings_the_extra_rows_with_it() {
        let mut ui = CalculatorUi::new();
        assert_eq!(ui.calc.mode, CalcMode::Standard);
        assert_eq!(
            probe::click(&mut ui, Target::Key("Standard")),
            EventResult::Consumed
        );
        assert_eq!(ui.calc.mode, CalcMode::Scientific);

        let keys = keys_on_screen(&ui);
        for expected in [
            "sin", "cos", "tan", "asin", "acos", "atan", "ln", "log", "sqrt", "exp", "abs",
            "floor", "ceil", "n!", "x^y", "mod", "pi", "e", "(", ")",
        ] {
            assert!(keys.contains(&expected), "no key for {expected}");
        }
        // The digits did not go anywhere.
        assert!(keys.contains(&"7"));
        // The button now names the mode it would switch back to.
        assert!(keys.contains(&"Scientific"));
        assert!(!keys.contains(&"Standard"));
    }

    #[test]
    fn no_two_keys_share_a_pixel() {
        // A keypad is nothing but hit boxes, so two that overlap mean a key
        // that quietly presses its neighbour. Checked in the roomier of the two
        // modes, which is where the cells are tightest.
        let mut ui = CalculatorUi::new();
        ui.calc.mode = CalcMode::Scientific;
        ui.calc.show_history = true;
        let (w, h) = <CalculatorUi as Probe>::SIZE;
        let frame = ui.frame(w, h);
        let boxes: Vec<_> = frame.hits().to_vec();
        for (i, (a_target, a)) in boxes.iter().enumerate() {
            for (b_target, b) in boxes.iter().skip(i + 1) {
                assert!(
                    a.intersect(*b).is_none(),
                    "{a_target:?} at {a:?} overlaps {b_target:?} at {b:?}"
                );
            }
        }
    }

    #[test]
    fn a_number_can_be_added_up_entirely_by_clicking() {
        let mut ui = CalculatorUi::new();
        for label in ["1", "2", "+", "3", "0", "="] {
            assert_eq!(
                probe::click(&mut ui, Target::Key(label)),
                EventResult::Consumed,
                "no key for {label}"
            );
        }
        assert_eq!(ui.calc.display, "42");
    }

    #[test]
    fn a_click_on_no_key_at_all_changes_nothing() {
        let mut ui = CalculatorUi::new();
        ui.calc.expression = String::from("99");
        // The gap between the display panel and the first key row belongs to
        // nothing, and a window that treated it as a key press would insert
        // digits the user never asked for.
        let (w, h) = <CalculatorUi as Probe>::SIZE;
        assert_eq!(
            ui.click_at(1.0, 1.0, MouseButton::Left, (w, h)),
            EventResult::Ignored
        );
        assert_eq!(ui.calc.expression, "99");
    }

    #[test]
    fn the_display_shows_the_memory_only_once_something_is_in_it() {
        let mut ui = CalculatorUi::new();
        let (w, h) = <CalculatorUi as Probe>::SIZE;
        let quiet = ui.frame(w, h);
        assert!(
            !quiet
                .commands()
                .iter()
                .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.starts_with("M "))),
            "an empty memory should not be announced"
        );

        ui.calc.expression = String::from("7");
        probe::click(&mut ui, Target::Key("MS"));
        let loaded = ui.frame(w, h);
        assert!(
            loaded
                .commands()
                .iter()
                .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "M 7")),
            "a stored 7 should be readable without pressing MR"
        );
    }

    #[test]
    fn the_history_panel_takes_its_space_from_the_keypad_not_from_the_window() {
        let mut ui = CalculatorUi::new();
        let (w, h) = <CalculatorUi as Probe>::SIZE;
        let closed = Layout::new(w, h, CalcMode::Standard, false);
        let open = Layout::new(w, h, CalcMode::Standard, true);

        assert_eq!(closed.rows.len(), open.rows.len());
        let closed_last = closed.rows.last().copied().unwrap();
        let open_last = open.rows.last().copied().unwrap();
        assert!(
            open_last.h < closed_last.h,
            "opening history should shrink the keys, not move them off the window"
        );
        assert!(
            open_last.bottom() <= h,
            "the last key row ran off the bottom edge"
        );

        // And the keys stay clickable at their new size.
        ui.calc.show_history = true;
        assert!(probe::rect_of(&ui, Target::Key("=")).is_some());
    }

    #[test]
    fn a_history_row_can_be_clicked_to_get_its_answer_back() {
        let mut ui = CalculatorUi::new();
        for label in ["6", "*", "7", "="] {
            probe::click(&mut ui, Target::Key(label));
        }
        assert_eq!(ui.calc.display, "42");

        probe::click(&mut ui, Target::Key("Hist"));
        assert!(ui.calc.show_history);
        assert_eq!(
            probe::click(&mut ui, Target::HistoryRow(0)),
            EventResult::Consumed
        );
        assert_eq!(ui.calc.expression, "42");
    }

    #[test]
    fn a_history_row_that_records_an_error_is_not_worth_recalling() {
        let mut ui = CalculatorUi::new();
        ui.calc.show_history = true;
        ui.calc.history.push_front(HistoryEntry {
            expression: String::from("1 / 0"),
            result: String::from("Error: Division by zero"),
        });
        ui.calc.expression = String::from("5");
        // The click lands -- the row is there -- but pasting the words of an
        // error into the expression would only produce a second one.
        assert_eq!(
            probe::click(&mut ui, Target::HistoryRow(0)),
            EventResult::Ignored
        );
        assert_eq!(ui.calc.expression, "5");
    }

    #[test]
    fn an_empty_history_says_so_rather_than_showing_nothing() {
        let mut ui = CalculatorUi::new();
        ui.calc.show_history = true;
        let (w, h) = <CalculatorUi as Probe>::SIZE;
        let frame = ui.frame(w, h);
        assert!(
            frame
                .commands()
                .iter()
                .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "No history yet"))
        );
        assert!(
            frame
                .hits()
                .iter()
                .all(|(t, _)| !matches!(t, Target::HistoryRow(_)))
        );
    }

    #[test]
    fn a_scrolled_history_clicks_the_row_the_user_can_see() {
        let mut ui = CalculatorUi::new();
        ui.calc.show_history = true;
        for n in 0..20 {
            ui.calc.history.push_back(HistoryEntry {
                expression: format!("{n} + 0"),
                result: n.to_string(),
            });
        }
        let capacity = ui.history_capacity();
        assert!(capacity > 0 && capacity < 20, "capacity was {capacity}");

        // Nothing below the fold is clickable to begin with: the clip drops the
        // hit box along with the picture.
        assert!(probe::rect_of(&ui, Target::HistoryRow(19)).is_none());

        // One notch is three rows.
        assert!(ui.handle_scroll(-1.0));
        assert_eq!(ui.history_scroll, wheel::ROWS_PER_NOTCH as usize);
        assert!(probe::rect_of(&ui, Target::HistoryRow(0)).is_none());
        assert!(probe::rect_of(&ui, Target::HistoryRow(3)).is_some());

        // And the wheel stops at the last full page rather than scrolling the
        // list off the top of its own panel.
        for _ in 0..20 {
            ui.handle_scroll(-1.0);
        }
        assert_eq!(ui.history_scroll, 20 - capacity);
    }

    #[test]
    fn the_wheel_does_nothing_when_there_is_no_history_panel_to_scroll() {
        let mut ui = CalculatorUi::new();
        assert!(!ui.calc.show_history);
        assert!(!ui.handle_scroll(-3.0));
        assert_eq!(ui.history_scroll, 0);
    }

    #[test]
    fn reopening_the_history_panel_shows_the_newest_entries() {
        let mut ui = CalculatorUi::new();
        ui.calc.show_history = true;
        for n in 0..20 {
            ui.calc.history.push_back(HistoryEntry {
                expression: format!("{n} + 0"),
                result: n.to_string(),
            });
        }
        ui.handle_scroll(-2.0);
        assert!(ui.history_scroll > 0);

        probe::click(&mut ui, Target::Key("Hist"));
        assert!(!ui.calc.show_history);
        probe::click(&mut ui, Target::Key("Hist"));
        assert!(ui.calc.show_history);
        assert_eq!(ui.history_scroll, 0, "the panel reopened where it was left");
    }

    #[test]
    fn a_taller_window_gives_its_extra_room_to_the_keys() {
        let ui = CalculatorUi::new();
        let (w, h) = <CalculatorUi as Probe>::SIZE;
        let short = Layout::new(w, h, CalcMode::Standard, false);
        let tall = Layout::new(w, h * 2.0, CalcMode::Standard, false);

        // The display is a fixed band; the keys absorb the difference.
        assert_eq!(short.display.h, tall.display.h);
        assert!(tall.rows[0].h > short.rows[0].h);

        // And the hit boxes follow, which is the part `Probe::SIZE` cannot see:
        // `probe::rect_of` always draws at the declared size, so the frame is
        // asked directly here.
        let equals = ui
            .frame(w, h * 2.0)
            .rect_of(|t| *t == Target::Key("="))
            .expect("the equals key");
        assert!(equals.bottom() <= h * 2.0);
        assert!(equals.h > short.rows[0].h);
    }

    #[test]
    fn a_window_too_small_to_draw_in_offers_nothing_to_press() {
        // The frame does not clip to the window, so a layout that clamped its
        // bands to a minimum height would record keys below the bottom edge --
        // invisible, and pressable.
        let ui = CalculatorUi::new();
        let frame = ui.frame(4.0, 4.0);
        for (target, rect) in frame.hits() {
            assert!(
                rect.bottom() <= 4.0 + f32::EPSILON,
                "{target:?} at {rect:?} is outside a 4x4 window"
            );
        }
        assert!(frame.is_balanced(), "a clip was left open");
    }

    #[test]
    fn a_nonsense_window_size_is_laid_out_rather_than_propagated() {
        // A NaN width would otherwise flow into every rectangle, and NaN
        // compares false against everything -- so every hit test would miss and
        // the window would look dead rather than small.
        let ui = CalculatorUi::new();
        let frame = ui.frame(f32::NAN, -10.0);
        assert!(
            frame
                .hits()
                .iter()
                .all(|(_, r)| r.x.is_finite() && r.y.is_finite())
        );
        assert!(frame.is_balanced());
    }

    #[test]
    fn the_keyboard_reaches_what_the_keypad_does() {
        let mut ui = CalculatorUi::new();
        probe::type_str(&mut ui, "6*7");
        assert_eq!(ui.calc.expression, "6 * 7");
        assert_eq!(
            probe::key(&mut ui, &probe::press(Key::Enter)),
            EventResult::Consumed
        );
        assert_eq!(ui.calc.display, "42");

        assert_eq!(
            probe::key(&mut ui, &probe::press(Key::Escape)),
            EventResult::Consumed
        );
        assert_eq!(ui.calc.display, "0");
    }

    #[test]
    fn a_keystroke_that_names_no_key_is_left_for_someone_else() {
        let mut ui = CalculatorUi::new();
        // Not a calculator key: reporting it consumed would stop the desktop
        // from ever seeing it.
        assert_eq!(
            probe::key(&mut ui, &probe::press(Key::F1)),
            EventResult::Ignored
        );
        assert_eq!(ui.calc.expression, "");
    }

    #[test]
    fn a_key_release_does_nothing() {
        let mut ui = CalculatorUi::new();
        let mut release = probe::press(Key::Num5);
        release.pressed = false;
        assert_eq!(probe::key(&mut ui, &release), EventResult::Ignored);
        assert_eq!(ui.calc.expression, "");
    }

    #[test]
    fn the_window_redraws_when_it_is_resized() {
        let mut ui = CalculatorUi::new();
        assert_eq!(
            ui.on_event(&Event::Resize {
                width: 500,
                height: 700
            }),
            Response::Redraw
        );
        // The recorded size is what `target_at` consults while the frame is
        // drawn at the size it is handed. Finding the equals key in a frame
        // drawn at the new size and then pressing its centre through
        // `target_at` is what proves the two agree -- before the resize that
        // point is well below the window and hits nothing.
        let equals = ui
            .frame(500.0, 700.0)
            .rect_of(|t| *t == Target::Key("="))
            .expect("the equals key");
        let (cx, cy) = equals.centre();
        assert!(
            cy > WINDOW_HEIGHT,
            "the key did not move down with the window"
        );
        assert_eq!(ui.target_at(cx, cy), Some(Target::Key("=")));
    }

    #[test]
    fn closing_the_window_exits() {
        let mut ui = CalculatorUi::new();
        assert_eq!(ui.on_event(&Event::CloseRequested), Response::Exit);
    }

    // ----------------------------------------------------------------
    // Keyboard handling tests
    // ----------------------------------------------------------------

    #[test]
    fn test_key_digit() {
        let mut calc = Calculator::new();
        let key = KeyEvent {
            key: Key::Num5,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: "5".to_string(),
        };
        assert!(handle_key(&mut calc, &key));
        assert_eq!(calc.expression, "5");
    }

    #[test]
    fn test_key_enter() {
        let mut calc = Calculator::new();
        calc.input_digit('7');
        let key = KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        assert!(handle_key(&mut calc, &key));
        assert!(calc.showing_result);
    }

    #[test]
    fn test_key_escape() {
        let mut calc = Calculator::new();
        calc.input_digit('9');
        let key = KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        assert!(handle_key(&mut calc, &key));
        assert_eq!(calc.display, "0");
    }

    #[test]
    fn test_key_backspace() {
        let mut calc = Calculator::new();
        calc.input_digit('4');
        calc.input_digit('2');
        let key = KeyEvent {
            key: Key::Backspace,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        assert!(handle_key(&mut calc, &key));
        assert_eq!(calc.expression, "4");
    }

    #[test]
    fn test_key_release_ignored() {
        let mut calc = Calculator::new();
        let key = KeyEvent {
            key: Key::Num5,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: "5".to_string(),
        };
        assert!(!handle_key(&mut calc, &key));
        assert_eq!(calc.expression, ""); // Not modified.
    }

    // ----------------------------------------------------------------
    // Edge cases
    // ----------------------------------------------------------------

    #[test]
    fn test_empty_expression() {
        let result = evaluate("", AngleUnit::Degrees);
        assert_eq!(result, Err("Empty expression"));
    }

    #[test]
    fn test_whitespace_only() {
        let result = evaluate("   ", AngleUnit::Degrees);
        assert_eq!(result, Err("Empty expression"));
    }

    #[test]
    fn test_mismatched_parens() {
        let result = evaluate("(2 + 3", AngleUnit::Degrees);
        assert_eq!(result, Err("Missing ')'"));
    }

    #[test]
    fn test_extra_close_paren() {
        let result = evaluate("2 + 3)", AngleUnit::Degrees);
        assert_eq!(result, Err("Unexpected token"));
    }

    #[test]
    fn test_consecutive_operators() {
        // "2 + * 3" is invalid because * after + is not a valid unary.
        let result = evaluate("2 + * 3", AngleUnit::Degrees);
        assert_eq!(result, Err("Unexpected token"));
    }

    #[test]
    fn test_just_a_number() {
        let result = evaluate("42", AngleUnit::Degrees);
        assert_eq!(result, Ok(42.0));
    }

    #[test]
    fn test_complex_expression() {
        // (2 + 3) * 4 - 10 / 2 = 20 - 5 = 15
        let result = evaluate("(2 + 3) * 4 - 10 / 2", AngleUnit::Degrees);
        assert_eq!(result, Ok(15.0));
    }

    #[test]
    fn test_scientific_notation_expression() {
        // sin(30) in degrees = 0.5
        let result = evaluate("sin(30)", AngleUnit::Degrees);
        assert!((result.expect("should succeed") - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_handle_button_digit() {
        let mut calc = Calculator::new();
        handle_button(&mut calc, "5");
        assert_eq!(calc.expression, "5");
    }

    #[test]
    fn test_handle_button_equals() {
        let mut calc = Calculator::new();
        calc.expression = String::from("2 + 3");
        handle_button(&mut calc, "=");
        assert_eq!(calc.display, "5");
    }

    #[test]
    fn test_handle_button_clear() {
        let mut calc = Calculator::new();
        calc.expression = String::from("123");
        handle_button(&mut calc, "C");
        assert_eq!(calc.display, "0");
    }

    #[test]
    fn test_handle_button_function() {
        let mut calc = Calculator::new();
        handle_button(&mut calc, "sin");
        assert_eq!(calc.expression, "sin(");
    }

    #[test]
    fn test_handle_button_mode_toggle() {
        let mut calc = Calculator::new();
        assert_eq!(calc.mode, CalcMode::Standard);
        handle_button(&mut calc, "Standard");
        assert_eq!(calc.mode, CalcMode::Scientific);
    }

    #[test]
    fn test_percent() {
        let mut calc = Calculator::new();
        calc.expression = String::from("200");
        calc.input_percent();
        assert_eq!(calc.display, "2");
    }
}
