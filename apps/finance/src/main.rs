#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_lossless)]

//! Slate OS Personal Finance — budget tracking and expense management.
//!
//! Track income and expenses across categories, set budgets, view spending
//! trends, manage accounts, and get financial summaries.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

// ── Catppuccin Mocha palette ────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const SKY: Color = Color::from_hex(0x89DCEB);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── Category ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Category {
    Food,
    Housing,
    Transportation,
    Utilities,
    Healthcare,
    Entertainment,
    Shopping,
    Education,
    Savings,
    Income,
    Investment,
    Other,
}

impl Category {
    const ALL: [Self; 12] = [
        Self::Food,
        Self::Housing,
        Self::Transportation,
        Self::Utilities,
        Self::Healthcare,
        Self::Entertainment,
        Self::Shopping,
        Self::Education,
        Self::Savings,
        Self::Income,
        Self::Investment,
        Self::Other,
    ];

    const EXPENSE_CATS: [Self; 9] = [
        Self::Food,
        Self::Housing,
        Self::Transportation,
        Self::Utilities,
        Self::Healthcare,
        Self::Entertainment,
        Self::Shopping,
        Self::Education,
        Self::Other,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Food => "Food & Dining",
            Self::Housing => "Housing",
            Self::Transportation => "Transport",
            Self::Utilities => "Utilities",
            Self::Healthcare => "Healthcare",
            Self::Entertainment => "Entertainment",
            Self::Shopping => "Shopping",
            Self::Education => "Education",
            Self::Savings => "Savings",
            Self::Income => "Income",
            Self::Investment => "Investment",
            Self::Other => "Other",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Food => "\u{1F354}",
            Self::Housing => "\u{1F3E0}",
            Self::Transportation => "\u{1F697}",
            Self::Utilities => "\u{26A1}",
            Self::Healthcare => "\u{1FA7A}",
            Self::Entertainment => "\u{1F3AC}",
            Self::Shopping => "\u{1F6CD}",
            Self::Education => "\u{1F4DA}",
            Self::Savings => "\u{1F4B0}",
            Self::Income => "\u{1F4B5}",
            Self::Investment => "\u{1F4C8}",
            Self::Other => "\u{1F4CB}",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Food => PEACH,
            Self::Housing => BLUE,
            Self::Transportation => SKY,
            Self::Utilities => YELLOW,
            Self::Healthcare => RED,
            Self::Entertainment => MAUVE,
            Self::Shopping => LAVENDER,
            Self::Education => TEAL,
            Self::Savings => GREEN,
            Self::Income => GREEN,
            Self::Investment => BLUE,
            Self::Other => OVERLAY0,
        }
    }
}

// ── Date ────────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
struct SimpleDate {
    year: u16,
    month: u8,
    day: u8,
}

impl SimpleDate {
    fn new(year: u16, month: u8, day: u8) -> Self {
        Self { year, month, day }
    }

    fn format(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    fn month_label(&self) -> &'static str {
        match self.month {
            1 => "January",
            2 => "February",
            3 => "March",
            4 => "April",
            5 => "May",
            6 => "June",
            7 => "July",
            8 => "August",
            9 => "September",
            10 => "October",
            11 => "November",
            12 => "December",
            _ => "Unknown",
        }
    }

    fn same_month(&self, other: &Self) -> bool {
        self.year == other.year && self.month == other.month
    }

    /// The first of the previous month.
    ///
    /// Stops at January of year 0 rather than wrapping to year 65535: a
    /// calendar that runs backwards past its own start is worse than one that
    /// refuses to.
    fn prev_month(self) -> Self {
        match self.month.checked_sub(1) {
            Some(m) if m >= 1 => Self::new(self.year, m, 1),
            _ => match self.year.checked_sub(1) {
                Some(y) => Self::new(y, 12, 1),
                None => Self::new(0, 1, 1),
            },
        }
    }

    /// The first of the next month, saturating at December of year 65535.
    fn next_month(self) -> Self {
        match self.month.checked_add(1) {
            Some(m) if m <= 12 => Self::new(self.year, m, 1),
            _ => match self.year.checked_add(1) {
                Some(y) => Self::new(y, 1, 1),
                None => Self::new(u16::MAX, 12, 1),
            },
        }
    }
}

// ── Transaction ─────────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct Transaction {
    id: u32,
    date: SimpleDate,
    description: String,
    amount: i64, // cents (positive=income, negative=expense)
    category: Category,
    account_id: u32,
    notes: String,
    recurring: bool,
}

impl Transaction {
    fn is_income(&self) -> bool {
        self.amount > 0
    }

    fn is_expense(&self) -> bool {
        self.amount < 0
    }
}

// ── Account ─────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct Account {
    id: u32,
    name: String,
    account_type: AccountType,
    initial_balance: i64, // cents
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AccountType {
    Checking,
    Savings,
    CreditCard,
    Cash,
    /// Nothing constructs this, and that is a statement about the app rather
    /// than about the variant: there is no account-creation UI at all, so every
    /// account in existence comes from `create_sample_data`, and the sample set
    /// happens to cover the other four. Deleting it would encode "the sample
    /// data has no brokerage account" as "SlateOS has no such account type".
    /// See known-issues.md -> TD-C-FINANCE-IS-A-VIEWER-OVER-SAMPLE-DATA.
    #[allow(dead_code, reason = "a model variant awaiting the creation UI")]
    Investment,
}

impl AccountType {
    fn label(self) -> &'static str {
        match self {
            Self::Checking => "Checking",
            Self::Savings => "Savings",
            Self::CreditCard => "Credit Card",
            Self::Cash => "Cash",
            Self::Investment => "Investment",
        }
    }
}

// ── Budget ──────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct Budget {
    category: Category,
    monthly_limit: i64, // cents (positive)
}

// ── View / screen ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Screen {
    Dashboard,
    Transactions,
    Budgets,
    Accounts,
    Reports,
}

impl Screen {
    const ALL: [Self; 5] = [
        Self::Dashboard,
        Self::Transactions,
        Self::Budgets,
        Self::Accounts,
        Self::Reports,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Dashboard => "Dashboard",
            Self::Transactions => "Transactions",
            Self::Budgets => "Budgets",
            Self::Accounts => "Accounts",
            Self::Reports => "Reports",
        }
    }
}

// ── App ─────────────────────────────────────────────────────────────
struct FinanceApp {
    width: f32,
    height: f32,
    screen: Screen,
    transactions: Vec<Transaction>,
    accounts: Vec<Account>,
    budgets: Vec<Budget>,
    next_tx_id: u32,
    next_account_id: u32,
    current_date: SimpleDate,
    view_month: SimpleDate, // first day of the month being viewed
    /// The selected transaction's **id**, not its index.
    ///
    /// It was an index, and `delete_transaction` calls `Vec::remove` — so every
    /// deletion silently re-pointed the selection at whatever slid into the
    /// gap. `CLAUDE.md` names this directly: store stable identifiers, not
    /// positions into a container that moves. `None` means nothing is selected,
    /// which is the honest state for an empty or fully-filtered-out list.
    selected_id: Option<u32>,
    search_query: String,
    search_active: bool,
    category_filter: Option<Category>,
    status_msg: String,
}

impl FinanceApp {
    fn new() -> Self {
        let today = SimpleDate::new(2026, 5, 18);
        let mut app = Self {
            width: 1100.0,
            height: 750.0,
            screen: Screen::Dashboard,
            transactions: Vec::new(),
            accounts: Vec::new(),
            budgets: Vec::new(),
            next_tx_id: 1,
            next_account_id: 1,
            current_date: today,
            view_month: SimpleDate::new(today.year, today.month, 1),
            selected_id: None,
            search_query: String::new(),
            search_active: false,
            category_filter: None,
            status_msg: String::from("Personal Finance"),
        };
        app.create_sample_data();
        app
    }

