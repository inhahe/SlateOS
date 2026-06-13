#![allow(dead_code)]
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
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

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

    fn is_income(self) -> bool {
        matches!(self, Self::Income | Self::Investment)
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

    fn next_day(self) -> Self {
        let days_in_month = match self.month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if self.year.is_multiple_of(4)
                    && (!self.year.is_multiple_of(100) || self.year.is_multiple_of(400))
                {
                    29
                } else {
                    28
                }
            }
            _ => 30,
        };
        if self.day < days_in_month {
            Self::new(self.year, self.month, self.day + 1)
        } else if self.month < 12 {
            Self::new(self.year, self.month + 1, 1)
        } else {
            Self::new(self.year + 1, 1, 1)
        }
    }

    fn prev_month(self) -> Self {
        if self.month > 1 {
            Self::new(self.year, self.month - 1, 1)
        } else {
            Self::new(self.year - 1, 12, 1)
        }
    }

    fn next_month(self) -> Self {
        if self.month < 12 {
            Self::new(self.year, self.month + 1, 1)
        } else {
            Self::new(self.year + 1, 1, 1)
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
    fn amount_dollars(&self) -> f64 {
        self.amount as f64 / 100.0
    }

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
    selected_tx: usize,
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
            selected_tx: 0,
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
        self.next_account_id += 1;
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
        self.next_tx_id += 1;
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

    fn delete_transaction(&mut self, idx: usize) {
        if idx < self.transactions.len() {
            self.transactions.remove(idx);
            if self.selected_tx >= self.transactions.len() && !self.transactions.is_empty() {
                self.selected_tx = self.transactions.len().saturating_sub(1);
            }
            self.status_msg = String::from("Transaction deleted");
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
        self.month_income() - self.month_expenses()
    }

    fn category_spending(&self, cat: Category) -> i64 {
        self.month_transactions()
            .iter()
            .filter(|tx| tx.category == cat && tx.is_expense())
            .map(|tx| tx.amount.abs())
            .sum()
    }

    fn budget_for(&self, cat: Category) -> Option<i64> {
        self.budgets
            .iter()
            .find(|b| b.category == cat)
            .map(|b| b.monthly_limit)
    }

    fn budget_usage(&self, cat: Category) -> Option<f32> {
        self.budget_for(cat).map(|limit| {
            if limit == 0 {
                return 0.0;
            }
            let spent = self.category_spending(cat);
            spent as f32 / limit as f32
        })
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
        initial + tx_sum
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
                }
                "Backspace" => {
                    self.search_query.pop();
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
            }
            "Right" => {
                self.view_month = self.view_month.next_month();
            }
            "Up" | "k" if self.selected_tx > 0 => {
                self.selected_tx -= 1;
            }
            "Down" | "j" if self.selected_tx + 1 < self.transactions.len() => {
                self.selected_tx += 1;
            }
            "/" => {
                self.search_active = true;
                self.search_query.clear();
            }
            "c" => {
                // Cycle category filter
                self.category_filter = match self.category_filter {
                    None => Some(Category::ALL[0]),
                    Some(cat) => {
                        let idx = Category::ALL.iter().position(|&c| c == cat).unwrap_or(0);
                        if idx + 1 < Category::ALL.len() {
                            Some(Category::ALL[idx + 1])
                        } else {
                            None
                        }
                    }
                };
                if let Some(cat) = self.category_filter {
                    self.status_msg = format!("Filter: {}", cat.label());
                } else {
                    self.status_msg = String::from("Filter: All");
                }
            }
            "Delete" | "d" if ctrl => {
                let idx = self.selected_tx;
                self.delete_transaction(idx);
            }
            _ => {}
        }
    }

    fn handle_search_text(&mut self, text: &str) {
        if self.search_active {
            self.search_query.push_str(text);
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
        let color = if cents > 0 {
            GREEN
        } else if cents < 0 {
            RED
        } else {
            TEXT_COLOR
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

    // ── Rendering ───────────────────────────────────────────────────
    fn render(&self) -> Vec<RenderCommand> {
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
                text: format!("{} {}", i + 1, screen.label()),
                font_size: 13.0,
                color: tc,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(Self::SIDEBAR_W - 40.0),
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
        });
        cmds.push(RenderCommand::Text {
            x: 16.0,
            y: self.height - 42.0,
            text: total_str,
            font_size: 18.0,
            color: total_color,
            font_weight: FontWeightHint::Bold,
            max_width: Some(Self::SIDEBAR_W - 24.0),
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
            });
            // Expenses are accumulated as a positive magnitude; render them as
            // a money-out (negative) figure so the header reads "-$X.XX".
            let val = if label == "Expenses" {
                Self::format_currency(-amount)
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
            let usage = if budget.monthly_limit > 0 {
                spent as f32 / budget.monthly_limit as f32
            } else {
                0.0
            };
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
            });
            cmds.push(RenderCommand::Text {
                x: cx + 100.0,
                y: ry,
                text: tx.description.clone(),
                font_size: 12.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular,
                max_width: Some(300.0),
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
            });
        }

        // Rows
        let filtered = self.filtered_transactions();
        let row_h = 36.0;
        let start = list_y + 32.0;
        for (vi, (orig_idx, tx)) in filtered.iter().enumerate() {
            let ry = start + vi as f32 * row_h;
            if ry > self.height - Self::STATUS_H {
                break;
            }
            let is_sel = *orig_idx == self.selected_tx;
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
            });
            cmds.push(RenderCommand::Text {
                x: cx + 98.0,
                y: ry + 10.0,
                text: tx.description.clone(),
                font_size: 13.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular,
                max_width: Some(270.0),
            });
            cmds.push(RenderCommand::Text {
                x: cx + 388.0,
                y: ry + 10.0,
                text: format!("{} {}", tx.category.icon(), tx.category.label()),
                font_size: 11.0,
                color: tx.category.color(),
                font_weight: FontWeightHint::Regular,
                max_width: Some(120.0),
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
        });

        let item_h = 80.0;
        for (i, budget) in self.budgets.iter().enumerate() {
            let iy = cy + 36.0 + i as f32 * (item_h + 8.0);
            if iy + item_h > self.height - Self::STATUS_H {
                break;
            }
            let spent = self.category_spending(budget.category);
            let usage = if budget.monthly_limit > 0 {
                spent as f32 / budget.monthly_limit as f32
            } else {
                0.0
            };
            let remaining = budget.monthly_limit - spent;
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
            });
            let rem_text = if remaining >= 0 {
                format!("{} remaining", Self::format_currency(remaining))
            } else {
                format!("{} over budget!", Self::format_currency(-remaining))
            };
            cmds.push(RenderCommand::Text {
                x: cx + 140.0,
                y: iy + 54.0,
                text: rem_text,
                font_size: 11.0,
                color: if remaining >= 0 { TEAL } else { RED },
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
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
            });
            cmds.push(RenderCommand::Text {
                x: ax + 12.0,
                y: ay + 32.0,
                text: account.account_type.label().to_string(),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
            });
            cmds.push(RenderCommand::Text {
                x: ax + 12.0,
                y: ay + 50.0,
                text: bal_str,
                font_size: 22.0,
                color: bal_color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(card_w - 24.0),
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
        });

        let income = self.month_income();
        let expenses = self.month_expenses();
        let net = income - expenses;
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
            });
            cmds.push(RenderCommand::Text {
                x: sx + 12.0,
                y: cy + 66.0,
                text: Self::format_currency(*amount),
                font_size: 22.0,
                color: *color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(sw - 24.0),
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
        });
    }
}