    fn create_sample_data(&mut self) {
        // Accounts
        let checking_id = self.add_account("Main Checking", AccountType::Checking, 350_000);
        let savings_id = self.add_account("Savings", AccountType::Savings, 1_200_000);
        let credit_id = self.add_account("Credit Card", AccountType::CreditCard, 0);
        let _cash_id = self.add_account("Cash", AccountType::Cash, 15_000);

        // Budgets
        self.set_budget(Category::Food, 60_000);
        self.set_budget(Category::Housing, 150_000);
        self.set_budget(Category::Transportation, 30_000);
        self.set_budget(Category::Utilities, 20_000);
        self.set_budget(Category::Entertainment, 15_000);
        self.set_budget(Category::Shopping, 25_000);
        self.set_budget(Category::Healthcare, 10_000);

        // Sample transactions for May 2026
        let may = |day: u8| SimpleDate::new(2026, 5, day);
        self.add_transaction(
            may(1),
            "Monthly Salary",
            500_000,
            Category::Income,
            checking_id,
            "",
            false,
        );
        self.add_transaction(
            may(1),
            "Rent Payment",
            -150_000,
            Category::Housing,
            checking_id,
            "Monthly rent",
            true,
        );
        self.add_transaction(
            may(2),
            "Grocery Store",
            -8_500,
            Category::Food,
            credit_id,
            "",
            false,
        );
        self.add_transaction(
            may(3),
            "Electric Bill",
            -9_500,
            Category::Utilities,
            checking_id,
            "",
            true,
        );
        self.add_transaction(
            may(4),
            "Coffee Shop",
            -550,
            Category::Food,
            credit_id,
            "",
            false,
        );
        self.add_transaction(
            may(5),
            "Gas Station",
            -4_500,
            Category::Transportation,
            credit_id,
            "",
            false,
        );
        self.add_transaction(
            may(6),
            "Netflix",
            -1_599,
            Category::Entertainment,
            credit_id,
            "Monthly sub",
            true,
        );
        self.add_transaction(
            may(7),
            "Restaurant Dinner",
            -6_200,
            Category::Food,
            credit_id,
            "",
            false,
        );
        self.add_transaction(
            may(8),
            "Pharmacy",
            -2_300,
            Category::Healthcare,
            credit_id,
            "",
            false,
        );
        self.add_transaction(
            may(9),
            "Online Shopping",
            -4_999,
            Category::Shopping,
            credit_id,
            "",
            false,
        );
        self.add_transaction(
            may(10),
            "Transfer to Savings",
            -50_000,
            Category::Savings,
            checking_id,
            "",
            true,
        );
        self.add_transaction(
            may(10),
            "Savings Deposit",
            50_000,
            Category::Savings,
            savings_id,
            "",
            false,
        );
        self.add_transaction(
            may(11),
            "Lunch",
            -1_200,
            Category::Food,
            credit_id,
            "",
            false,
        );
        self.add_transaction(
            may(12),
            "Book Purchase",
            -2_499,
            Category::Education,
            credit_id,
            "",
            false,
        );
        self.add_transaction(
            may(13),
            "Internet Bill",
            -7_999,
            Category::Utilities,
            checking_id,
            "",
            true,
        );
        self.add_transaction(
            may(14),
            "Grocery Store",
            -11_200,
            Category::Food,
            credit_id,
            "Weekly groceries",
            false,
        );
        self.add_transaction(
            may(15),
            "Freelance Payment",
            75_000,
            Category::Income,
            checking_id,
            "Web project",
            false,
        );
        self.add_transaction(
            may(16),
            "Movie Tickets",
            -3_000,
            Category::Entertainment,
            credit_id,
            "",
            false,
        );
        self.add_transaction(
            may(17),
            "Public Transit",
            -276,
            Category::Transportation,
            credit_id,
            "Bus fare",
            false,
        );
    }

    fn add_account(&mut self, name: &str, atype: AccountType, initial: i64) -> u32 {
        let id = self.next_account_id;
        // Saturating rather than wrapping: a wrapped counter hands out an id
        // that already exists, and selection and deletion are both by id.
        self.next_account_id = self.next_account_id.saturating_add(1);
        self.accounts.push(Account {
            id,
            name: name.to_string(),
            account_type: atype,
            initial_balance: initial,
        });
        id
    }

    // A transaction is defined by its date, description, amount, category,
    // owning account, note, and recurring flag; these are independent scalar
    // fields with no natural grouping, so they are passed positionally.
    #[allow(clippy::too_many_arguments)]
    fn add_transaction(
        &mut self,
        date: SimpleDate,
        desc: &str,
        amount: i64,
        category: Category,
        account_id: u32,
        notes: &str,
        recurring: bool,
    ) -> u32 {
        let id = self.next_tx_id;
        self.next_tx_id = self.next_tx_id.saturating_add(1);
        self.transactions.push(Transaction {
            id,
            date,
            description: desc.to_string(),
            amount,
            category,
            account_id,
            notes: notes.to_string(),
            recurring,
        });
        id
    }

    /// Delete by id, and leave the selection on a row that still exists.
    fn delete_transaction(&mut self, id: u32) {
        let Some(idx) = self.transactions.iter().position(|tx| tx.id == id) else {
            return;
        };
        self.transactions.remove(idx);
        // Prefer the row that took its place, then the one before it; the point
        // is that repeated deletes walk down the list rather than jumping to
        // the end or landing on nothing.
        let visible = self.visible_ids();
        self.selected_id = visible
            .get(idx)
            .or_else(|| visible.get(idx.saturating_sub(1)))
            .or_else(|| visible.first())
            .copied();
        self.status_msg = String::from("Transaction deleted");
    }

    /// The ids of the transactions currently on screen, in screen order.
    fn visible_ids(&self) -> Vec<u32> {
        self.filtered_transactions()
            .iter()
            .map(|(_, tx)| tx.id)
            .collect()
    }

    /// Move the selection by `delta` rows **through the list on screen**.
    ///
    /// It used to step through `transactions` by index while the screen showed
    /// `filtered_transactions()`, so with a filter or a search active the arrow
    /// keys walked through hidden rows: the highlight vanished for several
    /// presses, and Ctrl+D then deleted a row the user could not see.
    fn move_selection(&mut self, delta: isize) {
        let visible = self.visible_ids();
        if visible.is_empty() {
            self.selected_id = None;
            return;
        }
        let current = self
            .selected_id
            .and_then(|id| visible.iter().position(|&v| v == id));
        let next = match current {
            // Not on screen (the filter just changed under it): the first
            // visible row is where the selection belongs, whichever way the
            // user pressed.
            None => 0,
            Some(pos) => {
                let Ok(pos) = isize::try_from(pos) else {
                    return;
                };
                let Some(moved) = pos.checked_add(delta) else {
                    return;
                };
                let Ok(moved) = usize::try_from(moved) else {
                    return; // stepped off the top; stay put
                };
                if moved >= visible.len() {
                    return; // stepped off the bottom; stay put
                }
                moved
            }
        };
        self.selected_id = visible.get(next).copied();
    }

    /// Put the selection back on a visible row after the view changed.
    ///
    /// Changing a filter, a search or the month can hide whatever was selected.
    /// Leaving it hidden is the state that made the highlight disappear.
    fn reanchor_selection(&mut self) {
        let visible = self.visible_ids();
        if !self.selected_id.is_some_and(|id| visible.contains(&id)) {
            self.selected_id = visible.first().copied();
        }
    }

    fn set_budget(&mut self, category: Category, monthly_limit: i64) {
        if let Some(b) = self.budgets.iter_mut().find(|b| b.category == category) {
            b.monthly_limit = monthly_limit;
        } else {
            self.budgets.push(Budget {
                category,
                monthly_limit,
            });
        }
    }

    // ── Queries ─────────────────────────────────────────────────────
    fn month_transactions(&self) -> Vec<&Transaction> {
        self.transactions
            .iter()
            .filter(|tx| tx.date.same_month(&self.view_month))
            .collect()
    }

    fn month_income(&self) -> i64 {
        self.month_transactions()
            .iter()
            .filter(|tx| tx.is_income() && !matches!(tx.category, Category::Savings))
            .map(|tx| tx.amount)
            .sum()
    }

    fn month_expenses(&self) -> i64 {
        self.month_transactions()
            .iter()
            .filter(|tx| tx.is_expense() && !matches!(tx.category, Category::Savings))
            .map(|tx| tx.amount.abs())
            .sum()
    }

    fn month_savings(&self) -> i64 {
        // Saturating, not wrapping: i64 cents is ~92 quadrillion dollars, so
        // the only way to reach the edge is corrupt data — and a wrap there
        // would report a huge surplus as a huge deficit.
        self.month_income().saturating_sub(self.month_expenses())
    }

    fn category_spending(&self, cat: Category) -> i64 {
        self.month_transactions()
            .iter()
            .filter(|tx| tx.category == cat && tx.is_expense())
            .map(|tx| tx.amount.abs())
            .sum()
    }

    /// Fraction of a monthly limit already spent.
    ///
    /// The dashboard and the budgets screen both draw this bar and each used to
    /// divide for itself. Two copies of a division are two chances to disagree
    /// about what a limit of zero means — and they did: one returned `0.0`, the
    /// other divided by it.
    fn usage_ratio(spent: i64, monthly_limit: i64) -> f32 {
        if monthly_limit <= 0 {
            // An unset limit is not "infinitely overspent"; it is a budget
            // nobody has set, and its bar should read empty rather than red.
            return 0.0;
        }
        #[allow(
            clippy::cast_precision_loss,
            reason = "a ratio for a progress bar; cents beyond f32's exact range \
                      are past any plausible household budget and the bar is \
                      clamped for drawing anyway"
        )]
        {
            spent as f32 / monthly_limit as f32
        }
    }

    fn account_balance(&self, account_id: u32) -> i64 {
        let initial = self
            .accounts
            .iter()
            .find(|a| a.id == account_id)
            .map_or(0, |a| a.initial_balance);
        let tx_sum: i64 = self
            .transactions
            .iter()
            .filter(|tx| tx.account_id == account_id)
            .map(|tx| tx.amount)
            .sum();
        initial.saturating_add(tx_sum)
    }

    fn total_balance(&self) -> i64 {
        self.accounts
            .iter()
            .map(|a| self.account_balance(a.id))
            .sum()
    }

    fn filtered_transactions(&self) -> Vec<(usize, &Transaction)> {
        self.transactions
            .iter()
            .enumerate()
            .filter(|(_, tx)| {
                if let Some(cat) = self.category_filter
                    && tx.category != cat
                {
                    return false;
                }
                if !self.search_query.is_empty() {
                    let q = self.search_query.to_ascii_lowercase();
                    if !tx.description.to_ascii_lowercase().contains(&q)
                        && !tx.notes.to_ascii_lowercase().contains(&q)
                    {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    fn top_expense_categories(&self) -> Vec<(Category, i64)> {
        let mut cats: Vec<(Category, i64)> = Category::EXPENSE_CATS
            .iter()
            .map(|&c| (c, self.category_spending(c)))
            .filter(|(_, amt)| *amt > 0)
            .collect();
        cats.sort_by_key(|c| std::cmp::Reverse(c.1));
        cats
    }

    // ── Key handling ────────────────────────────────────────────────
    fn handle_key(&mut self, key: &str, ctrl: bool, _shift: bool) {
        if self.search_active {
            match key {
                "Escape" => {
                    self.search_active = false;
                    self.search_query.clear();
                    self.reanchor_selection();
                }
                "Backspace" => {
                    self.search_query.pop();
                    self.reanchor_selection();
                }
                _ => {}
            }
            return;
        }
        match key {
            "1" => self.screen = Screen::Dashboard,
            "2" => self.screen = Screen::Transactions,
            "3" => self.screen = Screen::Budgets,
            "4" => self.screen = Screen::Accounts,
            "5" => self.screen = Screen::Reports,
            "Left" => {
                self.view_month = self.view_month.prev_month();
                self.reanchor_selection();
            }
            "Right" => {
                self.view_month = self.view_month.next_month();
                self.reanchor_selection();
            }
            // Back to the month containing today, which is otherwise reachable
            // only by counting arrow presses.
            "Home" => {
                self.view_month =
                    SimpleDate::new(self.current_date.year, self.current_date.month, 1);
                self.reanchor_selection();
            }
            "Up" | "k" => self.move_selection(-1),
            "Down" | "j" => self.move_selection(1),
            "/" => {
                self.search_active = true;
                self.search_query.clear();
            }
            "c" => {
                // Cycle category filter
                // `get` on the next position rather than an index guarded by a
                // separate length test: running off the end is how the cycle
                // returns to "All", so it is the normal path and not an error.
                self.category_filter = match self.category_filter {
                    None => Category::ALL.first().copied(),
                    Some(cat) => Category::ALL
                        .iter()
                        .position(|&c| c == cat)
                        .and_then(|idx| idx.checked_add(1))
                        .and_then(|next| Category::ALL.get(next))
                        .copied(),
                };
                if let Some(cat) = self.category_filter {
                    self.status_msg = format!("Filter: {}", cat.label());
                } else {
                    self.status_msg = String::from("Filter: All");
                }
                self.reanchor_selection();
            }
            "Delete" | "d" if ctrl => {
                if let Some(id) = self.selected_id {
                    self.delete_transaction(id);
                }
            }
            _ => {}
        }
    }

    fn handle_search_text(&mut self, text: &str) {
        if self.search_active {
            self.search_query.push_str(text);
            self.reanchor_selection();
        }
    }

    fn format_currency(cents: i64) -> String {
        let sign = if cents < 0 { "-" } else { "" };
        let abs = cents.unsigned_abs();
        let dollars = abs / 100;
        let remainder = abs % 100;
        format!("{sign}${dollars}.{remainder:02}")
    }

    fn format_currency_colored(cents: i64) -> (String, Color) {
        let text = Self::format_currency(cents);
        let color = match cents.cmp(&0) {
            std::cmp::Ordering::Greater => GREEN,
            std::cmp::Ordering::Less => RED,
            std::cmp::Ordering::Equal => TEXT_COLOR,
        };
        (text, color)
    }

    // ── Layout ──────────────────────────────────────────────────────
    const SIDEBAR_W: f32 = 180.0;
    const HEADER_H: f32 = 50.0;
    const STATUS_H: f32 = 28.0;

    fn content_x(&self) -> f32 {
        Self::SIDEBAR_W
    }
    fn content_w(&self) -> f32 {
        (self.width - Self::SIDEBAR_W).max(100.0)
    }
    fn content_y(&self) -> f32 {
        Self::HEADER_H
    }
    fn content_h(&self) -> f32 {
        (self.height - Self::HEADER_H - Self::STATUS_H).max(100.0)
    }

    /// The y below which a row is off the bottom of the content area.
    ///
    /// The row loops each open-coded `self.height - STATUS_H`, which agrees
    /// with `content_h` at ordinary sizes and disagrees at tiny ones, where
    /// `content_h`'s floor applies and the open-coded form does not — so a very
    /// short window drew nothing at all rather than a clipped first row.
    fn content_bottom(&self) -> f32 {
        self.content_y() + self.content_h()
    }

    // ── Events ──────────────────────────────────────────────────────

    /// Route a compositor event into the app.
    fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key_ev) => self.handle_key_event(key_ev),
            Event::Resize { width, height } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a window dimension is far below f32's integer-exact range"
                )]
                {
                    self.width = *width as f32;
                    self.height = *height as f32;
                }
                // Deliberately not `Consumed`: a resize is not a reason to
                // redraw by itself. The compositor asks for the frame it wants.
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    /// Translate a key event and apply it.
    ///
    /// Text goes to the search box first, because a search for `1` must not be
    /// read as "switch to the dashboard".
    fn handle_key_event(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }
        let ctrl = key.modifiers.ctrl;
        let shift = key.modifiers.shift;

        // While searching, a typed character is search text and not a shortcut:
        // a search for "1" must not be read as "switch to the dashboard".
        if self.search_active && !ctrl && !key.text.is_empty() {
            self.handle_search_text(&key.text);
            return EventResult::Consumed;
        }

        let Some(name) = Self::key_name(key) else {
            return EventResult::Ignored;
        };
        let before = self.state_fingerprint();
        self.handle_key(&name, ctrl, shift);
        if self.state_fingerprint() == before {
            EventResult::Ignored
        } else {
            EventResult::Consumed
        }
    }

    /// The name `handle_key` knows a key by.
    ///
    /// `handle_key` matches on strings and its tests call it that way, so this
    /// is the one place the compositor's `Key` becomes one of those names —
    /// rather than duplicating the whole key table in a second form.
    fn key_name(key: &KeyEvent) -> Option<String> {
        let named = match key.key {
            Key::Up => "Up",
            Key::Down => "Down",
            Key::Left => "Left",
            Key::Right => "Right",
            Key::Escape => "Escape",
            Key::Backspace => "Backspace",
            Key::Delete => "Delete",
            Key::Enter => "Return",
            Key::Tab => "Tab",
            _ => {
                // Everything else is only interesting as the character typed,
                // which is how the shortcuts below are written.
                let typed = key.text.chars().next()?;
                return Some(typed.to_string());
            }
        };
        Some(named.to_string())
    }

    /// A cheap summary of everything a keystroke can change.
    ///
    /// `handle_key` reports nothing about whether it did anything, and an app
    /// that answers `Consumed` to every key redraws on keys it ignored. Rather
    /// than have every arm of that match remember to report, this compares the
    /// state around the call. It is a tuple of small copies, not a hash: a
    /// hash could collide and silently drop a redraw.
    fn state_fingerprint(&self) -> (Screen, Option<u32>, bool, usize, Option<Category>, u16, u8) {
        (
            self.screen,
            self.selected_id,
            self.search_active,
            self.transactions.len(),
            self.category_filter,
            self.view_month.year,
            self.view_month.month,
        )
    }

    // ── Rendering ───────────────────────────────────────────────────

    /// Named `render_commands` and not `render`: at equal arity an inherent
    /// method silently wins method lookup over `oswindow::app::App::render`,
    /// so an app that keeps the name draws nothing and says nothing about it.
    fn render_commands(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(512);

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_sidebar(&mut cmds);
        self.render_header(&mut cmds);

        match self.screen {
            Screen::Dashboard => self.render_dashboard(&mut cmds),
            Screen::Transactions => self.render_transactions(&mut cmds),
            Screen::Budgets => self.render_budgets(&mut cmds),
            Screen::Accounts => self.render_accounts(&mut cmds),
            Screen::Reports => self.render_reports(&mut cmds),
        }

        self.render_status(&mut cmds);
        cmds
    }

    fn render_sidebar(&self, cmds: &mut Vec<RenderCommand>) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: Self::SIDEBAR_W,
            height: self.height,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Logo
        cmds.push(RenderCommand::Text {
            x: 16.0,
            y: 16.0,
            text: String::from("\u{1F4B0} Finance"),
            font_size: 18.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(Self::SIDEBAR_W - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Nav items
        let nav_y = 52.0;
        for (i, screen) in Screen::ALL.iter().enumerate() {
            let iy = nav_y + i as f32 * 40.0;
            let is_active = *screen == self.screen;
            let bg = if is_active {
                SURFACE1
            } else {
                Color::rgba(0, 0, 0, 0)
            };
            let tc = if is_active { BLUE } else { SUBTEXT0 };

            cmds.push(RenderCommand::FillRect {
                x: 8.0,
                y: iy,
                width: Self::SIDEBAR_W - 16.0,
                height: 36.0,
                color: bg,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: 20.0,
                y: iy + 9.0,
                text: format!("{} {}", i.saturating_add(1), screen.label()),
                font_size: 13.0,
                color: tc,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(Self::SIDEBAR_W - 40.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Quick stats at bottom
        let total = self.total_balance();
        let (total_str, total_color) = Self::format_currency_colored(total);
        cmds.push(RenderCommand::Text {
            x: 16.0,
            y: self.height - 60.0,
            text: String::from("Total Balance"),
            font_size: 11.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(Self::SIDEBAR_W - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: 16.0,
            y: self.height - 42.0,
            text: total_str,
            font_size: 18.0,
            color: total_color,
            font_weight: FontWeightHint::Bold,
            max_width: Some(Self::SIDEBAR_W - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_header(&self, cmds: &mut Vec<RenderCommand>) {
        cmds.push(RenderCommand::FillRect {
            x: Self::SIDEBAR_W,
            y: 0.0,
            width: self.content_w(),
            height: Self::HEADER_H,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Month navigation
        cmds.push(RenderCommand::Text {
            x: Self::SIDEBAR_W + 16.0,
            y: 14.0,
            text: format!(
                "\u{25C0} {} {} \u{25B6}",
                self.view_month.month_label(),
                self.view_month.year
            ),
            font_size: 18.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Monthly summary in header
        let income = self.month_income();
        let expenses = self.month_expenses();
        let savings = self.month_savings();
        let hx = self.width - 460.0;
        for (label, amount, color, offset) in [
            ("Income", income, GREEN, 0.0_f32),
            ("Expenses", expenses, RED, 150.0),
            (
                "Savings",
                savings,
                if savings >= 0 { TEAL } else { RED },
                300.0,
            ),
        ] {
            cmds.push(RenderCommand::Text {
                x: hx + offset,
                y: 6.0,
                text: label.to_string(),
                font_size: 10.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(80.0),
                overflow: TextOverflow::Ellipsis,
            });
            // Expenses are accumulated as a positive magnitude; render them as
            // a money-out (negative) figure so the header reads "-$X.XX".
            let val = if label == "Expenses" {
                Self::format_currency(amount.saturating_neg())
            } else {
                Self::format_currency(amount)
            };
            cmds.push(RenderCommand::Text {
                x: hx + offset,
                y: 22.0,
                text: val,
                font_size: 16.0,
                color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(140.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_dashboard(&self, cmds: &mut Vec<RenderCommand>) {
        let cx = self.content_x() + 16.0;
        let cy = self.content_y() + 16.0;
        let cw = self.content_w() - 32.0;

        // Budget overview cards
        cmds.push(RenderCommand::Text {
            x: cx,
            y: cy,
            text: String::from("Budget Overview"),
            font_size: 16.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        let card_w = (cw - 16.0) / 2.0;
        let card_h = 60.0;
        let mut card_y = cy + 28.0;

        for (i, budget) in self.budgets.iter().enumerate() {
            let col = i % 2;
            let card_x = cx + col as f32 * (card_w + 16.0);
            if col == 0 && i > 0 {
                card_y += card_h + 8.0;
            }

            let spent = self.category_spending(budget.category);
            let usage = Self::usage_ratio(spent, budget.monthly_limit);
            let bar_color = if usage > 1.0 {
                RED
            } else if usage > 0.8 {
                YELLOW
            } else {
                GREEN
            };

            cmds.push(RenderCommand::FillRect {
                x: card_x,
                y: card_y,
                width: card_w,
                height: card_h,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });

            cmds.push(RenderCommand::Text {
                x: card_x + 8.0,
                y: card_y + 6.0,
                text: format!("{} {}", budget.category.icon(), budget.category.label()),
                font_size: 12.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Bold,
                max_width: Some(card_w - 16.0),
                overflow: TextOverflow::Ellipsis,
            });

            cmds.push(RenderCommand::Text {
                x: card_x + 8.0,
                y: card_y + 24.0,
                text: format!(
                    "{} / {}",
                    Self::format_currency(spent),
                    Self::format_currency(budget.monthly_limit)
                ),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(card_w - 16.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Progress bar
            let bar_x = card_x + 8.0;
            let bar_y = card_y + 42.0;
            let bar_w = card_w - 16.0;
            let bar_h = 8.0;
            cmds.push(RenderCommand::FillRect {
                x: bar_x,
                y: bar_y,
                width: bar_w,
                height: bar_h,
                color: SURFACE2,
                corner_radii: CornerRadii::all(4.0),
            });
            let fill_w = (bar_w * usage.min(1.0)).max(0.0);
            if fill_w > 0.0 {
                cmds.push(RenderCommand::FillRect {
                    x: bar_x,
                    y: bar_y,
                    width: fill_w,
                    height: bar_h,
                    color: bar_color,
                    corner_radii: CornerRadii::all(4.0),
                });
            }
        }

        // Top spending categories
        let section_y = card_y + card_h + 24.0;
        cmds.push(RenderCommand::Text {
            x: cx,
            y: section_y,
            text: String::from("Top Spending Categories"),
            font_size: 16.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });

        let top_cats = self.top_expense_categories();
        let max_amount = top_cats.first().map_or(1, |(_, a)| *a).max(1);
        for (i, (cat, amount)) in top_cats.iter().take(5).enumerate() {
            let ry = section_y + 28.0 + i as f32 * 36.0;
            let bar_ratio = *amount as f32 / max_amount as f32;
            let bar_w = (cw - 200.0) * bar_ratio;

            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 4.0,
                text: format!("{} {}", cat.icon(), cat.label()),
                font_size: 12.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular,
                max_width: Some(140.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::FillRect {
                x: cx + 150.0,
                y: ry + 2.0,
                width: bar_w.max(4.0),
                height: 20.0,
                color: cat.color(),
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: cx + 155.0 + bar_w,
                y: ry + 4.0,
                text: Self::format_currency(*amount),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Recent transactions
        let recent_y = section_y + 28.0 + 5.0 * 36.0 + 16.0;
        cmds.push(RenderCommand::Text {
            x: cx,
            y: recent_y,
            text: String::from("Recent Transactions"),
            font_size: 16.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });

        let mut sorted: Vec<&Transaction> = self.month_transactions();
        sorted.sort_by_key(|tx| std::cmp::Reverse(tx.date));
        for (i, tx) in sorted.iter().take(5).enumerate() {
            let ry = recent_y + 28.0 + i as f32 * 28.0;
            cmds.push(RenderCommand::Text {
                x: cx + 4.0,
                y: ry,
                text: tx.date.format(),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(90.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: cx + 100.0,
                y: ry,
                text: tx.description.clone(),
                font_size: 12.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular,
                max_width: Some(300.0),
                overflow: TextOverflow::Ellipsis,
            });
            let (amt_str, amt_color) = Self::format_currency_colored(tx.amount);
            cmds.push(RenderCommand::Text {
                x: cx + cw - 120.0,
                y: ry,
                text: amt_str,
                font_size: 12.0,
                color: amt_color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(110.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_transactions(&self, cmds: &mut Vec<RenderCommand>) {
        let cx = self.content_x() + 8.0;
        let cy = self.content_y() + 8.0;
        let cw = self.content_w() - 16.0;

        // Search bar
        cmds.push(RenderCommand::FillRect {
            x: cx,
            y: cy,
            width: cw,
            height: 32.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        let search_text = if self.search_query.is_empty() {
            if self.search_active {
                String::from("|")
            } else {
                String::from("Press / to search...")
            }
        } else {
            format!("{}|", self.search_query)
        };
        cmds.push(RenderCommand::Text {
            x: cx + 12.0,
            y: cy + 8.0,
            text: search_text,
            font_size: 13.0,
            color: if self.search_query.is_empty() && !self.search_active {
                OVERLAY0
            } else {
                TEXT_COLOR
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(cw - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Filter indicator
        if let Some(cat) = self.category_filter {
            cmds.push(RenderCommand::FillRect {
                x: cx + cw - 140.0,
                y: cy + 4.0,
                width: 130.0,
                height: 24.0,
                color: cat.color(),
                corner_radii: CornerRadii::all(12.0),
            });
            cmds.push(RenderCommand::Text {
                x: cx + cw - 132.0,
                y: cy + 8.0,
                text: cat.label().to_string(),
                font_size: 11.0,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(120.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Column headers
        let list_y = cy + 40.0;
        cmds.push(RenderCommand::FillRect {
            x: cx,
            y: list_y,
            width: cw,
            height: 28.0,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });
        for (hx, label) in [
            (0.0, "Date"),
            (90.0, "Description"),
            (380.0, "Category"),
            (520.0, "Amount"),
        ] {
            cmds.push(RenderCommand::Text {
                x: cx + hx + 8.0,
                y: list_y + 6.0,
                text: label.to_string(),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(120.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Rows
        let filtered = self.filtered_transactions();
        let row_h = 36.0;
        let start = list_y + 32.0;
        for (vi, (orig_idx, tx)) in filtered.iter().enumerate() {
            let ry = start + vi as f32 * row_h;
            if ry > self.content_bottom() {
                break;
            }
            let is_sel = Some(tx.id) == self.selected_id;
            let _ = orig_idx;
            let bg = if is_sel {
                SURFACE1
            } else if vi % 2 == 0 {
                SURFACE0
            } else {
                BASE
            };

            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: ry,
                width: cw,
                height: row_h,
                color: bg,
                corner_radii: CornerRadii::ZERO,
            });
            cmds.push(RenderCommand::Text {
                x: cx + 8.0,
                y: ry + 10.0,
                text: tx.date.format(),
                font_size: 12.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(80.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: cx + 98.0,
                y: ry + 10.0,
                text: tx.description.clone(),
                font_size: 13.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular,
                max_width: Some(270.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: cx + 388.0,
                y: ry + 10.0,
                text: format!("{} {}", tx.category.icon(), tx.category.label()),
                font_size: 11.0,
                color: tx.category.color(),
                font_weight: FontWeightHint::Regular,
                max_width: Some(120.0),
                overflow: TextOverflow::Ellipsis,
            });
            let (amt_str, amt_color) = Self::format_currency_colored(tx.amount);
            cmds.push(RenderCommand::Text {
                x: cx + 528.0,
                y: ry + 10.0,
                text: amt_str,
                font_size: 13.0,
                color: amt_color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(100.0),
                overflow: TextOverflow::Ellipsis,
            });
            if tx.recurring {
                cmds.push(RenderCommand::Text {
                    x: cx + cw - 24.0,
                    y: ry + 10.0,
                    text: String::from("\u{1F501}"),
                    font_size: 11.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(20.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }
    }

    fn render_budgets(&self, cmds: &mut Vec<RenderCommand>) {
        let cx = self.content_x() + 16.0;
        let cy = self.content_y() + 16.0;
        let cw = self.content_w() - 32.0;

        cmds.push(RenderCommand::Text {
            x: cx,
            y: cy,
            text: format!(
                "Budgets for {} {}",
                self.view_month.month_label(),
                self.view_month.year
            ),
            font_size: 18.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(400.0),
            overflow: TextOverflow::Ellipsis,
        });

        let item_h = 80.0;
        for (i, budget) in self.budgets.iter().enumerate() {
            let iy = cy + 36.0 + i as f32 * (item_h + 8.0);
            if iy + item_h > self.content_bottom() {
                break;
            }
            let spent = self.category_spending(budget.category);
            let usage = Self::usage_ratio(spent, budget.monthly_limit);
            let remaining = budget.monthly_limit.saturating_sub(spent);
            let bar_color = if usage > 1.0 {
                RED
            } else if usage > 0.8 {
                YELLOW
            } else {
                GREEN
            };

            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: iy,
                width: cw,
                height: item_h,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });

            cmds.push(RenderCommand::Text {
                x: cx + 12.0,
                y: iy + 8.0,
                text: format!("{} {}", budget.category.icon(), budget.category.label()),
                font_size: 15.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Bold,
                max_width: Some(250.0),
                overflow: TextOverflow::Ellipsis,
            });

            cmds.push(RenderCommand::Text {
                x: cx + cw - 200.0,
                y: iy + 8.0,
                text: format!(
                    "{} / {}",
                    Self::format_currency(spent),
                    Self::format_currency(budget.monthly_limit)
                ),
                font_size: 14.0,
                color: if remaining >= 0 { GREEN } else { RED },
                font_weight: FontWeightHint::Bold,
                max_width: Some(190.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Progress bar
            let bar_x = cx + 12.0;
            let bar_y = iy + 34.0;
            let bar_w = cw - 24.0;
            let bar_h = 12.0;
            cmds.push(RenderCommand::FillRect {
                x: bar_x,
                y: bar_y,
                width: bar_w,
                height: bar_h,
                color: SURFACE2,
                corner_radii: CornerRadii::all(6.0),
            });
            let fill_w = (bar_w * usage.min(1.0)).max(0.0);
            if fill_w > 0.0 {
                cmds.push(RenderCommand::FillRect {
                    x: bar_x,
                    y: bar_y,
                    width: fill_w,
                    height: bar_h,
                    color: bar_color,
                    corner_radii: CornerRadii::all(6.0),
                });
            }

            // Usage percentage and remaining
            cmds.push(RenderCommand::Text {
                x: cx + 12.0,
                y: iy + 54.0,
                text: format!("{:.0}% used", usage * 100.0),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
                overflow: TextOverflow::Ellipsis,
            });
            let rem_text = if remaining >= 0 {
                format!("{} remaining", Self::format_currency(remaining))
            } else {
                format!(
                    "{} over budget!",
                    Self::format_currency(remaining.saturating_neg())
                )
            };
            cmds.push(RenderCommand::Text {
                x: cx + 140.0,
                y: iy + 54.0,
                text: rem_text,
                font_size: 11.0,
                color: if remaining >= 0 { TEAL } else { RED },
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_accounts(&self, cmds: &mut Vec<RenderCommand>) {
        let cx = self.content_x() + 16.0;
        let cy = self.content_y() + 16.0;
        let cw = self.content_w() - 32.0;

        cmds.push(RenderCommand::Text {
            x: cx,
            y: cy,
            text: String::from("Accounts"),
            font_size: 18.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        let card_w = (cw - 16.0) / 2.0;
        let card_h = 80.0;
        for (i, account) in self.accounts.iter().enumerate() {
            let col = i % 2;
            let row = i / 2;
            let ax = cx + col as f32 * (card_w + 16.0);
            let ay = cy + 36.0 + row as f32 * (card_h + 12.0);

            let balance = self.account_balance(account.id);
            let (bal_str, bal_color) = Self::format_currency_colored(balance);

            cmds.push(RenderCommand::FillRect {
                x: ax,
                y: ay,
                width: card_w,
                height: card_h,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });
            cmds.push(RenderCommand::Text {
                x: ax + 12.0,
                y: ay + 10.0,
                text: account.name.clone(),
                font_size: 15.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Bold,
                max_width: Some(card_w - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: ax + 12.0,
                y: ay + 32.0,
                text: account.account_type.label().to_string(),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: ax + 12.0,
                y: ay + 50.0,
                text: bal_str,
                font_size: 22.0,
                color: bal_color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(card_w - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_reports(&self, cmds: &mut Vec<RenderCommand>) {
        let cx = self.content_x() + 16.0;
        let cy = self.content_y() + 16.0;
        let cw = self.content_w() - 32.0;

        cmds.push(RenderCommand::Text {
            x: cx,
            y: cy,
            text: format!(
                "Financial Report — {} {}",
                self.view_month.month_label(),
                self.view_month.year
            ),
            font_size: 18.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(400.0),
            overflow: TextOverflow::Ellipsis,
        });

        let income = self.month_income();
        let expenses = self.month_expenses();
        let net = income.saturating_sub(expenses);
        let tx_count = self.month_transactions().len();

        // Summary cards
        let summaries = [
            ("Total Income", income, GREEN),
            ("Total Expenses", expenses, RED),
            ("Net Savings", net, if net >= 0 { TEAL } else { RED }),
        ];
        for (i, (label, amount, color)) in summaries.iter().enumerate() {
            let sx = cx + i as f32 * (cw / 3.0);
            let sw = cw / 3.0 - 12.0;
            cmds.push(RenderCommand::FillRect {
                x: sx,
                y: cy + 36.0,
                width: sw,
                height: 70.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });
            cmds.push(RenderCommand::Text {
                x: sx + 12.0,
                y: cy + 46.0,
                text: (*label).to_string(),
                font_size: 12.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(sw - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: sx + 12.0,
                y: cy + 66.0,
                text: Self::format_currency(*amount),
                font_size: 22.0,
                color: *color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(sw - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Transaction count
        cmds.push(RenderCommand::Text {
            x: cx,
            y: cy + 120.0,
            text: format!("{tx_count} transactions this month"),
            font_size: 13.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Category breakdown
        cmds.push(RenderCommand::Text {
            x: cx,
            y: cy + 150.0,
            text: String::from("Expense Breakdown by Category"),
            font_size: 16.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(400.0),
            overflow: TextOverflow::Ellipsis,
        });

        let top = self.top_expense_categories();
        let total_exp = expenses.max(1) as f32;
        for (i, (cat, amount)) in top.iter().enumerate() {
            let ry = cy + 178.0 + i as f32 * 32.0;
            let pct = *amount as f32 / total_exp * 100.0;
            let bar_w = (cw - 280.0) * (*amount as f32 / total_exp);

            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 4.0,
                text: format!("{} {}", cat.icon(), cat.label()),
                font_size: 12.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular,
                max_width: Some(140.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::FillRect {
                x: cx + 150.0,
                y: ry + 2.0,
                width: bar_w.max(4.0),
                height: 20.0,
                color: cat.color(),
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: cx + 160.0 + bar_w,
                y: ry + 4.0,
                text: format!("{} ({pct:.1}%)", Self::format_currency(*amount)),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(150.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Savings rate
        if income > 0 {
            let savings_rate = net as f64 / income as f64 * 100.0;
            let sry = cy + 178.0 + top.len() as f32 * 32.0 + 24.0;
            cmds.push(RenderCommand::Text {
                x: cx,
                y: sry,
                text: format!("Savings Rate: {savings_rate:.1}%"),
                font_size: 16.0,
                color: if savings_rate >= 20.0 {
                    GREEN
                } else if savings_rate >= 0.0 {
                    YELLOW
                } else {
                    RED
                },
                font_weight: FontWeightHint::Bold,
                max_width: Some(300.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_status(&self, cmds: &mut Vec<RenderCommand>) {
        let sy = self.height - Self::STATUS_H;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: sy,
            width: self.width,
            height: Self::STATUS_H,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: Self::SIDEBAR_W + 8.0,
            y: sy + 6.0,
            text: self.status_msg.clone(),
            font_size: 12.0,
            color: SUBTEXT1,
            font_weight: FontWeightHint::Regular,
            max_width: Some(400.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

impl App for FinanceApp {
    fn title(&self) -> String {
        "Finance".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "both are positive constants well inside u32"
        )]
        {
            (self.width as u32, self.height as u32)
        }
    }

    /// No clock.
    ///
    /// Nothing in this app ages: balances change only when a transaction is
    /// added or deleted, and the month on screen moves only when the user moves
    /// it. Asking for a tick would wake the machine to redraw an identical
    /// frame. This is the opposite of `known-issues.md` lesson 47, and the
    /// check is the same one — *is there state that advances on its own?* Here
    /// there is not, and `current_date` is a constant besides, which is its own
    /// entry.
    fn tick_interval(&self) -> Option<Duration> {
        None
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // Reconciled with the size we are handed rather than trusted from the
        // last `Resize`: the compositor may grant a size that was never asked
        // for, and the first frame is drawn before any `Resize` arrives.
        self.width = width;
        self.height = height;
        RenderTree {
            commands: self.render_commands(),
        }
    }
}

fn main() -> ExitCode {
    let mut finance = FinanceApp::new();
    app::launch("finance", &mut finance)
}

// ── Tests ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    // A test that overflows, indexes out of range or unwraps a `None` should
    // fail loudly and point at the line that did it — that is the diagnosis.
    // The defensive lints exist to keep panics out of code that runs on a
    // user's data, which this is not.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;

    #[test]
    fn test_new_app() {
        let app = FinanceApp::new();
        assert!(!app.transactions.is_empty());
        assert!(!app.accounts.is_empty());
        assert!(!app.budgets.is_empty());
        assert_eq!(app.screen, Screen::Dashboard);
    }

    #[test]
    fn test_sample_data_accounts() {
        let app = FinanceApp::new();
        assert_eq!(app.accounts.len(), 4);
    }

    #[test]
    fn test_sample_data_budgets() {
        let app = FinanceApp::new();
        assert_eq!(app.budgets.len(), 7);
    }

    #[test]
    fn test_add_account() {
        let mut app = FinanceApp::new();
        let n = app.accounts.len();
        app.add_account("Test", AccountType::Cash, 1000);
        assert_eq!(app.accounts.len(), n + 1);
    }

    #[test]
    fn test_add_transaction() {
        let mut app = FinanceApp::new();
        let n = app.transactions.len();
        app.add_transaction(
            SimpleDate::new(2026, 5, 18),
            "Test",
            -1000,
            Category::Food,
            1,
            "",
            false,
        );
        assert_eq!(app.transactions.len(), n + 1);
    }

    #[test]
    fn test_delete_transaction() {
        let mut app = FinanceApp::new();
        let n = app.transactions.len();
        let id = app.transactions[0].id;
        app.delete_transaction(id);
        assert_eq!(app.transactions.len(), n - 1);
        assert!(
            !app.transactions.iter().any(|tx| tx.id == id),
            "the deleted transaction should be the one that is gone"
        );
    }

    #[test]
    fn deleting_leaves_the_selection_on_a_row_that_still_exists() {
        // The old index-based selection re-pointed at whatever slid into the
        // gap; worse, deleting the last row left it past the end.
        let mut app = FinanceApp::new();
        app.handle_key("Down", false, false);
        for _ in 0..3 {
            let Some(id) = app.selected_id else {
                panic!("something should be selected while rows remain")
            };
            app.delete_transaction(id);
            assert_ne!(
                app.selected_id,
                Some(id),
                "selection stayed on a deleted row"
            );
            if let Some(sel) = app.selected_id {
                assert!(
                    app.transactions.iter().any(|tx| tx.id == sel),
                    "selection points at a transaction that does not exist"
                );
            }
        }
    }

    #[test]
    fn arrow_keys_stay_inside_the_rows_actually_on_screen() {
        // With a filter on, the arrow keys used to walk through hidden rows:
        // the highlight vanished for several presses and Ctrl+D then deleted
        // something the user could not see.
        let mut app = FinanceApp::new();
        app.category_filter = Some(Category::Food);
        app.reanchor_selection();
        let visible = app.visible_ids();
        assert!(
            visible.len() >= 2,
            "the sample data should have at least two Food transactions"
        );
        assert!(
            visible.len() < app.transactions.len(),
            "the filter should hide rows"
        );
        for _ in 0..app.transactions.len() {
            app.handle_key("Down", false, false);
            let sel = app.selected_id.expect("a visible row stays selected");
            assert!(visible.contains(&sel), "selection left the filtered view");
        }
    }

    #[test]
    fn nothing_is_deleted_while_nothing_is_selected() {
        // Ctrl+D on a fresh window used to delete the first transaction, which
        // the user had never pointed at.
        let mut app = FinanceApp::new();
        let n = app.transactions.len();
        assert!(app.selected_id.is_none(), "a fresh window selects nothing");
        app.handle_key("d", true, false);
        assert_eq!(app.transactions.len(), n);
    }

    #[test]
    fn test_delete_out_of_bounds() {
        let mut app = FinanceApp::new();
        let n = app.transactions.len();
        app.delete_transaction(999);
        assert_eq!(app.transactions.len(), n);
    }

    #[test]
    fn test_set_budget_new() {
        let mut app = FinanceApp::new();
        let n = app.budgets.len();
        app.set_budget(Category::Savings, 100_000);
        assert_eq!(app.budgets.len(), n + 1);
    }

    #[test]
    fn test_month_income() {
        let app = FinanceApp::new();
        let income = app.month_income();
        assert!(income > 0);
    }

    #[test]
    fn test_month_expenses() {
        let app = FinanceApp::new();
        let expenses = app.month_expenses();
        assert!(expenses > 0);
    }

    #[test]
    fn test_month_savings() {
        let app = FinanceApp::new();
        let savings = app.month_savings();
        let income = app.month_income();
        let expenses = app.month_expenses();
        assert_eq!(savings, income - expenses);
    }

    #[test]
    fn test_category_spending() {
        let app = FinanceApp::new();
        let food = app.category_spending(Category::Food);
        assert!(food > 0);
    }

    #[test]
    fn test_account_balance() {
        let app = FinanceApp::new();
        let bal = app.account_balance(1);
        assert!(bal != 0);
    }

    #[test]
    fn test_total_balance() {
        let app = FinanceApp::new();
        let total = app.total_balance();
        assert!(total > 0);
    }

    #[test]
    fn test_filtered_all() {
        let app = FinanceApp::new();
        let f = app.filtered_transactions();
        assert_eq!(f.len(), app.transactions.len());
    }

    #[test]
    fn test_filtered_by_category() {
        let mut app = FinanceApp::new();
        app.category_filter = Some(Category::Food);
        let f = app.filtered_transactions();
        assert!(f.len() < app.transactions.len());
        for (_, tx) in &f {
            assert_eq!(tx.category, Category::Food);
        }
    }

    #[test]
    fn test_filtered_by_search() {
        let mut app = FinanceApp::new();
        app.search_query = String::from("grocery");
        let f = app.filtered_transactions();
        assert!(!f.is_empty());
        for (_, tx) in &f {
            assert!(tx.description.to_ascii_lowercase().contains("grocery"));
        }
    }

    #[test]
    fn test_top_expense_categories() {
        let app = FinanceApp::new();
        let top = app.top_expense_categories();
        assert!(!top.is_empty());
        // Should be sorted descending
        for w in top.windows(2) {
            assert!(w[0].1 >= w[1].1);
        }
    }

    #[test]
    fn test_format_currency() {
        assert_eq!(FinanceApp::format_currency(12345), "$123.45");
        assert_eq!(FinanceApp::format_currency(-500), "-$5.00");
        assert_eq!(FinanceApp::format_currency(0), "$0.00");
    }

    #[test]
    fn test_format_currency_colored() {
        let (_, c1) = FinanceApp::format_currency_colored(100);
        let (_, c2) = FinanceApp::format_currency_colored(-100);
        let (_, c3) = FinanceApp::format_currency_colored(0);
        assert_eq!(c1.r, GREEN.r);
        assert_eq!(c2.r, RED.r);
        assert_eq!(c3.r, TEXT_COLOR.r);
    }

    #[test]
    fn test_simple_date_format() {
        let d = SimpleDate::new(2026, 5, 18);
        assert_eq!(d.format(), "2026-05-18");
    }

    #[test]
    fn test_simple_date_month_label() {
        let d = SimpleDate::new(2026, 1, 1);
        assert_eq!(d.month_label(), "January");
        let d = SimpleDate::new(2026, 12, 1);
        assert_eq!(d.month_label(), "December");
    }

    #[test]
    fn test_simple_date_same_month() {
        let a = SimpleDate::new(2026, 5, 1);
        let b = SimpleDate::new(2026, 5, 31);
        assert!(a.same_month(&b));
        let c = SimpleDate::new(2026, 6, 1);
        assert!(!a.same_month(&c));
    }

    #[test]
    fn test_prev_month() {
        let d = SimpleDate::new(2026, 5, 1);
        let p = d.prev_month();
        assert_eq!(p.month, 4);
    }

    #[test]
    fn test_prev_month_year_wrap() {
        let d = SimpleDate::new(2026, 1, 1);
        let p = d.prev_month();
        assert_eq!(p.year, 2025);
        assert_eq!(p.month, 12);
    }

    #[test]
    fn test_next_month() {
        let d = SimpleDate::new(2026, 5, 1);
        let n = d.next_month();
        assert_eq!(n.month, 6);
    }

    #[test]
    fn test_next_month_year_wrap() {
        let d = SimpleDate::new(2026, 12, 1);
        let n = d.next_month();
        assert_eq!(n.year, 2027);
        assert_eq!(n.month, 1);
    }

    #[test]
    fn test_category_labels() {
        for cat in &Category::ALL {
            assert!(!cat.label().is_empty());
            assert!(!cat.icon().is_empty());
        }
    }

    #[test]
    fn test_transaction_income_expense() {
        let income = Transaction {
            id: 1,
            date: SimpleDate::new(2026, 5, 1),
            description: String::new(),
            amount: 1000,
            category: Category::Income,
            account_id: 1,
            notes: String::new(),
            recurring: false,
        };
        assert!(income.is_income());
        assert!(!income.is_expense());

        let expense = Transaction {
            id: 2,
            date: SimpleDate::new(2026, 5, 1),
            description: String::new(),
            amount: -1000,
            category: Category::Food,
            account_id: 1,
            notes: String::new(),
            recurring: false,
        };
        assert!(!expense.is_income());
        assert!(expense.is_expense());
    }

    #[test]
    fn test_handle_key_screen_switch() {
        let mut app = FinanceApp::new();
        app.handle_key("2", false, false);
        assert_eq!(app.screen, Screen::Transactions);
        app.handle_key("3", false, false);
        assert_eq!(app.screen, Screen::Budgets);
        app.handle_key("4", false, false);
        assert_eq!(app.screen, Screen::Accounts);
        app.handle_key("5", false, false);
        assert_eq!(app.screen, Screen::Reports);
        app.handle_key("1", false, false);
        assert_eq!(app.screen, Screen::Dashboard);
    }

    #[test]
    fn test_handle_key_month_nav() {
        let mut app = FinanceApp::new();
        let month = app.view_month.month;
        app.handle_key("Left", false, false);
        assert_eq!(app.view_month.month, month - 1);
        app.handle_key("Right", false, false);
        assert_eq!(app.view_month.month, month);
    }

    #[test]
    fn test_handle_key_search() {
        let mut app = FinanceApp::new();
        app.handle_key("/", false, false);
        assert!(app.search_active);
        app.handle_key("Escape", false, false);
        assert!(!app.search_active);
    }

    #[test]
    fn test_handle_key_category_filter() {
        let mut app = FinanceApp::new();
        assert!(app.category_filter.is_none());
        app.handle_key("c", false, false);
        assert!(app.category_filter.is_some());
    }

    #[test]
    fn test_set_budget() {
        let mut app = FinanceApp::new();
        app.set_budget(Category::Food, 80_000);
        // Observed on the store itself rather than through an accessor that
        // exists only for this test.
        let b = app
            .budgets
            .iter()
            .find(|b| b.category == Category::Food)
            .map(|b| b.monthly_limit);
        assert_eq!(b, Some(80_000));
    }

    #[test]
    fn usage_ratio_is_the_fraction_of_the_limit_spent() {
        assert!((FinanceApp::usage_ratio(30_000, 60_000) - 0.5).abs() < 0.001);
        assert!((FinanceApp::usage_ratio(60_000, 60_000) - 1.0).abs() < 0.001);
    }

    #[test]
    fn usage_ratio_reports_overspending_rather_than_clamping() {
        // The bar's colour depends on crossing 1.0, so the ratio must be able
        // to exceed it.
        assert!(FinanceApp::usage_ratio(90_000, 60_000) > 1.0);
    }

    #[test]
    fn a_limit_of_zero_reads_as_empty_and_not_as_infinity() {
        // This is the branch the dashboard and the budgets screen disagreed
        // about while each had its own copy of the division: one returned 0.0,
        // the other divided by zero and produced `inf`, which draws as a bar
        // past the end of its track and a permanently red category.
        assert!((FinanceApp::usage_ratio(5_000, 0) - 0.0).abs() < f32::EPSILON);
        assert!(FinanceApp::usage_ratio(5_000, 0).is_finite());
        // A negative limit is nonsense rather than infinite overspend.
        assert!((FinanceApp::usage_ratio(5_000, -100) - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn spending_nothing_against_a_real_limit_is_zero() {
        assert!((FinanceApp::usage_ratio(0, 60_000) - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_handle_key_navigation() {
        let mut app = FinanceApp::new();
        let visible = app.visible_ids();
        assert!(visible.len() >= 2, "the sample data should fill the list");
        // Nothing is selected until the user moves, and the first move lands on
        // the first row rather than the second.
        app.handle_key("Down", false, false);
        assert_eq!(app.selected_id, visible.first().copied());
        app.handle_key("Down", false, false);
        assert_eq!(app.selected_id, visible.get(1).copied());
        app.handle_key("Up", false, false);
        assert_eq!(app.selected_id, visible.first().copied());
        // At the top, Up stays put rather than wrapping to the bottom.
        app.handle_key("Up", false, false);
        assert_eq!(app.selected_id, visible.first().copied());
    }

    #[test]
    fn test_handle_search_text() {
        let mut app = FinanceApp::new();
        app.search_active = true;
        app.handle_search_text("test");
        assert_eq!(app.search_query, "test");
    }

    #[test]
    fn test_render_dashboard() {
        let app = FinanceApp::new();
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_transactions() {
        let mut app = FinanceApp::new();
        app.screen = Screen::Transactions;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_budgets() {
        let mut app = FinanceApp::new();
        app.screen = Screen::Budgets;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_accounts() {
        let mut app = FinanceApp::new();
        app.screen = Screen::Accounts;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_reports() {
        let mut app = FinanceApp::new();
        app.screen = Screen::Reports;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_search() {
        let mut app = FinanceApp::new();
        app.screen = Screen::Transactions;
        app.search_active = true;
        app.search_query = String::from("grocery");
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_filter() {
        let mut app = FinanceApp::new();
        app.screen = Screen::Transactions;
        app.category_filter = Some(Category::Food);
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_account_type_labels() {
        let types = [
            AccountType::Checking,
            AccountType::Savings,
            AccountType::CreditCard,
            AccountType::Cash,
            AccountType::Investment,
        ];
        for t in &types {
            assert!(!t.label().is_empty());
        }
    }

    #[test]
    fn test_screen_labels() {
        for s in &Screen::ALL {
            assert!(!s.label().is_empty());
        }
    }

    #[test]
    fn test_month_transactions_only_current() {
        let app = FinanceApp::new();
        let txs = app.month_transactions();
        for tx in &txs {
            assert!(tx.date.same_month(&app.view_month));
        }
    }

    #[test]
    fn test_different_month_no_transactions() {
        let mut app = FinanceApp::new();
        app.view_month = SimpleDate::new(2025, 1, 1);
        let txs = app.month_transactions();
        assert!(txs.is_empty());
    }

    #[test]
    fn test_handle_key_delete() {
        let mut app = FinanceApp::new();
        let n = app.transactions.len();
        app.handle_key("Down", false, false);
        app.handle_key("d", true, false);
        assert_eq!(app.transactions.len(), n - 1);
    }
}