fn main() {
    let _app = FinanceApp::new();
}

// ── Tests ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
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
        app.delete_transaction(0);
        assert_eq!(app.transactions.len(), n - 1);
    }

    #[test]
    fn test_delete_out_of_bounds() {
        let mut app = FinanceApp::new();
        let n = app.transactions.len();
        app.delete_transaction(999);
        assert_eq!(app.transactions.len(), n);
    }

    #[test]
    fn test_set_budget() {
        let mut app = FinanceApp::new();
        app.set_budget(Category::Food, 80_000);
        let b = app.budget_for(Category::Food);
        assert_eq!(b, Some(80_000));
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
    fn test_budget_usage() {
        let app = FinanceApp::new();
        let usage = app.budget_usage(Category::Food);
        assert!(usage.is_some());
        let u = usage.unwrap();
        assert!(u > 0.0);
    }

    #[test]
    fn test_budget_usage_none() {
        let app = FinanceApp::new();
        let usage = app.budget_usage(Category::Income);
        assert!(usage.is_none());
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
    fn test_simple_date_next_day() {
        let d = SimpleDate::new(2026, 5, 18);
        let n = d.next_day();
        assert_eq!(n.day, 19);
    }

    #[test]
    fn test_simple_date_next_day_month_wrap() {
        let d = SimpleDate::new(2026, 5, 31);
        let n = d.next_day();
        assert_eq!(n.month, 6);
        assert_eq!(n.day, 1);
    }

    #[test]
    fn test_simple_date_next_day_year_wrap() {
        let d = SimpleDate::new(2026, 12, 31);
        let n = d.next_day();
        assert_eq!(n.year, 2027);
        assert_eq!(n.month, 1);
        assert_eq!(n.day, 1);
    }

    #[test]
    fn test_simple_date_leap_year() {
        let d = SimpleDate::new(2024, 2, 28);
        let n = d.next_day();
        assert_eq!(n.day, 29);
        let m = n.next_day();
        assert_eq!(m.month, 3);
    }

    #[test]
    fn test_simple_date_non_leap_year() {
        let d = SimpleDate::new(2026, 2, 28);
        let n = d.next_day();
        assert_eq!(n.month, 3);
        assert_eq!(n.day, 1);
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
    fn test_category_is_income() {
        assert!(Category::Income.is_income());
        assert!(Category::Investment.is_income());
        assert!(!Category::Food.is_income());
    }

    #[test]
    fn test_transaction_amount_dollars() {
        let tx = Transaction {
            id: 1,
            date: SimpleDate::new(2026, 5, 1),
            description: String::new(),
            amount: 12345,
            category: Category::Income,
            account_id: 1,
            notes: String::new(),
            recurring: false,
        };
        assert!((tx.amount_dollars() - 123.45).abs() < 0.01);
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
    fn test_handle_key_navigation() {
        let mut app = FinanceApp::new();
        app.handle_key("Down", false, false);
        assert_eq!(app.selected_tx, 1);
        app.handle_key("Up", false, false);
        assert_eq!(app.selected_tx, 0);
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
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_transactions() {
        let mut app = FinanceApp::new();
        app.screen = Screen::Transactions;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_budgets() {
        let mut app = FinanceApp::new();
        app.screen = Screen::Budgets;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_accounts() {
        let mut app = FinanceApp::new();
        app.screen = Screen::Accounts;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_reports() {
        let mut app = FinanceApp::new();
        app.screen = Screen::Reports;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_search() {
        let mut app = FinanceApp::new();
        app.screen = Screen::Transactions;
        app.search_active = true;
        app.search_query = String::from("grocery");
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_filter() {
        let mut app = FinanceApp::new();
        app.screen = Screen::Transactions;
        app.category_filter = Some(Category::Food);
        let cmds = app.render();
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
        app.handle_key("d", true, false);
        assert_eq!(app.transactions.len(), n - 1);
    }
}
