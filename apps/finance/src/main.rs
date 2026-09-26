#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_lossless)]

//! Slate OS Personal Finance — budget tracking and expense management.
//!
//! Track income and expenses across categories, set budgets, view spending
//! trends, manage accounts, and get financial summaries.
//!
//! Accounts, transactions and budgets are entered through forms (`Form`), and
//! every control answers the pointer as well as the keys (F1 lists them).
//! Everything is kept as it changes, in a text ledger in the settings
//! directory (`ledger_path`, `ledger_text`, `parse_ledger`). "Today" is the
//! clock's. It could record nothing until 2026-09-25: `add_account`,
//! `add_transaction` and `set_budget` had no production caller, nothing was
//! kept, and the window said so from under the sidebar and header that painted
//! over the notice.

use appearance::Palette;
use appearance::Surface;
use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text::TextCursor;
use guitk::textedit;
use guitk::textinput::TextInput;
use guitk::wheel;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use textfmt::tsv;

// ── Catppuccin Mocha palette ────────────────────────────────────────

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

    /// The name the ledger writes: fixed, unlike `label`, which is for people
    /// and may change.
    fn key(self) -> &'static str {
        match self {
            Self::Food => "food",
            Self::Housing => "housing",
            Self::Transportation => "transportation",
            Self::Utilities => "utilities",
            Self::Healthcare => "healthcare",
            Self::Entertainment => "entertainment",
            Self::Shopping => "shopping",
            Self::Education => "education",
            Self::Savings => "savings",
            Self::Income => "income",
            Self::Investment => "investment",
            Self::Other => "other",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == key)
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

    fn color(self, pal: &Palette) -> Color {
        match self {
            Self::Food => pal.peach,
            Self::Housing => pal.blue,
            Self::Transportation => pal.sky,
            Self::Utilities => pal.yellow,
            Self::Healthcare => pal.red,
            Self::Entertainment => pal.mauve,
            Self::Shopping => pal.lavender,
            Self::Education => pal.teal,
            Self::Savings => pal.green,
            Self::Income => pal.green,
            Self::Investment => pal.blue,
            Self::Other => pal.overlay0,
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

    /// A date typed as `YYYY-MM-DD`, or `None` if it is not one.
    fn parse(text: &str) -> Option<Self> {
        let mut parts = text.trim().splitn(3, '-');
        let year: u16 = parts.next()?.trim().parse().ok()?;
        let month: u8 = parts.next()?.trim().parse().ok()?;
        let day: u8 = parts.next()?.trim().parse().ok()?;
        if !(1..=12).contains(&month) || day == 0 {
            return None;
        }
        if u32::from(day) > guitk::date::days_in_month(i32::from(year), u32::from(month)) {
            return None;
        }
        Some(Self::new(year, month, day))
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
#[derive(Clone, Debug, PartialEq, Eq)]
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
#[derive(Clone, Debug, PartialEq, Eq)]
struct Account {
    id: u32,
    name: String,
    account_type: AccountType,
    initial_balance: i64, // cents
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
/// The kinds of account this app can model, each chosen in the account form.
enum AccountType {
    Checking,
    Savings,
    CreditCard,
    Cash,
    Investment,
}

impl AccountType {
    const ALL: [Self; 5] = [
        Self::Checking,
        Self::Savings,
        Self::CreditCard,
        Self::Cash,
        Self::Investment,
    ];

    /// The name the ledger writes.
    fn key(self) -> &'static str {
        match self {
            Self::Checking => "checking",
            Self::Savings => "savings",
            Self::CreditCard => "credit-card",
            Self::Cash => "cash",
            Self::Investment => "investment",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }

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
#[derive(Clone, Debug, PartialEq, Eq)]
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

// ── Forms ───────────────────────────────────────────────────────────

/// A field of one of the three forms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FormField {
    Date,
    Description,
    Amount,
    /// Income or expense.
    Kind,
    Category,
    Account,
    Notes,
    Recurring,
    Name,
    AccountKind,
    Opening,
    Limit,
}

impl FormField {
    /// Whether the field is typed into; the others are chosen, a press or
    /// Left and Right stepping through their values.
    fn is_text(self) -> bool {
        matches!(
            self,
            Self::Date
                | Self::Description
                | Self::Amount
                | Self::Notes
                | Self::Name
                | Self::Opening
                | Self::Limit
        )
    }
}

/// What a form is entering.
///
/// There was no way to put a figure into this program: `add_account`,
/// `add_transaction` and `set_budget` were written, tested, and called by
/// nothing but the tests.
#[derive(Clone, Debug)]
enum Form {
    /// A transaction, new (`id: None`) or being changed.
    Transaction {
        id: Option<u32>,
        date: TextInput,
        description: TextInput,
        amount: TextInput,
        notes: TextInput,
        income: bool,
        category: Category,
        account: Option<u32>,
        recurring: bool,
    },
    /// An account, new or being changed.
    Account {
        id: Option<u32>,
        name: TextInput,
        kind: AccountType,
        opening: TextInput,
    },
    /// A category's monthly budget. Empty or zero takes it off.
    Budget {
        category: Category,
        limit: TextInput,
    },
}

impl Form {
    /// The fields, in the order Tab walks them.
    fn fields(&self) -> &'static [FormField] {
        match self {
            Self::Transaction { .. } => &[
                FormField::Date,
                FormField::Description,
                FormField::Amount,
                FormField::Kind,
                FormField::Category,
                FormField::Account,
                FormField::Notes,
                FormField::Recurring,
            ],
            Self::Account { .. } => &[FormField::Name, FormField::AccountKind, FormField::Opening],
            Self::Budget { .. } => &[FormField::Limit],
        }
    }

    /// The text field `which`, if this form has it.
    fn input(&mut self, which: FormField) -> Option<&mut TextInput> {
        match (self, which) {
            (Self::Transaction { date, .. }, FormField::Date) => Some(date),
            (Self::Transaction { description, .. }, FormField::Description) => Some(description),
            (Self::Transaction { amount, .. }, FormField::Amount) => Some(amount),
            (Self::Transaction { notes, .. }, FormField::Notes) => Some(notes),
            (Self::Account { name, .. }, FormField::Name) => Some(name),
            (Self::Account { opening, .. }, FormField::Opening) => Some(opening),
            (Self::Budget { limit, .. }, FormField::Limit) => Some(limit),
            _ => None,
        }
    }

    /// The text field `which`, to read.
    fn input_ref(&self, which: FormField) -> Option<&TextInput> {
        match (self, which) {
            (Self::Transaction { date, .. }, FormField::Date) => Some(date),
            (Self::Transaction { description, .. }, FormField::Description) => Some(description),
            (Self::Transaction { amount, .. }, FormField::Amount) => Some(amount),
            (Self::Transaction { notes, .. }, FormField::Notes) => Some(notes),
            (Self::Account { name, .. }, FormField::Name) => Some(name),
            (Self::Account { opening, .. }, FormField::Opening) => Some(opening),
            (Self::Budget { limit, .. }, FormField::Limit) => Some(limit),
            _ => None,
        }
    }
}

/// What a delete waiting on its answer would remove.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Doomed {
    Transaction(u32),
    /// An account, with every transaction in it.
    Account(u32),
}

/// Everything in the window a pointer can press, as the renderer records it.
///
/// The program drew five screens in a sidebar, a month with arrows, a search
/// box and rows of transactions, budgets and accounts, and handled no pointer
/// event (`known-issues.md` ->
/// `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Screen(Screen),
    MonthPrev,
    MonthNext,
    ThisMonth,
    Help,
    NewTransaction,
    NewAccount,
    Edit,
    Delete,
    Search,
    FilterChip,
    TxList,
    TxRow(u32),
    AccountList,
    AccountRow(u32),
    BudgetList,
    /// A budget, by its category's place in `Category::EXPENSE_CATS`.
    BudgetRow(usize),
    /// A transaction in the dashboard's recent list: a press shows it in the
    /// transactions screen.
    RecentRow(u32),
    Field(FormField),
    /// The arrows either side of a chosen field.
    StepBack(FormField),
    StepForward(FormField),
    Save,
    Cancel,
    /// Around and behind a form's controls: a press does nothing.
    FormBackdrop,
    ConfirmDelete,
    KeepIt,
    QuestionBackdrop,
    QuestionCard,
    HelpCard,
}

/// Every key this program answers, and what it does.
///
/// **Each row is a key this program actually answers**, checked by
/// `every_advertised_key_does_something`.
const SHORTCUTS: &[(&str, &str)] = &[
    (
        "1-5",
        "Dashboard / transactions / budgets / accounts / reports",
    ),
    (
        "Left / Right",
        "Previous / next month; in a form, the choice",
    ),
    ("Home", "This month"),
    ("Up / Down", "Choose a transaction, a budget or an account"),
    ("N", "New transaction; on the accounts screen, new account"),
    ("Enter", "Change what is chosen; in a form, save"),
    ("Delete / Ctrl+D", "Delete what is chosen (asks first)"),
    ("/", "Search the transactions"),
    ("C", "Show one category"),
    ("PgUp / PgDn", "Scroll the list"),
    ("Tab / Shift+Tab", "Next / previous field in a form"),
    ("Esc", "Close the form, the search or this list"),
    ("F1", "This list"),
];

// ── App ─────────────────────────────────────────────────────────────
struct FinanceApp {
    width: f32,
    height: f32,
    screen: Screen,
    transactions: Vec<Transaction>,
    accounts: Vec<Account>,
    budgets: Vec<Budget>,
    // Allocated by `add_transaction` and `add_account`, neither of which has
    // a caller: nothing in this program creates a transaction or an account.
    // See `NO_DATA_LINES`.
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
    /// The form that is up, the field the keyboard is in, and what its last
    /// save said was wrong.
    form: Option<Form>,
    field: FormField,
    form_error: Option<String>,
    /// The fields' clipboard.
    clipboard: String,
    /// A delete waiting on its answer.
    pending_delete: Option<Doomed>,
    /// The chosen account, by id, and budget, by its category's place in
    /// `Category::EXPENSE_CATS`.
    selected_account: Option<u32>,
    selected_budget: usize,
    /// How far each list is scrolled, in rows. None of them scrolled: rows
    /// past the bottom edge were simply not drawn, and the transaction arrows
    /// were held to the rows on screen, so an older transaction could never
    /// be seen at all.
    tx_scroll: usize,
    account_scroll: usize,
    budget_scroll: usize,
    /// Whether the list of keys is up.
    show_help: bool,
    /// What the pointer is over, so it can be drawn lit.
    hover: Option<Target>,
    /// Every box the last paint recorded, for hover and the wheel.
    last_hits: Vec<(Target, Rect)>,
    /// The wheel's remainder.
    wheel: wheel::Accumulator,
    /// Whether changes are kept. Off in `new`, so no test can write the
    /// user's ledger; `from_settings`, which `main` uses, turns it on.
    persist: bool,
    /// Why the ledger is not being kept, drawn for as long as it is true: it
    /// could not be read (and so is left exactly as it is), or the last save
    /// failed.
    ledger_error: Option<String>,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
}

/// Everything that decides whether a frame is worth drawing.
///
/// `handle_key` reports nothing about whether it did anything, so this is
/// compared around every event and the answer *is* `EventResult`. A field
/// missing from here is a change the user cannot see.
///
/// `search_query` was missing. Typing is safe -- that path answers `Consumed`
/// outright, ahead of this comparison -- but `Backspace` comes through
/// `handle_key`, changes only the query, and so answered `Ignored`: the search
/// bar draws the query with a caret after it, and the deleted character stayed
/// on screen. One half of an edit repainting and the other half not is worse
/// than neither, because it reads as the key having failed.
///
/// A struct rather than the tuple this was. Seven fields was still legible and
/// eight is where it stops being, and `apps/jsonviewer` and `apps/flashcards`
/// both reached the same shape the same day -- one because Rust implements
/// `PartialEq` for tuples only up to twelve, the other because clippy refused
/// eleven. A reader adding a field to a positional list has no way to check
/// they put it in the right place, which is how all three came to be missing
/// one.
#[derive(Clone, Debug, PartialEq)]
struct Fingerprint {
    screen: Screen,
    selected_id: Option<u32>,
    search_active: bool,
    /// What is in the search box, not merely that it is open.
    search_query: String,
    transactions: usize,
    category_filter: Option<Category>,
    year: u16,
    month: u8,
    form_open: bool,
    pending_delete: Option<Doomed>,
    selected_account: Option<u32>,
    selected_budget: usize,
    scrolls: (usize, usize, usize),
}

impl FinanceApp {
    fn new() -> Self {
        // The clock's day. It was 18 May 2026 in every run, so "this month"
        // was May for good and a new entry was dated then.
        let today = today_from_clock().unwrap_or(SimpleDate::new(1970, 1, 1));
        Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
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
            form: None,
            field: FormField::Date,
            form_error: None,
            clipboard: String::new(),
            pending_delete: None,
            selected_account: None,
            selected_budget: 0,
            tx_scroll: 0,
            account_scroll: 0,
            budget_scroll: 0,
            show_help: false,
            hover: None,
            last_hits: Vec::new(),
            wheel: wheel::Accumulator::default(),
            persist: false,
            ledger_error: None,
        }
    }

    /// The window's finances: the ledger kept last time, and every change
    /// kept from here on.
    fn from_settings() -> Self {
        let mut app = Self::new();
        app.persist = true;
        match ledger_path() {
            Some(path) => app.load_ledger(&path),
            None => app.ledger_error = Some(String::from(NO_HOME)),
        }
        app
    }

    /// Read the ledger at `path`; with none there yet, this is a first run.
    ///
    /// One that cannot be read whole is left exactly as it is: nothing is
    /// saved over it, and the window says so for as long as it is open. A
    /// save would write back only what was understood.
    fn load_ledger(&mut self, path: &std::path::Path) {
        let refused = |why: String| {
            format!(
                "{} was not read ({why}), so nothing is saved over it",
                path.display()
            )
        };
        let read = match safeio::read_to_string_capped(path, MAX_LEDGER_BYTES) {
            Ok(read) => read,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
            Err(err) => {
                self.persist = false;
                self.ledger_error = Some(refused(err.to_string()));
                return;
            }
        };
        if read.truncated {
            self.persist = false;
            self.ledger_error = Some(refused(format!(
                "it is larger than {} MiB",
                MAX_LEDGER_BYTES / (1024 * 1024)
            )));
            return;
        }
        match parse_ledger(&read.text) {
            Ok(ledger) => {
                self.next_account_id = ledger
                    .accounts
                    .iter()
                    .map(|a| a.id.saturating_add(1))
                    .max()
                    .unwrap_or(1);
                self.next_tx_id = ledger
                    .transactions
                    .iter()
                    .map(|t| t.id.saturating_add(1))
                    .max()
                    .unwrap_or(1);
                self.selected_account = ledger.accounts.first().map(|a| a.id);
                self.accounts = ledger.accounts;
                self.budgets = ledger.budgets;
                self.transactions = ledger.transactions;
            }
            Err(why) => {
                self.persist = false;
                self.ledger_error = Some(refused(why));
            }
        }
    }

    /// Keep the ledger as it is now, if this window keeps anything.
    fn save_ledger(&mut self) {
        if !self.persist {
            return;
        }
        let Some(path) = ledger_path() else {
            self.ledger_error = Some(String::from(NO_HOME));
            return;
        };
        let text = ledger_text(&self.accounts, &self.budgets, &self.transactions);
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| safeio::write_str_atomically(&path, &text));
        self.ledger_error = match written {
            Ok(()) => None,
            Err(err) => Some(format!("Not saved to {}: {err}", path.display())),
        };
    }

    /// Where what is entered goes, for the first-run card.
    fn keeping_line(&self) -> String {
        match ledger_path() {
            Some(path) if self.persist && self.ledger_error.is_none() => {
                format!("What you enter is kept in {}.", path.display())
            }
            _ => String::from("Nothing you enter here is kept."),
        }
    }

    /// An app holding the accounts and transactions `new` used to invent.
    ///
    /// `#[cfg(test)]`. Most of this app's tests are about the ledger, the
    /// budget arithmetic, the category filter, the month view and the search
    /// -- all of which need *transactions*, not specifically invented ones.
    ///
    /// Dated as the app used to date every run, 18 May 2026, so the month on
    /// screen is the month the sample was entered in whatever the clock says.
    #[cfg(test)]
    fn with_sample_data() -> Self {
        let mut app = Self::new();
        app.current_date = SimpleDate::new(2026, 5, 18);
        app.view_month = SimpleDate::new(2026, 5, 1);
        app.create_sample_data();
        app
    }

    /// Accounts, budgets and transactions, for tests.
    ///
    /// `#[cfg(test)]` since 2026-09-15. `new` called it, so the app opened on
    /// a "Main Checking" holding 3,500, a "Savings" holding 12,000, seven
    /// budget lines and a month of transactions including a 5,000 salary.
    ///
    /// Nobody would mistake these for their own accounts on sight. The harm is
    /// downstream: the moment the user adds a real transaction, every total
    /// this app computes -- net worth, budget remaining, category spend -- is
    /// summed over their figure *and* the invented ones, and the answer looks
    /// like arithmetic rather than like a mistake.
    #[cfg(test)]
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

    /// A new account, by the account form.
    pub fn add_account(&mut self, name: &str, atype: AccountType, initial: i64) -> u32 {
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
    /// A new transaction, by the transaction form.
    pub fn add_transaction(
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
        // Its place in the list on screen -- not in `transactions`, which is
        // in the order things were entered. The two used to be confused, so
        // with a filter on, the selection after a delete jumped.
        let at = self
            .visible_ids()
            .iter()
            .position(|v| *v == id)
            .unwrap_or(0);
        self.transactions.remove(idx);
        // Prefer the row that took its place, then the one before it; the point
        // is that repeated deletes walk down the list rather than jumping to
        // the end or landing on nothing.
        let visible = self.visible_ids();
        self.selected_id = visible
            .get(at)
            .or_else(|| visible.get(at.saturating_sub(1)))
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
        // Not on screen (the filter just changed under it): the first visible
        // row is where the selection belongs, whichever way the user pressed.
        // Otherwise the move stops at either end, so a page from near the top
        // lands on the top rather than refusing to move at all.
        let current = self
            .selected_id
            .and_then(|id| visible.iter().position(|&v| v == id));
        let next = step_index(current, delta, visible.len());
        self.selected_id = next.and_then(|i| visible.get(i)).copied();
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

    /// A category's monthly budget; zero takes it off.
    fn set_budget(&mut self, category: Category, monthly_limit: i64) {
        if monthly_limit <= 0 {
            self.budgets.retain(|b| b.category != category);
            return;
        }
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

    /// The transactions the list shows, newest first: the month in the
    /// header, or -- while there is a search -- every month, so "when did I
    /// last pay the plumber" has an answer.
    ///
    /// It showed every month in the order things were typed in, under a
    /// header naming one month whose arrows changed nothing in the list.
    fn filtered_transactions(&self) -> Vec<(usize, &Transaction)> {
        let query = self.search_query.to_lowercase();
        let mut rows: Vec<(usize, &Transaction)> = self
            .transactions
            .iter()
            .enumerate()
            .filter(|(_, tx)| {
                if let Some(cat) = self.category_filter
                    && tx.category != cat
                {
                    return false;
                }
                if query.is_empty() {
                    return tx.date.same_month(&self.view_month);
                }
                tx.description.to_lowercase().contains(&query)
                    || tx.notes.to_lowercase().contains(&query)
            })
            .collect();
        rows.sort_by(|(_, a), (_, b)| b.date.cmp(&a.date).then(b.id.cmp(&a.id)));
        rows
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

    // ── Forms ───────────────────────────────────────────────────────

    /// A new transaction, dated today, in the chosen account (or the first).
    /// With no account there is nowhere for one to go, so the account form
    /// comes up instead, saying why.
    fn open_new_transaction(&mut self) {
        let Some(account) = self
            .selected_account
            .filter(|id| self.accounts.iter().any(|a| a.id == *id))
            .or_else(|| self.accounts.first().map(|a| a.id))
        else {
            self.open_new_account();
            self.form_error = Some(String::from(
                "Add an account first: a transaction goes into one",
            ));
            return;
        };
        let mut date = TextInput::new();
        date.set_text(&self.current_date.format());
        self.form = Some(Form::Transaction {
            id: None,
            date,
            description: TextInput::new(),
            amount: TextInput::new(),
            notes: TextInput::new(),
            income: false,
            category: Category::Food,
            account: Some(account),
            recurring: false,
        });
        self.field = FormField::Description;
        self.form_error = None;
    }

    /// Change transaction `id`.
    fn open_edit_transaction(&mut self, id: u32) {
        let Some(tx) = self.transactions.iter().find(|t| t.id == id) else {
            return;
        };
        let text = |s: &str| {
            let mut input = TextInput::new();
            input.set_text(s);
            input
        };
        let magnitude = Self::format_currency(tx.amount.saturating_abs());
        self.form = Some(Form::Transaction {
            id: Some(id),
            date: text(&tx.date.format()),
            description: text(&tx.description),
            amount: text(magnitude.trim_start_matches('$')),
            notes: text(&tx.notes),
            income: tx.amount > 0,
            category: tx.category,
            account: Some(tx.account_id),
            recurring: tx.recurring,
        });
        self.field = FormField::Description;
        self.form_error = None;
    }

    fn open_new_account(&mut self) {
        self.form = Some(Form::Account {
            id: None,
            name: TextInput::new(),
            kind: AccountType::Checking,
            opening: TextInput::new(),
        });
        self.field = FormField::Name;
        self.form_error = None;
    }

    fn open_edit_account(&mut self, id: u32) {
        let Some(account) = self.accounts.iter().find(|a| a.id == id) else {
            return;
        };
        let mut name = TextInput::new();
        name.set_text(&account.name);
        let mut opening = TextInput::new();
        opening.set_text(
            Self::format_currency(account.initial_balance)
                .replace('$', "")
                .as_str(),
        );
        self.form = Some(Form::Account {
            id: Some(id),
            name,
            kind: account.account_type,
            opening,
        });
        self.field = FormField::Name;
        self.form_error = None;
    }

    /// Set the budget of the category at `index` in `EXPENSE_CATS`.
    fn open_budget(&mut self, index: usize) {
        let Some(&category) = Category::EXPENSE_CATS.get(index) else {
            return;
        };
        let mut limit = TextInput::new();
        if let Some(b) = self.budgets.iter().find(|b| b.category == category) {
            limit.set_text(Self::format_currency(b.monthly_limit).trim_start_matches('$'));
        }
        self.form = Some(Form::Budget { category, limit });
        self.field = FormField::Limit;
        self.form_error = None;
    }

    /// Keep what the form holds, or say in the form what is wrong with it.
    fn save_form(&mut self) {
        let Some(form) = self.form.clone() else {
            return;
        };
        let result = match form {
            Form::Transaction {
                id,
                date,
                description,
                amount,
                notes,
                income,
                category,
                account,
                recurring,
            } => self.save_transaction(
                id,
                &date,
                &description,
                &amount,
                &notes,
                income,
                category,
                account,
                recurring,
            ),
            Form::Account {
                id,
                name,
                kind,
                opening,
            } => self.save_account(id, &name, kind, &opening),
            Form::Budget { category, limit } => match parse_cents(limit.text(), false) {
                Ok(cents) => {
                    self.set_budget(category, cents);
                    self.status_msg = if cents > 0 {
                        format!("Budget for {} set", category.label())
                    } else {
                        format!("Budget for {} taken off", category.label())
                    };
                    Ok(())
                }
                Err(why) => Err(why),
            },
        };
        match result {
            Ok(()) => {
                self.form = None;
                self.form_error = None;
                self.after_change();
            }
            Err(why) => self.form_error = Some(why),
        }
    }

    // A transaction form's fields, read and checked; grouped by nothing but
    // being one form, so they arrive as themselves.
    #[allow(clippy::too_many_arguments)]
    fn save_transaction(
        &mut self,
        id: Option<u32>,
        date: &TextInput,
        description: &TextInput,
        amount: &TextInput,
        notes: &TextInput,
        income: bool,
        category: Category,
        account: Option<u32>,
        recurring: bool,
    ) -> Result<(), String> {
        let Some(date) = SimpleDate::parse(date.text()) else {
            return Err(String::from("That is not a date; write it YYYY-MM-DD"));
        };
        let description = description.text().trim().to_owned();
        if description.is_empty() {
            return Err(String::from("A transaction needs a description"));
        }
        let magnitude = parse_cents(amount.text(), false)?;
        if magnitude == 0 {
            return Err(String::from("The amount is zero"));
        }
        let Some(account) = account.filter(|a| self.accounts.iter().any(|x| x.id == *a)) else {
            return Err(String::from("Choose an account"));
        };
        let cents = if income {
            magnitude
        } else {
            magnitude.saturating_neg()
        };
        let notes = notes.text().trim().to_owned();
        match id {
            Some(id) => {
                let Some(tx) = self.transactions.iter_mut().find(|t| t.id == id) else {
                    return Err(String::from("That transaction is gone"));
                };
                tx.date = date;
                tx.description = description;
                tx.amount = cents;
                tx.category = category;
                tx.account_id = account;
                tx.notes = notes;
                tx.recurring = recurring;
                self.selected_id = Some(id);
                self.status_msg = String::from("Transaction changed");
            }
            None => {
                let id = self.add_transaction(
                    date,
                    &description,
                    cents,
                    category,
                    account,
                    &notes,
                    recurring,
                );
                self.selected_id = Some(id);
                self.status_msg = String::from("Transaction added");
            }
        }
        Ok(())
    }

    fn save_account(
        &mut self,
        id: Option<u32>,
        name: &TextInput,
        kind: AccountType,
        opening: &TextInput,
    ) -> Result<(), String> {
        let name = name.text().trim().to_owned();
        if name.is_empty() {
            return Err(String::from("An account needs a name"));
        }
        let opening = if opening.text().trim().is_empty() {
            0
        } else {
            parse_cents(opening.text(), true)?
        };
        match id {
            Some(id) => {
                let Some(account) = self.accounts.iter_mut().find(|a| a.id == id) else {
                    return Err(String::from("That account is gone"));
                };
                account.name.clone_from(&name);
                account.account_type = kind;
                account.initial_balance = opening;
                self.status_msg = format!("Account {name} changed");
            }
            None => {
                let id = self.add_account(&name, kind, opening);
                self.selected_account = Some(id);
                self.status_msg = format!("Account {name} added");
            }
        }
        Ok(())
    }

    /// Step a chosen field's value: forward, or back.
    fn step_choice(&mut self, which: FormField, forward: bool) -> bool {
        let accounts: Vec<u32> = self.accounts.iter().map(|a| a.id).collect();
        let Some(form) = self.form.as_mut() else {
            return false;
        };
        let step = |at: usize, len: usize| -> usize {
            if forward {
                at.saturating_add(1).checked_rem(len).unwrap_or(0)
            } else {
                at.checked_sub(1).unwrap_or(len.saturating_sub(1))
            }
        };
        match (form, which) {
            (
                Form::Transaction {
                    income, category, ..
                },
                FormField::Kind,
            ) => {
                *income = !*income;
                // Money in filed under Food, or money out under Income, is
                // never what was meant; the other categories go either way.
                if *income && Category::EXPENSE_CATS.contains(category) {
                    *category = Category::Income;
                } else if !*income && *category == Category::Income {
                    *category = Category::Food;
                }
            }
            (Form::Transaction { recurring, .. }, FormField::Recurring) => *recurring = !*recurring,
            (Form::Transaction { category, .. }, FormField::Category) => {
                let at = Category::ALL
                    .iter()
                    .position(|c| c == category)
                    .unwrap_or(0);
                *category = Category::ALL
                    .get(step(at, Category::ALL.len()))
                    .copied()
                    .unwrap_or(*category);
            }
            (Form::Transaction { account, .. }, FormField::Account) => {
                if accounts.is_empty() {
                    return false;
                }
                let at = account
                    .and_then(|id| accounts.iter().position(|a| *a == id))
                    .unwrap_or(0);
                *account = accounts.get(step(at, accounts.len())).copied();
            }
            (Form::Account { kind, .. }, FormField::AccountKind) => {
                let at = AccountType::ALL.iter().position(|k| k == kind).unwrap_or(0);
                *kind = AccountType::ALL
                    .get(step(at, AccountType::ALL.len()))
                    .copied()
                    .unwrap_or(*kind);
            }
            _ => return false,
        }
        true
    }

    /// Keys while a form is up: Tab between fields, Enter saves, Escape
    /// leaves, Left/Right/Space step a choice, the rest edit a text field.
    fn handle_form_key(&mut self, key: &KeyEvent) -> EventResult {
        let Some(form) = self.form.as_ref() else {
            return EventResult::Ignored;
        };
        let fields = form.fields();
        if !fields.contains(&self.field) {
            self.field = fields.first().copied().unwrap_or(FormField::Description);
        }
        match key.key {
            Key::Tab => {
                let at = fields.iter().position(|f| *f == self.field).unwrap_or(0);
                let next = if key.modifiers.shift {
                    at.checked_sub(1).unwrap_or(fields.len().saturating_sub(1))
                } else {
                    at.saturating_add(1).checked_rem(fields.len()).unwrap_or(0)
                };
                self.field = fields.get(next).copied().unwrap_or(self.field);
                EventResult::Consumed
            }
            Key::Enter => {
                self.save_form();
                EventResult::Consumed
            }
            Key::Escape => {
                self.form = None;
                self.form_error = None;
                self.status_msg = String::from("Cancelled");
                EventResult::Consumed
            }
            Key::Left | Key::Right | Key::Space if !self.field.is_text() => {
                if self.step_choice(self.field, key.key != Key::Left) {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => {
                let which = self.field;
                let clipboard = self.clipboard.clone();
                let Some(input) = self.form.as_mut().and_then(|f| f.input(which)) else {
                    return EventResult::Ignored;
                };
                let done = edit_line(input, key, 200, &clipboard);
                if let Some(copied) = done.copied {
                    self.clipboard = copied;
                }
                if done.handled {
                    self.form_error = None;
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
        }
    }

    // ── Deleting ────────────────────────────────────────────────────

    /// Ask before deleting what is chosen on this screen. A transaction went
    /// at once on Ctrl+D; an account could not be deleted at all.
    fn ask_to_delete(&mut self) {
        self.pending_delete = match self.screen {
            Screen::Accounts => self
                .selected_account
                .filter(|id| self.accounts.iter().any(|a| a.id == *id))
                .map(Doomed::Account),
            // Only a row the user can see: Ctrl+D on the dashboard deleted a
            // transaction chosen on another screen, unseen.
            Screen::Transactions => self.chosen_transaction().map(Doomed::Transaction),
            Screen::Dashboard | Screen::Budgets | Screen::Reports => None,
        };
    }

    /// The chosen transaction, if the list on screen shows it.
    fn chosen_transaction(&self) -> Option<u32> {
        self.selected_id
            .filter(|id| self.visible_ids().contains(id))
    }

    /// Change what is chosen on this screen.
    fn edit_chosen(&mut self) {
        match self.screen {
            Screen::Transactions => {
                if let Some(id) = self.chosen_transaction() {
                    self.open_edit_transaction(id);
                }
            }
            Screen::Accounts => {
                if let Some(id) = self.selected_account {
                    self.open_edit_account(id);
                }
            }
            Screen::Budgets => self.open_budget(self.selected_budget),
            Screen::Dashboard | Screen::Reports => {}
        }
    }

    /// Show transaction `id` in the transactions screen, chosen: a press on
    /// the dashboard's recent list.
    fn show_transaction(&mut self, id: u32) {
        let Some(tx) = self.transactions.iter().find(|t| t.id == id) else {
            return;
        };
        let month = SimpleDate::new(tx.date.year, tx.date.month, 1);
        self.screen = Screen::Transactions;
        if !self.visible_ids().contains(&id) {
            // A filter or a search would hide it: show its month, whole.
            self.category_filter = None;
            self.search_query.clear();
            self.search_active = false;
            self.view_month = month;
        }
        self.selected_id = Some(id);
        self.keep_lists_in_view();
    }

    /// The next category the list shows, and then all of them again.
    fn cycle_category_filter(&mut self) {
        // `get` on the next position rather than an index guarded by a
        // separate length test: running off the end is how the cycle returns
        // to "All", so it is the normal path and not an error.
        self.category_filter = match self.category_filter {
            None => Category::ALL.first().copied(),
            Some(cat) => Category::ALL
                .iter()
                .position(|&c| c == cat)
                .and_then(|idx| idx.checked_add(1))
                .and_then(|next| Category::ALL.get(next))
                .copied(),
        };
        self.status_msg = match self.category_filter {
            Some(cat) => format!("Showing {}", cat.label()),
            None => String::from("Showing every category"),
        };
        self.reanchor_selection();
    }

    fn delete_doomed(&mut self, doomed: Doomed) {
        match doomed {
            Doomed::Transaction(id) => self.delete_transaction(id),
            Doomed::Account(id) => {
                let Some(pos) = self.accounts.iter().position(|a| a.id == id) else {
                    return;
                };
                let gone = self.accounts.remove(pos);
                let before = self.transactions.len();
                self.transactions.retain(|t| t.account_id != id);
                self.status_msg = format!(
                    "Deleted {} and its {} transaction(s)",
                    gone.name,
                    before.saturating_sub(self.transactions.len())
                );
                self.selected_account = self
                    .accounts
                    .get(pos)
                    .or_else(|| self.accounts.last())
                    .map(|a| a.id);
                self.reanchor_selection();
            }
        }
        self.after_change();
    }

    /// What every change to the ledger is followed by: it is kept at once,
    /// so closing the window never loses anything.
    fn after_change(&mut self) {
        self.keep_lists_in_view();
        self.save_ledger();
    }

    // ── The lists ───────────────────────────────────────────────────

    /// A list's pane in the content area from `top` down, `margin` in from
    /// each side, and how many whole rows of `row_h` it holds.
    fn list_pane(&self, top: f32, row_h: f32, margin: f32) -> (Rect, usize) {
        let pane = Rect::new(
            self.content_x() + margin,
            top,
            (self.content_w() - 2.0 * margin).max(0.0),
            (self.content_bottom() - top).max(0.0),
        );
        (pane, ((pane.h / row_h).floor().max(1.0)) as usize)
    }

    /// Where the transaction rows are drawn: under the toolbar and the column
    /// heads.
    fn tx_pane(&self) -> (Rect, usize) {
        self.list_pane(self.content_y() + 80.0, TX_ROW_H, 8.0)
    }

    /// Where the account rows are drawn: under the title row.
    fn account_pane(&self) -> (Rect, usize) {
        self.list_pane(self.content_y() + 56.0, ACCOUNT_ROW_H, 16.0)
    }

    /// Where the budget rows are drawn: under the title row.
    fn budget_pane(&self) -> (Rect, usize) {
        self.list_pane(self.content_y() + 56.0, BUDGET_ROW_H, 16.0)
    }

    /// Scroll each list so what is chosen in it is on screen, and no list
    /// past its end.
    fn keep_lists_in_view(&mut self) {
        let follow = |scroll: &mut usize, at: Option<usize>, count: usize, visible: usize| {
            if let Some(at) = at {
                if at < *scroll {
                    *scroll = at;
                } else if at >= scroll.saturating_add(visible) {
                    *scroll = at.saturating_add(1).saturating_sub(visible);
                }
            }
            *scroll = (*scroll).min(count.saturating_sub(visible));
        };
        let visible_ids = self.visible_ids();
        let (_, tx_rows) = self.tx_pane();
        let at = self
            .selected_id
            .and_then(|id| visible_ids.iter().position(|v| *v == id));
        follow(&mut self.tx_scroll, at, visible_ids.len(), tx_rows);
        let (_, account_rows) = self.account_pane();
        let at = self
            .selected_account
            .and_then(|id| self.accounts.iter().position(|a| a.id == id));
        let accounts = self.accounts.len();
        follow(&mut self.account_scroll, at, accounts, account_rows);
        let (_, budget_rows) = self.budget_pane();
        follow(
            &mut self.budget_scroll,
            Some(self.selected_budget),
            Category::EXPENSE_CATS.len(),
            budget_rows,
        );
    }

    /// No list scrolled past its end, as a resize can leave one; unlike
    /// `keep_lists_in_view`, leaves the wheel's position alone otherwise.
    fn clamp_scrolls(&mut self) {
        let tx_last = self.visible_ids().len().saturating_sub(self.tx_pane().1);
        self.tx_scroll = self.tx_scroll.min(tx_last);
        let account_last = self.accounts.len().saturating_sub(self.account_pane().1);
        self.account_scroll = self.account_scroll.min(account_last);
        let budget_last = Category::EXPENSE_CATS
            .len()
            .saturating_sub(self.budget_pane().1);
        self.budget_scroll = self.budget_scroll.min(budget_last);
    }

    /// Up or Down on the list this screen shows.
    fn move_in_list(&mut self, delta: isize) {
        match self.screen {
            Screen::Accounts => {
                let ids: Vec<u32> = self.accounts.iter().map(|a| a.id).collect();
                let at = self
                    .selected_account
                    .and_then(|id| ids.iter().position(|v| *v == id));
                let next = step_index(at, delta, ids.len());
                self.selected_account = next.and_then(|i| ids.get(i)).copied();
            }
            Screen::Budgets => {
                if let Some(next) = step_index(
                    Some(self.selected_budget),
                    delta,
                    Category::EXPENSE_CATS.len(),
                ) {
                    self.selected_budget = next;
                }
            }
            Screen::Transactions => self.move_selection(delta),
            // Nothing is chosen on these two, so there is nothing to move.
            Screen::Dashboard | Screen::Reports => {}
        }
        self.keep_lists_in_view();
    }

    /// Page Down and Page Up: a page of the list on screen.
    fn page(&mut self, down: bool) {
        let rows = match self.screen {
            Screen::Accounts => self.account_pane().1,
            Screen::Budgets => self.budget_pane().1,
            _ => self.tx_pane().1,
        };
        let delta = isize::try_from(rows).unwrap_or(1);
        self.move_in_list(if down { delta } else { delta.saturating_neg() });
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
                // Out of the box with the search kept, so the rows it found
                // can be walked.
                "Return" => self.search_active = false,
                _ => {}
            }
            self.keep_lists_in_view();
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
            "Up" | "k" => self.move_in_list(-1),
            "Down" | "j" => self.move_in_list(1),
            "PageUp" => self.page(false),
            "PageDown" => self.page(true),
            "/" => {
                self.screen = Screen::Transactions;
                self.search_active = true;
                self.search_query.clear();
            }
            "c" | "C" => self.cycle_category_filter(),
            "n" | "N" if !ctrl => {
                if self.screen == Screen::Accounts {
                    self.open_new_account();
                } else {
                    self.open_new_transaction();
                }
            }
            "Return" => self.edit_chosen(),
            "Delete" => self.ask_to_delete(),
            "d" if ctrl => self.ask_to_delete(),
            _ => {}
        }
        self.keep_lists_in_view();
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

    fn format_currency_colored(cents: i64, pal: &Palette) -> (String, Color) {
        let text = Self::format_currency(cents);
        // Inked here: an amount is always written, never filled, and all
        // four callers draw it as text. `text` is floored in the palette
        // already and `ink` leaves it alone. 837;
        // `gui/appearance/colour-methods.py`.
        let color = match cents.cmp(&0) {
            std::cmp::Ordering::Greater => pal.ink(pal.green),
            std::cmp::Ordering::Less => pal.ink(pal.red),
            std::cmp::Ordering::Equal => pal.text,
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
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            // Midnight, or near enough to check.
            Event::Tick { .. } => {
                let Some(today) = today_from_clock() else {
                    return EventResult::Ignored;
                };
                if today == self.current_date {
                    return EventResult::Ignored;
                }
                // The view follows the month if it was on this one.
                if self.view_month.same_month(&self.current_date) {
                    self.view_month = SimpleDate::new(today.year, today.month, 1);
                }
                self.current_date = today;
                EventResult::Consumed
            }
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
        // The list of keys, from anywhere; modal while it is up.
        if key.key == Key::F1 {
            self.show_help = !self.show_help;
            return EventResult::Consumed;
        }
        if self.show_help {
            if matches!(key.key, Key::Escape | Key::Enter) {
                self.show_help = false;
                return EventResult::Consumed;
            }
            return EventResult::Ignored;
        }
        // A delete waiting on its answer takes the next key; only Y deletes.
        if let Some(doomed) = self.pending_delete.take() {
            let yes = key
                .single_char()
                .map_or(key.key == Key::Y, |c| c.eq_ignore_ascii_case(&'y'));
            if yes {
                self.delete_doomed(doomed);
            } else {
                self.status_msg = String::from("Kept");
            }
            return EventResult::Consumed;
        }
        // A form takes every key, or a `1` in an amount would switch screens.
        if self.form.is_some() {
            return self.handle_form_key(key);
        }

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
            Key::PageUp => "PageUp",
            Key::PageDown => "PageDown",
            Key::Home => "Home",
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
                // which is how the shortcuts below are written -- or, with no
                // text on it (as a keystroke built from its key alone
                // arrives), the character its key types.
                let typed = key
                    .text
                    .chars()
                    .next()
                    .or_else(|| key_char(key.key, key.modifiers.shift))?;
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
    fn state_fingerprint(&self) -> Fingerprint {
        Fingerprint {
            screen: self.screen,
            selected_id: self.selected_id,
            search_active: self.search_active,
            search_query: self.search_query.clone(),
            transactions: self.transactions.len(),
            category_filter: self.category_filter,
            year: self.view_month.year,
            month: self.view_month.month,
            form_open: self.form.is_some(),
            pending_delete: self.pending_delete,
            selected_account: self.selected_account,
            selected_budget: self.selected_budget,
            scrolls: (self.tx_scroll, self.account_scroll, self.budget_scroll),
        }
    }

    // ── Rendering ───────────────────────────────────────────────────

    /// Named `render_commands` and not `render`: at equal arity an inherent
    /// method silently wins method lookup over `oswindow::app::App::render`,
    /// so an app that keeps the name draws nothing and says nothing about it.
    ///
    /// For the tests: the window draws `frame()`, whose boxes it keeps.
    #[cfg(test)]
    fn render_commands(&self) -> Vec<RenderCommand> {
        self.frame().into_tree().commands
    }

    /// The area the screens draw in: right of the sidebar, below the header,
    /// above the status bar.
    fn content_rect(&self) -> Rect {
        Rect::new(
            self.content_x(),
            self.content_y(),
            self.content_w(),
            self.content_h(),
        )
    }

    /// Draw the window, recording every control where it is drawn: both the
    /// picture and the hit test.
    fn frame(&self) -> Frame<Target> {
        let (w, h) = (self.width, self.height);
        let mut f = Frame::new(w, h);
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });
        self.render_sidebar(&mut f);
        self.render_header(&mut f);
        // Each screen is held to its area: the lists ran on under the status
        // bar, and the dashboard's last section off the bottom of the window.
        f.clip(self.content_rect());
        match self.screen {
            Screen::Dashboard => self.render_dashboard(&mut f),
            Screen::Transactions => self.render_transactions(&mut f),
            Screen::Budgets => self.render_budgets(&mut f),
            Screen::Accounts => self.render_accounts(&mut f),
            Screen::Reports => self.render_reports(&mut f),
        }
        f.unclip();
        self.render_status(&mut f);
        if let Some(form) = &self.form {
            self.render_form(&mut f, form);
        }
        if let Some(doomed) = self.pending_delete {
            self.render_question(&mut f, doomed);
        }
        if self.show_help {
            guitk::shortcut::render_card(
                &mut f,
                &self.palette,
                (w, h),
                Self::HEADER_H,
                SHORTCUTS,
                "F1 closes this",
            );
            f.hit(Target::HelpCard, Rect::new(0.0, 0.0, w, h));
        }
        f
    }

    /// A button, lit while the pointer is on it; one with nothing to do is
    /// drawn dim and records no box.
    fn button(
        &self,
        f: &mut Frame<Target>,
        rect: Rect,
        label: &str,
        target: Target,
        enabled: bool,
    ) {
        let lit = enabled && self.hover == Some(target);
        f.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: if lit {
                self.palette.surface2
            } else {
                self.palette.surface1
            },
            corner_radii: CornerRadii::all(6.0),
        });
        f.push(RenderCommand::Text {
            x: guitk::text::center_x(label, rect.x + rect.w / 2.0, 12.0, FontWeightHint::Regular)
                .max(rect.x + 4.0),
            y: rect.y + (rect.h - 12.0) / 2.0,
            text: label.to_string(),
            font_size: 12.0,
            color: if enabled {
                self.palette.text
            } else {
                self.palette.overlay0
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some((rect.w - 8.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        if enabled {
            f.hit(target, rect);
        }
    }

    /// Buttons ending at `right`, each as wide as it says, in the order
    /// named.
    fn buttons_to(
        &self,
        f: &mut Frame<Target>,
        right: f32,
        y: f32,
        buttons: &[(&str, f32, Target, bool)],
    ) {
        let total: f32 = buttons.iter().map(|(_, w, _, _)| w + 6.0).sum();
        let mut x = right - (total - 6.0).max(0.0);
        for (label, width, target, enabled) in buttons {
            self.button(f, Rect::new(x, y, *width, 28.0), label, *target, *enabled);
            x += width + 6.0;
        }
    }

    /// A title at the top left of a screen.
    fn screen_title(f: &mut Frame<Target>, x: f32, y: f32, text: String, color: Color) {
        f.push(RenderCommand::Text {
            x,
            y,
            text,
            font_size: 18.0,
            color,
            font_weight: FontWeightHint::Bold,
            max_width: Some(420.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// A section's heading.
    fn heading(&self, f: &mut Frame<Target>, x: f32, y: f32, text: &str, width: f32) {
        f.push(RenderCommand::Text {
            x,
            y,
            text: text.to_string(),
            font_size: 15.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width.max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// A line of quiet text.
    fn note(&self, f: &mut Frame<Target>, x: f32, y: f32, text: String, width: f32) {
        f.push(RenderCommand::Text {
            x,
            y,
            text,
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width.max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Text whose right edge is at `right`.
    fn right_text(
        f: &mut Frame<Target>,
        right: f32,
        y: f32,
        text: String,
        (size, weight, color): (f32, FontWeightHint, Color),
    ) {
        f.push(RenderCommand::Text {
            x: guitk::text::right_x(&text, right, size, weight),
            y,
            text,
            font_size: size,
            color,
            font_weight: weight,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    /// A progress bar `usage` full, red past the limit and yellow near it.
    fn usage_bar(&self, f: &mut Frame<Target>, rect: Rect, usage: f32) {
        f.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: self.palette.surface2,
            corner_radii: CornerRadii::all(rect.h / 2.0),
        });
        let fill = (rect.w * usage.min(1.0)).max(0.0);
        if fill > 0.0 {
            f.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: fill,
                height: rect.h,
                color: if usage > 1.0 {
                    self.palette.red
                } else if usage > 0.8 {
                    self.palette.yellow
                } else {
                    self.palette.green
                },
                corner_radii: CornerRadii::all(rect.h / 2.0),
            });
        }
    }

    /// A thumb at the right edge of `pane` when the list is longer than it.
    fn scroll_thumb(
        &self,
        f: &mut Frame<Target>,
        pane: Rect,
        count: usize,
        visible: usize,
        scroll: usize,
    ) {
        if count <= visible {
            return;
        }
        let h = (pane.h * visible as f32 / count as f32).clamp(16.0_f32.min(pane.h), pane.h);
        let last = count.saturating_sub(visible).max(1);
        let y = pane.y + (pane.h - h) * (scroll.min(last) as f32 / last as f32);
        f.push(RenderCommand::FillRect {
            x: pane.right() - 5.0,
            y,
            width: 4.0,
            height: h,
            color: self.palette.surface2,
            corner_radii: CornerRadii::all(2.0),
        });
    }

    /// The sidebar entry for the `i`th screen.
    fn nav_rect(i: usize) -> Rect {
        Rect::new(8.0, 52.0 + i as f32 * 40.0, Self::SIDEBAR_W - 16.0, 36.0)
    }

    fn render_sidebar(&self, f: &mut Frame<Target>) {
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: Self::SIDEBAR_W,
            height: self.height,
            color: self.palette.crust,
            corner_radii: CornerRadii::ZERO,
        });
        f.push(RenderCommand::Text {
            x: 16.0,
            y: 16.0,
            text: String::from("\u{1F4B0} Finance"),
            font_size: 18.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(Self::SIDEBAR_W - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
        for (i, screen) in Screen::ALL.iter().enumerate() {
            let rect = Self::nav_rect(i);
            let target = Target::Screen(*screen);
            let is_active = *screen == self.screen;
            f.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: rect.w,
                height: rect.h,
                color: if is_active {
                    self.palette.surface1
                } else if self.hover == Some(target) {
                    self.palette.surface0
                } else {
                    Color::rgba(0, 0, 0, 0)
                },
                corner_radii: CornerRadii::all(6.0),
            });
            f.push(RenderCommand::Text {
                x: rect.x + 12.0,
                y: rect.y + 9.0,
                text: format!("{} {}", i.saturating_add(1), screen.label()),
                font_size: 13.0,
                color: if is_active {
                    self.palette.ink(self.palette.blue)
                } else {
                    self.palette.subtext0
                },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(rect.w - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            f.hit(target, rect);
        }
        let keys = Self::nav_rect(Screen::ALL.len());
        self.button(
            f,
            Rect::new(keys.x, keys.y + 6.0, keys.w, 28.0),
            "Keys  (F1)",
            Target::Help,
            true,
        );

        // The total, above the status bar that used to cover half of it.
        let total_y = self.height - Self::STATUS_H - 52.0;
        let (total_str, total_color) =
            Self::format_currency_colored(self.total_balance(), &self.palette);
        f.push(RenderCommand::Text {
            x: 16.0,
            y: total_y,
            text: String::from("Total Balance"),
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(Self::SIDEBAR_W - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
        f.push(RenderCommand::Text {
            x: 16.0,
            y: total_y + 18.0,
            text: total_str,
            font_size: 18.0,
            color: total_color,
            font_weight: FontWeightHint::Bold,
            max_width: Some(Self::SIDEBAR_W - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_header(&self, f: &mut Frame<Target>) {
        let x0 = Self::SIDEBAR_W;
        f.push(RenderCommand::FillRect {
            x: x0,
            y: 0.0,
            width: self.content_w(),
            height: Self::HEADER_H,
            color: self.palette.mantle,
            corner_radii: CornerRadii::ZERO,
        });

        // The month, between arrows that are buttons now: they were drawn
        // into the month's own text.
        self.button(
            f,
            Rect::new(x0 + 12.0, 10.0, 30.0, 30.0),
            "\u{25C0}",
            Target::MonthPrev,
            true,
        );
        f.push(RenderCommand::Text {
            x: x0 + 50.0,
            y: 14.0,
            text: format!("{} {}", self.view_month.month_label(), self.view_month.year),
            font_size: 18.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(150.0),
            overflow: TextOverflow::Ellipsis,
        });
        self.button(
            f,
            Rect::new(x0 + 204.0, 10.0, 30.0, 30.0),
            "\u{25B6}",
            Target::MonthNext,
            true,
        );
        self.button(
            f,
            Rect::new(x0 + 242.0, 10.0, 96.0, 30.0),
            "This month",
            Target::ThisMonth,
            !self.view_month.same_month(&self.current_date),
        );

        // The month's figures, as far right as there is room, never over the
        // month's controls.
        let income = self.month_income();
        let expenses = self.month_expenses();
        let savings = self.month_savings();
        let hx = (self.width - 460.0).max(x0 + 350.0);
        for (label, amount, color, offset) in [
            // The three figures are the whole of what this header says, so
            // each is inked. Income green, expenses red, savings teal or red
            // by sign -- all dual-use roles, all text.
            (
                "Income",
                income,
                self.palette.ink(self.palette.green),
                0.0_f32,
            ),
            (
                "Expenses",
                expenses,
                self.palette.ink(self.palette.red),
                150.0,
            ),
            (
                "Savings",
                savings,
                if savings >= 0 {
                    self.palette.ink(self.palette.teal)
                } else {
                    self.palette.ink(self.palette.red)
                },
                300.0,
            ),
        ] {
            f.push(RenderCommand::Text {
                x: hx + offset,
                y: 6.0,
                text: label.to_string(),
                font_size: 10.0,
                color: self.palette.subtext0,
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
            f.push(RenderCommand::Text {
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

    fn render_dashboard(&self, f: &mut Frame<Target>) {
        let cx = self.content_x() + 16.0;
        let cy = self.content_y() + 16.0;
        let cw = (self.content_w() - 32.0).max(0.0);
        if self.accounts.is_empty() {
            self.render_first_run(f, cx, cy, cw);
            return;
        }
        Self::screen_title(f, cx, cy, String::from("Overview"), self.palette.text);
        self.buttons_to(
            f,
            cx + cw,
            cy - 4.0,
            &[
                ("+ Transaction", 120.0, Target::NewTransaction, true),
                ("+ Account", 100.0, Target::NewAccount, true),
            ],
        );
        let top = cy + 40.0;
        let col_w = ((cw - 24.0) / 2.0).max(0.0);
        self.render_budget_overview(f, cx, top, col_w);
        let right_x = cx + col_w + 24.0;
        let recent_top = self.render_top_spending(f, right_x, top, col_w);
        self.render_recent(f, right_x, recent_top, col_w);
    }

    /// What the dashboard says before there is anything to show: why it is
    /// empty, and the one control that begins.
    fn render_first_run(&self, f: &mut Frame<Target>, cx: f32, cy: f32, cw: f32) {
        let card = Rect::new(cx, cy, cw, 128.0);
        self.palette
            .push_surface(f, card.x, card.y, card.w, card.h, 8.0, Surface::Card);
        let keeping = self.keeping_line();
        let lines = NO_DATA_LINES
            .iter()
            .copied()
            .chain(std::iter::once(keeping.as_str()));
        for (i, line) in lines.enumerate() {
            f.push(RenderCommand::Text {
                x: card.x + 16.0,
                y: card.y + 14.0 + i as f32 * 22.0,
                text: line.to_string(),
                color: if i == 0 {
                    self.palette.text
                } else {
                    self.palette.subtext0
                },
                font_size: if i == 0 { 15.0 } else { 12.0 },
                font_weight: if i == 0 {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some((card.w - 32.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
        self.button(
            f,
            Rect::new(card.x + 16.0, card.bottom() - 40.0, 120.0, 28.0),
            "+ Account",
            Target::NewAccount,
            true,
        );
    }

    /// Each budget set, in the order the categories are listed, with how
    /// much of it is gone.
    fn render_budget_overview(&self, f: &mut Frame<Target>, x: f32, top: f32, w: f32) {
        self.heading(f, x, top, "Budgets", w);
        if self.budgets.is_empty() {
            self.note(
                f,
                x,
                top + 28.0,
                String::from("None set. The budgets screen (3) takes one per category."),
                w,
            );
            return;
        }
        let mut y = top + 28.0;
        for cat in Category::EXPENSE_CATS {
            let Some(budget) = self.budgets.iter().find(|b| b.category == cat) else {
                continue;
            };
            let spent = self.category_spending(cat);
            f.push(RenderCommand::Text {
                x,
                y,
                text: format!("{} {}", cat.icon(), cat.label()),
                font_size: 12.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some((w - 150.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            Self::right_text(
                f,
                x + w,
                y + 1.0,
                format!(
                    "{} / {}",
                    Self::format_currency(spent),
                    Self::format_currency(budget.monthly_limit)
                ),
                (11.0, FontWeightHint::Regular, self.palette.subtext0),
            );
            self.usage_bar(
                f,
                Rect::new(x, y + 20.0, w, 8.0),
                Self::usage_ratio(spent, budget.monthly_limit),
            );
            y += 40.0;
        }
    }

    /// The month's five largest categories of spending; answers where the
    /// section below it starts.
    fn render_top_spending(&self, f: &mut Frame<Target>, x: f32, top: f32, w: f32) -> f32 {
        self.heading(f, x, top, "Top spending", w);
        let top_cats = self.top_expense_categories();
        if top_cats.is_empty() {
            self.note(
                f,
                x,
                top + 28.0,
                String::from("Nothing spent this month."),
                w,
            );
            return top + 28.0 + 30.0 + 16.0;
        }
        let max_amount = top_cats.first().map_or(1, |(_, a)| *a).max(1);
        let shown = top_cats.len().min(5);
        let bar_room = (w - 136.0 - 90.0).max(0.0);
        for (i, (cat, amount)) in top_cats.iter().take(5).enumerate() {
            let ry = top + 28.0 + i as f32 * 30.0;
            let bar_w = bar_room * (*amount as f32 / max_amount as f32);
            f.push(RenderCommand::Text {
                x,
                y: ry + 3.0,
                text: format!("{} {}", cat.icon(), cat.label()),
                font_size: 12.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some(130.0),
                overflow: TextOverflow::Ellipsis,
            });
            f.push(RenderCommand::FillRect {
                x: x + 136.0,
                y: ry,
                width: bar_w.max(4.0),
                height: 20.0,
                color: cat.color(&self.palette),
                corner_radii: CornerRadii::all(4.0),
            });
            f.push(RenderCommand::Text {
                x: x + 142.0 + bar_w,
                y: ry + 3.0,
                text: Self::format_currency(*amount),
                font_size: 11.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(90.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
        top + 28.0 + shown as f32 * 30.0 + 16.0
    }

    /// The month's latest transactions, as many as fit; a press shows one in
    /// the transactions screen.
    fn render_recent(&self, f: &mut Frame<Target>, x: f32, top: f32, w: f32) {
        self.heading(f, x, top, "Recent transactions", w);
        let mut recent = self.month_transactions();
        recent.sort_by(|a, b| b.date.cmp(&a.date).then(b.id.cmp(&a.id)));
        if recent.is_empty() {
            self.note(
                f,
                x,
                top + 28.0,
                String::from("None this month. N adds one."),
                w,
            );
            return;
        }
        let mut ry = top + 28.0;
        for tx in recent {
            if ry + 26.0 > self.content_bottom() {
                break;
            }
            let row = Rect::new(x - 4.0, ry - 4.0, w + 8.0, 26.0);
            let target = Target::RecentRow(tx.id);
            if self.hover == Some(target) {
                f.push(RenderCommand::FillRect {
                    x: row.x,
                    y: row.y,
                    width: row.w,
                    height: row.h,
                    color: self.palette.surface0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }
            f.push(RenderCommand::Text {
                x,
                y: ry + 1.0,
                text: tx.date.format(),
                font_size: 11.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(80.0),
                overflow: TextOverflow::Ellipsis,
            });
            f.push(RenderCommand::Text {
                x: x + 84.0,
                y: ry,
                text: tx.description.clone(),
                font_size: 12.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some((w - 84.0 - 110.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            let (amount, color) = Self::format_currency_colored(tx.amount, &self.palette);
            Self::right_text(f, x + w, ry, amount, (12.0, FontWeightHint::Bold, color));
            f.hit(target, row);
            ry += 28.0;
        }
    }

    /// Where the transaction columns start in a list `w` wide: the date, the
    /// description, the category, and the amount's right edge.
    fn tx_columns(w: f32) -> (f32, f32, f32, f32) {
        let amount_right = (w - 32.0).max(0.0);
        let category = (amount_right - 120.0 - 150.0).max(98.0);
        (8.0, 98.0, category, amount_right)
    }

    /// What the transaction list says when it has no rows, which is never
    /// the same thing twice: why it is empty, and what would fill it.
    fn empty_list_text(&self) -> String {
        let month = format!("{} {}", self.view_month.month_label(), self.view_month.year);
        if self.accounts.is_empty() {
            String::from(
                "No accounts yet: a transaction goes into one. The accounts screen (4) adds one.",
            )
        } else if !self.search_query.is_empty() {
            format!(
                "Nothing in any month matches \u{201C}{}\u{201D}.",
                self.search_query
            )
        } else if let Some(cat) = self.category_filter {
            format!("No {} in {month}. C shows the next category.", cat.label())
        } else {
            format!("No transactions in {month}. N adds one.")
        }
    }

    fn render_transactions(&self, f: &mut Frame<Target>) {
        let cx = self.content_x() + 8.0;
        let cy = self.content_y() + 8.0;
        let cw = (self.content_w() - 16.0).max(0.0);
        let chosen = self.chosen_transaction().is_some();

        // The toolbar: search, the category shown, and what can be done.
        let buttons_w = 3.0 * 80.0 + 2.0 * 6.0;
        let chip_w = 160.0;
        let search = Rect::new(cx, cy, (cw - buttons_w - chip_w - 12.0).max(60.0), 32.0);
        self.render_search(f, search);
        self.render_filter_chip(f, Rect::new(search.right() + 6.0, cy, chip_w, 32.0));
        self.buttons_to(
            f,
            cx + cw,
            cy + 2.0,
            &[
                ("+ New", 80.0, Target::NewTransaction, true),
                ("Edit", 80.0, Target::Edit, chosen),
                ("Delete", 80.0, Target::Delete, chosen),
            ],
        );

        // Column heads.
        let head_y = cy + 40.0;
        self.palette
            .push_surface(f, cx, head_y, cw, 28.0, 0.0, Surface::Card);
        let (date_x, desc_x, cat_x, amount_right) = Self::tx_columns(cw);
        for (hx, label) in [
            (date_x, "Date"),
            (desc_x, "Description"),
            (cat_x, "Category"),
        ] {
            f.push(RenderCommand::Text {
                x: cx + hx,
                y: head_y + 7.0,
                text: label.to_string(),
                font_size: 11.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(120.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
        Self::right_text(
            f,
            cx + amount_right,
            head_y + 7.0,
            String::from("Amount"),
            (11.0, FontWeightHint::Bold, self.palette.subtext0),
        );

        // The rows.
        let (pane, rows) = self.tx_pane();
        f.hit(Target::TxList, pane);
        let filtered = self.filtered_transactions();
        if filtered.is_empty() {
            self.note(
                f,
                pane.x + 8.0,
                pane.y + 12.0,
                self.empty_list_text(),
                pane.w - 16.0,
            );
        }
        for (vi, (_, tx)) in filtered
            .iter()
            .enumerate()
            .skip(self.tx_scroll)
            .take(rows.saturating_add(1))
        {
            let ry = pane.y + vi.saturating_sub(self.tx_scroll) as f32 * TX_ROW_H;
            let row = Rect::new(pane.x, ry, pane.w, TX_ROW_H);
            f.push(RenderCommand::FillRect {
                x: row.x,
                y: row.y,
                width: row.w,
                height: row.h,
                color: if Some(tx.id) == self.selected_id {
                    self.palette.surface1
                } else if vi % 2 == 0 {
                    self.palette.surface0
                } else {
                    self.palette.base
                },
                corner_radii: CornerRadii::ZERO,
            });
            f.push(RenderCommand::Text {
                x: row.x + date_x,
                y: ry + 10.0,
                text: tx.date.format(),
                font_size: 12.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(84.0),
                overflow: TextOverflow::Ellipsis,
            });
            f.push(RenderCommand::Text {
                x: row.x + desc_x,
                y: ry + 10.0,
                text: tx.description.clone(),
                font_size: 13.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some((cat_x - desc_x - 8.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            f.push(RenderCommand::Text {
                x: row.x + cat_x,
                y: ry + 10.0,
                text: format!("{} {}", tx.category.icon(), tx.category.label()),
                font_size: 11.0,
                color: self.palette.ink(tx.category.color(&self.palette)),
                font_weight: FontWeightHint::Regular,
                max_width: Some(144.0),
                overflow: TextOverflow::Ellipsis,
            });
            let (amount, color) = Self::format_currency_colored(tx.amount, &self.palette);
            Self::right_text(
                f,
                row.x + amount_right,
                ry + 10.0,
                amount,
                (13.0, FontWeightHint::Bold, color),
            );
            if tx.recurring {
                f.push(RenderCommand::Text {
                    x: row.right() - 24.0,
                    y: ry + 10.0,
                    text: String::from("\u{1F501}"),
                    font_size: 11.0,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(20.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            f.hit(Target::TxRow(tx.id), row);
        }
        self.scroll_thumb(f, pane, filtered.len(), rows, self.tx_scroll);
    }

    /// The search box: what is typed, a caret while typing, and -- once there
    /// is a query -- that it reaches every month.
    fn render_search(&self, f: &mut Frame<Target>, rect: Rect) {
        self.palette
            .push_surface(f, rect.x, rect.y, rect.w, rect.h, 6.0, Surface::Card);
        if self.search_active {
            f.push(RenderCommand::StrokeRect {
                x: rect.x,
                y: rect.y,
                width: rect.w,
                height: rect.h,
                color: self.palette.blue,
                line_width: 2.0,
                corner_radii: CornerRadii::all(6.0),
            });
        }
        let room = (rect.w - 110.0).max(0.0);
        let placeholder = self.search_query.is_empty() && !self.search_active;
        f.push(RenderCommand::Text {
            x: rect.x + 12.0,
            y: rect.y + 8.0,
            text: if placeholder {
                String::from("Search every month  ( / )")
            } else {
                self.search_query.clone()
            },
            font_size: 13.0,
            color: if placeholder {
                self.palette.subtext0
            } else {
                self.palette.text
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(room),
            overflow: TextOverflow::Ellipsis,
        });
        if self.search_active {
            let typed = guitk::text::measure(&self.search_query, 13.0, FontWeightHint::Regular);
            f.push(RenderCommand::FillRect {
                x: rect.x + 12.0 + typed.min(room),
                y: rect.y + 7.0,
                width: textedit::CARET_WIDTH,
                height: 18.0,
                color: self.palette.text,
                corner_radii: CornerRadii::ZERO,
            });
        }
        if !self.search_query.is_empty() {
            Self::right_text(
                f,
                rect.right() - 10.0,
                rect.y + 10.0,
                String::from("every month"),
                (11.0, FontWeightHint::Regular, self.palette.subtext0),
            );
        }
        f.hit(Target::Search, rect);
    }

    /// The category the list is showing; a press shows the next.
    fn render_filter_chip(&self, f: &mut Frame<Target>, rect: Rect) {
        let (fill, label, ink) = match self.category_filter {
            Some(cat) => (
                cat.color(&self.palette),
                format!("{} {}", cat.icon(), cat.label()),
                self.palette.crust,
            ),
            None => (
                if self.hover == Some(Target::FilterChip) {
                    self.palette.surface2
                } else {
                    self.palette.surface1
                },
                String::from("All categories  (C)"),
                self.palette.text,
            ),
        };
        f.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: fill,
            corner_radii: CornerRadii::all(rect.h / 2.0),
        });
        f.push(RenderCommand::Text {
            x: rect.x + 12.0,
            y: rect.y + 9.0,
            text: label,
            font_size: 12.0,
            color: ink,
            font_weight: FontWeightHint::Bold,
            max_width: Some((rect.w - 24.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(Target::FilterChip, rect);
    }

    fn render_budgets(&self, f: &mut Frame<Target>) {
        let cx = self.content_x() + 16.0;
        let cy = self.content_y() + 16.0;
        let cw = (self.content_w() - 32.0).max(0.0);
        Self::screen_title(
            f,
            cx,
            cy,
            format!(
                "Budgets for {} {}",
                self.view_month.month_label(),
                self.view_month.year
            ),
            self.palette.text,
        );
        self.buttons_to(
            f,
            cx + cw,
            cy - 4.0,
            &[("Set budget", 110.0, Target::Edit, true)],
        );

        // Every category, with a budget or not: only the ones with a budget
        // were listed, so the first could never be set.
        let (pane, rows) = self.budget_pane();
        f.hit(Target::BudgetList, pane);
        for (i, cat) in Category::EXPENSE_CATS
            .iter()
            .enumerate()
            .skip(self.budget_scroll)
            .take(rows.saturating_add(1))
        {
            let ry = pane.y + i.saturating_sub(self.budget_scroll) as f32 * BUDGET_ROW_H;
            let row = Rect::new(pane.x, ry, pane.w, BUDGET_ROW_H);
            f.push(RenderCommand::FillRect {
                x: row.x,
                y: row.y,
                width: row.w,
                height: row.h,
                color: if i == self.selected_budget {
                    self.palette.surface1
                } else if i % 2 == 0 {
                    self.palette.surface0
                } else {
                    self.palette.base
                },
                corner_radii: CornerRadii::ZERO,
            });
            f.push(RenderCommand::Text {
                x: row.x + 12.0,
                y: ry + 8.0,
                text: format!("{} {}", cat.icon(), cat.label()),
                font_size: 14.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some((row.w - 260.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            let spent = self.category_spending(*cat);
            if let Some(budget) = self.budgets.iter().find(|b| b.category == *cat) {
                let usage = Self::usage_ratio(spent, budget.monthly_limit);
                let remaining = budget.monthly_limit.saturating_sub(spent);
                let within = remaining >= 0;
                Self::right_text(
                    f,
                    row.right() - 12.0,
                    ry + 8.0,
                    format!(
                        "{} / {}",
                        Self::format_currency(spent),
                        Self::format_currency(budget.monthly_limit)
                    ),
                    (
                        13.0,
                        FontWeightHint::Bold,
                        if within {
                            self.palette.ink(self.palette.green)
                        } else {
                            self.palette.ink(self.palette.red)
                        },
                    ),
                );
                self.usage_bar(
                    f,
                    Rect::new(row.x + 12.0, ry + 30.0, (row.w - 24.0).max(0.0), 10.0),
                    usage,
                );
                f.push(RenderCommand::Text {
                    x: row.x + 12.0,
                    y: ry + 45.0,
                    text: format!("{:.0}% used", usage * 100.0),
                    font_size: 11.0,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(90.0),
                    overflow: TextOverflow::Ellipsis,
                });
                f.push(RenderCommand::Text {
                    x: row.x + 104.0,
                    y: ry + 45.0,
                    text: if within {
                        format!("{} remaining", Self::format_currency(remaining))
                    } else {
                        format!(
                            "{} over budget",
                            Self::format_currency(remaining.saturating_neg())
                        )
                    },
                    font_size: 11.0,
                    color: if within {
                        self.palette.ink(self.palette.teal)
                    } else {
                        self.palette.ink(self.palette.red)
                    },
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(220.0),
                    overflow: TextOverflow::Ellipsis,
                });
            } else {
                Self::right_text(
                    f,
                    row.right() - 12.0,
                    ry + 9.0,
                    String::from("No budget"),
                    (12.0, FontWeightHint::Regular, self.palette.subtext0),
                );
                let hint = if spent > 0 {
                    format!(
                        "{} spent. Enter sets a budget.",
                        Self::format_currency(spent)
                    )
                } else {
                    String::from("Enter sets a budget.")
                };
                self.note(f, row.x + 12.0, ry + 36.0, hint, row.w - 24.0);
            }
            f.hit(Target::BudgetRow(i), row);
        }
        self.scroll_thumb(
            f,
            pane,
            Category::EXPENSE_CATS.len(),
            rows,
            self.budget_scroll,
        );
    }

    fn render_accounts(&self, f: &mut Frame<Target>) {
        let cx = self.content_x() + 16.0;
        let cy = self.content_y() + 16.0;
        let cw = (self.content_w() - 32.0).max(0.0);
        Self::screen_title(f, cx, cy, String::from("Accounts"), self.palette.text);
        let chosen = self
            .selected_account
            .is_some_and(|id| self.accounts.iter().any(|a| a.id == id));
        self.buttons_to(
            f,
            cx + cw,
            cy - 4.0,
            &[
                ("+ Account", 100.0, Target::NewAccount, true),
                ("Edit", 80.0, Target::Edit, chosen),
                ("Delete", 80.0, Target::Delete, chosen),
            ],
        );

        let (pane, rows) = self.account_pane();
        f.hit(Target::AccountList, pane);
        if self.accounts.is_empty() {
            self.note(
                f,
                pane.x + 8.0,
                pane.y + 12.0,
                String::from(
                    "No accounts yet. + Account adds one: its name, its kind, and what it held when you started.",
                ),
                pane.w - 16.0,
            );
        }
        for (i, account) in self
            .accounts
            .iter()
            .enumerate()
            .skip(self.account_scroll)
            .take(rows.saturating_add(1))
        {
            let ry = pane.y + i.saturating_sub(self.account_scroll) as f32 * ACCOUNT_ROW_H;
            let row = Rect::new(pane.x, ry, pane.w, ACCOUNT_ROW_H);
            f.push(RenderCommand::FillRect {
                x: row.x,
                y: row.y,
                width: row.w,
                height: row.h,
                color: if self.selected_account == Some(account.id) {
                    self.palette.surface1
                } else if i % 2 == 0 {
                    self.palette.surface0
                } else {
                    self.palette.base
                },
                corner_radii: CornerRadii::ZERO,
            });
            f.push(RenderCommand::Text {
                x: row.x + 12.0,
                y: ry + 9.0,
                text: account.name.clone(),
                font_size: 15.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some((row.w - 260.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            f.push(RenderCommand::Text {
                x: row.x + 12.0,
                y: ry + 32.0,
                text: account.account_type.label().to_string(),
                font_size: 11.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(160.0),
                overflow: TextOverflow::Ellipsis,
            });
            let (balance, color) =
                Self::format_currency_colored(self.account_balance(account.id), &self.palette);
            Self::right_text(
                f,
                row.right() - 12.0,
                ry + 8.0,
                balance,
                (18.0, FontWeightHint::Bold, color),
            );
            let count = self
                .transactions
                .iter()
                .filter(|t| t.account_id == account.id)
                .count();
            Self::right_text(
                f,
                row.right() - 12.0,
                ry + 34.0,
                format!("{count} transaction{}", if count == 1 { "" } else { "s" }),
                (11.0, FontWeightHint::Regular, self.palette.subtext0),
            );
            f.hit(Target::AccountRow(account.id), row);
        }
        self.scroll_thumb(f, pane, self.accounts.len(), rows, self.account_scroll);
    }

    fn render_reports(&self, f: &mut Frame<Target>) {
        let cx = self.content_x() + 16.0;
        let cy = self.content_y() + 16.0;
        let cw = (self.content_w() - 32.0).max(0.0);
        Self::screen_title(
            f,
            cx,
            cy,
            format!(
                "Financial Report \u{2014} {} {}",
                self.view_month.month_label(),
                self.view_month.year
            ),
            self.palette.text,
        );

        let income = self.month_income();
        let expenses = self.month_expenses();
        let net = income.saturating_sub(expenses);
        let tx_count = self.month_transactions().len();

        // Summary cards
        let summaries = [
            ("Total Income", income, self.palette.ink(self.palette.green)),
            (
                "Total Expenses",
                expenses,
                self.palette.ink(self.palette.red),
            ),
            (
                "Net Savings",
                net,
                if net >= 0 {
                    self.palette.ink(self.palette.teal)
                } else {
                    self.palette.ink(self.palette.red)
                },
            ),
        ];
        for (i, (label, amount, color)) in summaries.iter().enumerate() {
            let sx = cx + i as f32 * (cw / 3.0);
            let sw = (cw / 3.0 - 12.0).max(0.0);
            self.palette
                .push_surface(f, sx, cy + 36.0, sw, 70.0, 8.0, Surface::Card);
            f.push(RenderCommand::Text {
                x: sx + 12.0,
                y: cy + 46.0,
                text: (*label).to_string(),
                font_size: 12.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((sw - 24.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            f.push(RenderCommand::Text {
                x: sx + 12.0,
                y: cy + 66.0,
                text: Self::format_currency(*amount),
                font_size: 22.0,
                color: *color,
                font_weight: FontWeightHint::Bold,
                max_width: Some((sw - 24.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }

        self.note(
            f,
            cx,
            cy + 120.0,
            format!(
                "{tx_count} transaction{} this month",
                if tx_count == 1 { "" } else { "s" }
            ),
            300.0,
        );

        self.heading(f, cx, cy + 150.0, "Expense Breakdown by Category", 400.0);
        let top = self.top_expense_categories();
        let total_exp = expenses.max(1) as f32;
        for (i, (cat, amount)) in top.iter().enumerate() {
            let ry = cy + 178.0 + i as f32 * 32.0;
            let pct = *amount as f32 / total_exp * 100.0;
            let bar_w = (cw - 280.0).max(0.0) * (*amount as f32 / total_exp);
            f.push(RenderCommand::Text {
                x: cx,
                y: ry + 4.0,
                text: format!("{} {}", cat.icon(), cat.label()),
                font_size: 12.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some(140.0),
                overflow: TextOverflow::Ellipsis,
            });
            f.push(RenderCommand::FillRect {
                x: cx + 150.0,
                y: ry + 2.0,
                width: bar_w.max(4.0),
                height: 20.0,
                color: cat.color(&self.palette),
                corner_radii: CornerRadii::all(4.0),
            });
            f.push(RenderCommand::Text {
                x: cx + 160.0 + bar_w,
                y: ry + 4.0,
                text: format!("{} ({pct:.1}%)", Self::format_currency(*amount)),
                font_size: 11.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(150.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Savings rate
        if income > 0 {
            let savings_rate = net as f64 / income as f64 * 100.0;
            let sry = cy + 178.0 + top.len() as f32 * 32.0 + 24.0;
            f.push(RenderCommand::Text {
                x: cx,
                y: sry,
                text: format!("Savings Rate: {savings_rate:.1}%"),
                font_size: 16.0,
                color: if savings_rate >= 20.0 {
                    self.palette.ink(self.palette.green)
                } else if savings_rate >= 0.0 {
                    self.palette.ink(self.palette.yellow)
                } else {
                    self.palette.ink(self.palette.red)
                },
                font_weight: FontWeightHint::Bold,
                max_width: Some(300.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_status(&self, f: &mut Frame<Target>) {
        let sy = self.height - Self::STATUS_H;
        self.palette
            .push_surface(f, 0.0, sy, self.width, Self::STATUS_H, 0.0, Surface::Card);
        let room = (self.width - Self::SIDEBAR_W - 16.0).max(0.0);
        // Why nothing is being kept, beside whatever the last action said:
        // "Transaction added" is true and incomplete when the save failed.
        let error_room = if self.ledger_error.is_some() {
            room * 0.6
        } else {
            0.0
        };
        f.push(RenderCommand::Text {
            x: Self::SIDEBAR_W + 8.0,
            y: sy + 6.0,
            text: self.status_msg.clone(),
            font_size: 12.0,
            color: self.palette.subtext1,
            font_weight: FontWeightHint::Regular,
            max_width: Some((room - error_room - 12.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        if let Some(error) = &self.ledger_error {
            f.push(RenderCommand::Text {
                x: self.width - 8.0 - error_room,
                y: sy + 6.0,
                text: error.clone(),
                font_size: 12.0,
                color: self.palette.ink(self.palette.red),
                font_weight: FontWeightHint::Bold,
                max_width: Some(error_room),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    // ── The form ────────────────────────────────────────────────────

    /// A form row's height.
    const FORM_ROW_H: f32 = 40.0;

    /// Where the form's card is.
    fn form_card(&self, form: &Form) -> Rect {
        let (w, h) = (self.width, self.height);
        let rows = form.fields().len() as f32;
        let card_w = 540.0_f32.min(w - 24.0).max(0.0);
        let card_h = (60.0 + rows * Self::FORM_ROW_H + 92.0)
            .min(h - 24.0)
            .max(0.0);
        Rect::new((w - card_w) / 2.0, (h - card_h) / 2.0, card_w, card_h)
    }

    fn form_title(form: &Form) -> String {
        match form {
            Form::Transaction { id: None, .. } => String::from("New transaction"),
            Form::Transaction { id: Some(_), .. } => String::from("Change transaction"),
            Form::Account { id: None, .. } => String::from("New account"),
            Form::Account { id: Some(_), .. } => String::from("Change account"),
            Form::Budget { category, .. } => format!("Budget for {}", category.label()),
        }
    }

    fn field_label(field: FormField) -> &'static str {
        match field {
            FormField::Date => "Date",
            FormField::Description => "Description",
            FormField::Amount => "Amount",
            FormField::Kind => "Money",
            FormField::Category => "Category",
            FormField::Account => "Account",
            FormField::Notes => "Notes",
            FormField::Recurring => "Recurring",
            FormField::Name => "Name",
            FormField::AccountKind => "Kind",
            FormField::Opening => "Opening balance",
            FormField::Limit => "Monthly limit",
        }
    }

    /// What an empty text field says it wants.
    fn placeholder(field: FormField) -> &'static str {
        match field {
            FormField::Date => "YYYY-MM-DD",
            FormField::Description => "What it was for",
            FormField::Amount => "0.00",
            FormField::Notes => "Optional",
            FormField::Name => "What you call it",
            FormField::Opening => "0.00 when you started; minus if owed",
            FormField::Limit => "0.00 a month; empty takes it off",
            FormField::Kind
            | FormField::Category
            | FormField::Account
            | FormField::Recurring
            | FormField::AccountKind => "",
        }
    }

    /// What a chosen field shows.
    fn choice_label(&self, form: &Form, field: FormField) -> String {
        match (form, field) {
            (Form::Transaction { income, .. }, FormField::Kind) => String::from(if *income {
                "Income (money in)"
            } else {
                "Expense (money out)"
            }),
            (Form::Transaction { category, .. }, FormField::Category) => {
                format!("{} {}", category.icon(), category.label())
            }
            (Form::Transaction { account, .. }, FormField::Account) => account
                .and_then(|id| self.accounts.iter().find(|a| a.id == id))
                .map_or_else(|| String::from("(no account)"), |a| a.name.clone()),
            (Form::Transaction { recurring, .. }, FormField::Recurring) => {
                String::from(if *recurring {
                    "Yes \u{2014} marked \u{1F501} in the list"
                } else {
                    "No"
                })
            }
            (Form::Account { kind, .. }, FormField::AccountKind) => kind.label().to_owned(),
            _ => String::new(),
        }
    }

    /// The form over the window: a row per field, what the last save said
    /// was wrong, and Save and Cancel.
    fn render_form(&self, f: &mut Frame<Target>, form: &Form) {
        let (w, h) = (self.width, self.height);
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            color: Color::rgba(0, 0, 0, 150),
            corner_radii: CornerRadii::ZERO,
        });
        // Around and behind the card a press does nothing: the form is modal,
        // and a press reaching a row behind it would change what it is about.
        f.hit(Target::FormBackdrop, Rect::new(0.0, 0.0, w, h));
        let card = self.form_card(form);
        self.palette
            .push_surface(f, card.x, card.y, card.w, card.h, 12.0, Surface::Card);
        f.push(RenderCommand::Text {
            x: card.x + 20.0,
            y: card.y + 18.0,
            text: Self::form_title(form),
            font_size: 16.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some((card.w - 40.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        let label_w = 116.0;
        let control_w = (card.w - 40.0 - label_w).max(0.0);
        let mut y = card.y + 56.0;
        for &field in form.fields() {
            f.push(RenderCommand::Text {
                x: card.x + 20.0,
                y: y + 9.0,
                text: Self::field_label(field).to_owned(),
                font_size: 12.0,
                color: self.palette.subtext1,
                font_weight: FontWeightHint::Regular,
                max_width: Some(label_w - 8.0),
                overflow: TextOverflow::Ellipsis,
            });
            let rect = Rect::new(card.x + 20.0 + label_w, y, control_w, 32.0);
            if field.is_text() {
                self.render_text_field(f, form, field, rect);
            } else {
                self.render_choice(f, form, field, rect);
            }
            y += Self::FORM_ROW_H;
        }
        if let Some(error) = &self.form_error {
            f.push(RenderCommand::Text {
                x: card.x + 20.0,
                y: y + 4.0,
                text: error.clone(),
                font_size: 12.0,
                color: self.palette.ink(self.palette.red),
                font_weight: FontWeightHint::Bold,
                max_width: Some((card.w - 40.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
        let buttons_y = card.bottom() - 46.0;
        self.note(
            f,
            card.x + 20.0,
            buttons_y + 7.0,
            String::from("Tab: next field  \u{00B7}  Enter: save  \u{00B7}  Esc: cancel"),
            card.w - 40.0 - 180.0,
        );
        self.buttons_to(
            f,
            card.right() - 20.0,
            buttons_y,
            &[
                ("Cancel", 80.0, Target::Cancel, true),
                ("Save", 80.0, Target::Save, true),
            ],
        );
    }

    /// A text field: its box, and what is typed with the caret, or what it
    /// wants while it is empty and the keys are elsewhere.
    fn render_text_field(&self, f: &mut Frame<Target>, form: &Form, field: FormField, rect: Rect) {
        let focused = self.field == field;
        self.palette
            .push_surface(f, rect.x, rect.y, rect.w, rect.h, 4.0, Surface::Card);
        f.push(RenderCommand::StrokeRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: if focused {
                self.palette.blue
            } else {
                self.palette.surface1
            },
            line_width: if focused { 2.0 } else { 1.0 },
            corner_radii: CornerRadii::all(4.0),
        });
        if let Some(input) = form.input_ref(field) {
            if input.text().is_empty() && !focused {
                f.push(RenderCommand::Text {
                    x: rect.x + 8.0,
                    y: rect.y + 8.0,
                    text: Self::placeholder(field).to_owned(),
                    font_size: 13.0,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some((rect.w - 16.0).max(0.0)),
                    overflow: TextOverflow::Ellipsis,
                });
            } else {
                let mut tree = RenderTree::new();
                textedit::draw(
                    &mut tree,
                    &textedit::SingleLine {
                        text: input.text(),
                        cursor: if focused {
                            input.cursor()
                        } else {
                            TextCursor::default()
                        },
                        selection_anchor: if focused {
                            input.selection_anchor()
                        } else {
                            None
                        },
                        focused,
                        x: rect.x + 8.0,
                        y: rect.y + 7.0,
                        width: (rect.w - 16.0).max(0.0),
                        line_height: 18.0,
                        font_size: 13.0,
                        weight: FontWeightHint::Regular,
                        color: self.palette.text,
                        selection_bg: self.palette.blue,
                        selection_fg: self.palette.crust,
                        caret_width: textedit::CARET_WIDTH,
                    },
                );
                f.extend(tree.commands);
            }
        }
        // A press puts the keyboard in the field, and the caret under it.
        f.hit(Target::Field(field), rect);
    }

    /// A chosen field: its value between arrows. A press on the value steps
    /// it on, as the arrow after it does.
    fn render_choice(&self, f: &mut Frame<Target>, form: &Form, field: FormField, rect: Rect) {
        let focused = self.field == field;
        let value = Rect::new(rect.x + 36.0, rect.y, (rect.w - 72.0).max(0.0), rect.h);
        self.button(
            f,
            Rect::new(rect.x, rect.y, 32.0, rect.h),
            "\u{25C0}",
            Target::StepBack(field),
            true,
        );
        self.button(
            f,
            Rect::new(rect.right() - 32.0, rect.y, 32.0, rect.h),
            "\u{25B6}",
            Target::StepForward(field),
            true,
        );
        self.palette
            .push_surface(f, value.x, value.y, value.w, value.h, 4.0, Surface::Card);
        f.push(RenderCommand::StrokeRect {
            x: value.x,
            y: value.y,
            width: value.w,
            height: value.h,
            color: if focused {
                self.palette.blue
            } else {
                self.palette.surface1
            },
            line_width: if focused { 2.0 } else { 1.0 },
            corner_radii: CornerRadii::all(4.0),
        });
        let label = self.choice_label(form, field);
        f.push(RenderCommand::Text {
            x: guitk::text::center_x(
                &label,
                value.x + value.w / 2.0,
                13.0,
                FontWeightHint::Regular,
            )
            .max(value.x + 8.0),
            y: value.y + 8.0,
            text: label,
            font_size: 13.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some((value.w - 16.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(Target::Field(field), value);
    }

    /// The question before a delete, with a button for each answer.
    fn render_question(&self, f: &mut Frame<Target>, doomed: Doomed) {
        let (w, h) = (self.width, self.height);
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            color: Color::rgba(0, 0, 0, 160),
            corner_radii: CornerRadii::ZERO,
        });
        f.hit(Target::QuestionBackdrop, Rect::new(0.0, 0.0, w, h));
        let card = Rect::new((w - 480.0) / 2.0, (h - 150.0) / 2.0, 480.0, 150.0);
        self.palette
            .push_surface(f, card.x, card.y, card.w, card.h, 12.0, Surface::Card);
        f.hit(Target::QuestionCard, card);
        let (title, body) = match doomed {
            Doomed::Transaction(id) => {
                let tx = self.transactions.iter().find(|t| t.id == id);
                (
                    String::from("Delete this transaction?"),
                    tx.map_or_else(String::new, |t| {
                        format!(
                            "{}, {}, {}. This cannot be undone.",
                            t.date.format(),
                            t.description,
                            Self::format_currency(t.amount)
                        )
                    }),
                )
            }
            Doomed::Account(id) => {
                let name = self
                    .accounts
                    .iter()
                    .find(|a| a.id == id)
                    .map_or("", |a| a.name.as_str());
                let count = self
                    .transactions
                    .iter()
                    .filter(|t| t.account_id == id)
                    .count();
                (
                    format!("Delete the account {name}?"),
                    format!(
                        "Its {count} transaction{} go with it, and this cannot be undone.",
                        if count == 1 { "" } else { "s" }
                    ),
                )
            }
        };
        f.push(RenderCommand::Text {
            x: card.x + 20.0,
            y: card.y + 20.0,
            text: title,
            font_size: 16.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(card.w - 40.0),
            overflow: TextOverflow::Ellipsis,
        });
        f.push(RenderCommand::Text {
            x: card.x + 20.0,
            y: card.y + 50.0,
            text: body,
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(card.w - 40.0),
            overflow: TextOverflow::Ellipsis,
        });
        let delete = Rect::new(
            card.right() - 20.0 - 210.0,
            card.bottom() - 50.0,
            120.0,
            32.0,
        );
        f.push(RenderCommand::FillRect {
            x: delete.x,
            y: delete.y,
            width: delete.w,
            height: delete.h,
            color: self.palette.red,
            corner_radii: CornerRadii::all(6.0),
        });
        f.push(RenderCommand::Text {
            x: delete.x + 14.0,
            y: delete.y + 9.0,
            text: String::from("Delete (Y)"),
            font_size: 12.0,
            color: self.palette.crust,
            font_weight: FontWeightHint::Bold,
            max_width: Some(delete.w - 20.0),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(Target::ConfirmDelete, delete);
        self.button(
            f,
            Rect::new(card.right() - 20.0 - 80.0, card.bottom() - 50.0, 80.0, 32.0),
            "Keep",
            Target::KeepIt,
            true,
        );
    }

    // ── The pointer ─────────────────────────────────────────────────

    /// What is under `(x, y)` in the frame last shown.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        if self.last_hits.is_empty() {
            return self.frame().hit_test(x, y);
        }
        self.last_hits
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(target, _)| *target)
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                let frame = self.frame();
                let Some(target) = frame.hit_test(event.x, event.y) else {
                    return EventResult::Ignored;
                };
                if let Target::Field(field) = target
                    && field.is_text()
                    && let Some(rect) = frame.rect_of(|t| *t == target)
                {
                    self.place_caret(field, rect, event.x);
                }
                self.press(target)
            }
            MouseEventKind::Move => {
                let over = self.target_at(event.x, event.y);
                if over == self.hover {
                    return EventResult::Ignored;
                }
                self.hover = over;
                EventResult::Consumed
            }
            MouseEventKind::Leave => {
                if self.hover.take().is_some() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            MouseEventKind::Scroll { dy, .. } => self.wheel_at(event.x, event.y, dy),
            _ => EventResult::Ignored,
        }
    }

    /// Put a text field's caret under the pointer at `x`.
    fn place_caret(&mut self, field: FormField, rect: Rect, x: f32) {
        let was_focused = self.field == field;
        let Some(input) = self.form.as_mut().and_then(|form| form.input(field)) else {
            return;
        };
        // Measured against the field as it was drawn: an unfocused field is
        // drawn from its start, a focused one scrolled to its caret.
        let drawn = if was_focused {
            input.cursor()
        } else {
            TextCursor::default()
        };
        let cursor = textedit::cursor_at_click(
            input.text(),
            drawn,
            (rect.w - 16.0).max(0.0),
            13.0,
            FontWeightHint::Regular,
            x - rect.x - 8.0,
        );
        input.set_selection_anchor(None);
        input.set_cursor(cursor);
    }

    /// A left press on `target`.
    fn press(&mut self, target: Target) -> EventResult {
        // A press anywhere but the search box takes the keys out of it; the
        // search itself stays.
        if target != Target::Search {
            self.search_active = false;
        }
        match target {
            Target::HelpCard => self.show_help = false,
            Target::Help => self.show_help = true,
            Target::Screen(screen) => self.screen = screen,
            Target::MonthPrev => self.handle_key("Left", false, false),
            Target::MonthNext => self.handle_key("Right", false, false),
            Target::ThisMonth => self.handle_key("Home", false, false),
            Target::NewTransaction => self.open_new_transaction(),
            Target::NewAccount => self.open_new_account(),
            Target::Edit => self.edit_chosen(),
            Target::Delete => self.ask_to_delete(),
            Target::Search => {
                self.screen = Screen::Transactions;
                self.search_active = true;
            }
            Target::FilterChip => self.cycle_category_filter(),
            // A press chooses a row; a second press on it changes it.
            Target::TxRow(id) => {
                if self.selected_id == Some(id) {
                    self.open_edit_transaction(id);
                } else {
                    self.selected_id = Some(id);
                }
            }
            Target::AccountRow(id) => {
                if self.selected_account == Some(id) {
                    self.open_edit_account(id);
                } else {
                    self.selected_account = Some(id);
                }
            }
            Target::BudgetRow(i) => {
                if self.selected_budget == i {
                    self.open_budget(i);
                } else {
                    self.selected_budget = i;
                }
            }
            Target::RecentRow(id) => self.show_transaction(id),
            Target::Field(field) => {
                self.field = field;
                if !field.is_text() {
                    self.step_choice(field, true);
                }
            }
            Target::StepBack(field) => {
                self.field = field;
                self.step_choice(field, false);
            }
            Target::StepForward(field) => {
                self.field = field;
                self.step_choice(field, true);
            }
            Target::Save => self.save_form(),
            Target::Cancel => {
                self.form = None;
                self.form_error = None;
                self.status_msg = String::from("Cancelled");
            }
            Target::ConfirmDelete => {
                let Some(doomed) = self.pending_delete.take() else {
                    return EventResult::Ignored;
                };
                self.delete_doomed(doomed);
            }
            Target::KeepIt | Target::QuestionBackdrop => {
                if self.pending_delete.take().is_none() {
                    return EventResult::Ignored;
                }
                self.status_msg = String::from("Kept");
            }
            Target::QuestionCard
            | Target::FormBackdrop
            | Target::TxList
            | Target::AccountList
            | Target::BudgetList => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    /// The wheel over one of the three lists.
    fn wheel_at(&mut self, x: f32, y: f32, dy: f32) -> EventResult {
        let (now, count, visible) = match self.target_at(x, y) {
            Some(Target::TxList | Target::TxRow(_)) => {
                (self.tx_scroll, self.visible_ids().len(), self.tx_pane().1)
            }
            Some(Target::AccountList | Target::AccountRow(_)) => (
                self.account_scroll,
                self.accounts.len(),
                self.account_pane().1,
            ),
            Some(Target::BudgetList | Target::BudgetRow(_)) => (
                self.budget_scroll,
                Category::EXPENSE_CATS.len(),
                self.budget_pane().1,
            ),
            _ => return EventResult::Ignored,
        };
        let rows = self.wheel.rows(dy);
        let last = count.saturating_sub(visible);
        let next = if rows < 0 {
            now.saturating_sub(rows.unsigned_abs())
        } else {
            now.saturating_add(rows.unsigned_abs())
        }
        .min(last);
        if next == now {
            return EventResult::Ignored;
        }
        match self.screen {
            Screen::Accounts => self.account_scroll = next,
            Screen::Budgets => self.budget_scroll = next,
            _ => self.tx_scroll = next,
        }
        EventResult::Consumed
    }
}

impl App for FinanceApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

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

    /// Until the next midnight, and at least hourly.
    ///
    /// Today is a new transaction's date and the month "This month" returns
    /// to, and it was a constant: 18 May 2026 in every run. A tick that finds
    /// the same day answers `Ignored` and costs a clock read, not a frame.
    fn tick_interval(&self) -> Option<Duration> {
        let left = clock_now().map_or(3600, |(_, left)| left.saturating_add(1));
        Some(Duration::from_secs(left.min(3600)))
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
        self.clamp_scrolls();
        let frame = self.frame();
        self.last_hits = frame.hits().to_vec();
        frame.into_tree()
    }
}

/// What the dashboard says with no accounts: that the emptiness is correct,
/// and how to begin. It was drawn at the top of the window before the sidebar
/// and header, which painted over all of it.
///
/// A third line, drawn after these, says where what is entered is kept
/// (`FinanceApp::keeping_line`).
const NO_DATA_LINES: [&str; 2] = [
    "No accounts yet.",
    "Start with + Account (or 4, then N); then N adds a transaction.",
];

/// What the window says when there is nowhere to keep anything.
const NO_HOME: &str = "Nothing is kept: no home directory is set";

// ── Keeping the ledger ──────────────────────────────────────────────

/// The first line of a ledger this version writes, and the only one it reads.
const LEDGER_HEADER: &str = "# SlateOS finance ledger, format 1";

/// The largest ledger read. Past it nothing is read or written, rather than a
/// part being taken for the whole; a transaction is a line of about a hundred
/// bytes, so this is several hundred thousand of them.
const MAX_LEDGER_BYTES: usize = 64 * 1024 * 1024;

/// Where the ledger is kept: a text file in the user's settings directory,
/// beside `apps/flashcards`' decks and `apps/habits`' record.
fn ledger_path() -> Option<std::path::PathBuf> {
    settingsfile::config_dir().map(|dir| dir.join("finance").join("ledger.txt"))
}

/// Everything a ledger holds.
struct Ledger {
    accounts: Vec<Account>,
    budgets: Vec<Budget>,
    transactions: Vec<Transaction>,
}

/// The ledger as text: the header, then a line per account, budget and
/// transaction, its fields separated by tabs. Money is in whole cents and a
/// date is `YYYY-MM-DD`, so nothing is rounded on the way through.
fn ledger_text(accounts: &[Account], budgets: &[Budget], transactions: &[Transaction]) -> String {
    let mut out = String::from(LEDGER_HEADER);
    out.push('\n');
    for a in accounts {
        out.push_str(&format!(
            "account\t{}\t{}\t{}\t{}\n",
            a.id,
            a.account_type.key(),
            a.initial_balance,
            tsv::escape(&a.name)
        ));
    }
    for b in budgets {
        out.push_str(&format!(
            "budget\t{}\t{}\n",
            b.category.key(),
            b.monthly_limit
        ));
    }
    for t in transactions {
        out.push_str(&format!(
            "tx\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            t.id,
            t.date.format(),
            t.amount,
            t.category.key(),
            t.account_id,
            if t.recurring { "y" } else { "n" },
            tsv::escape(&t.description),
            tsv::escape(&t.notes)
        ));
    }
    out
}

/// Read a ledger `ledger_text` wrote, or say which line is wrong and how.
///
/// All or nothing: a ledger with one line not understood is refused whole,
/// because the next save would write back only what was read, and the line
/// not understood would be gone.
fn parse_ledger(text: &str) -> Result<Ledger, String> {
    let mut lines = text.lines().enumerate();
    match lines.next() {
        Some((_, first)) if first == LEDGER_HEADER => {}
        Some((_, first)) if first.starts_with("# SlateOS finance ledger") => {
            return Err(String::from(
                "it was written by a newer version of this program",
            ));
        }
        _ => {
            return Err(String::from(
                "it does not begin with the ledger's first line",
            ));
        }
    }
    let mut ledger = Ledger {
        accounts: Vec::new(),
        budgets: Vec::new(),
        transactions: Vec::new(),
    };
    // Each transaction's line, to name it if its account is missing.
    let mut tx_lines = Vec::new();
    for (i, line) in lines {
        let n = i.saturating_add(1);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let bad = |why: &str| format!("line {n}: {why}");
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.as_slice() {
            ["account", id, kind, opening, name] => {
                let id: u32 = id
                    .parse()
                    .map_err(|_| bad("an account's number is not one"))?;
                if ledger.accounts.iter().any(|a| a.id == id) {
                    return Err(bad("two accounts have one number"));
                }
                ledger.accounts.push(Account {
                    id,
                    account_type: AccountType::from_key(kind)
                        .ok_or_else(|| bad("an account is of a kind this program does not know"))?,
                    initial_balance: opening
                        .parse()
                        .map_err(|_| bad("an opening balance is not a whole number of cents"))?,
                    name: tsv::unescape(name)
                        .ok_or_else(|| bad("a name holds an unknown escape"))?,
                });
            }
            ["budget", category, limit] => {
                let category = Category::from_key(category)
                    .ok_or_else(|| bad("a budget is for a category this program does not know"))?;
                if ledger.budgets.iter().any(|b| b.category == category) {
                    return Err(bad("a category has two budgets"));
                }
                let monthly_limit: i64 = limit
                    .parse()
                    .map_err(|_| bad("a budget is not a whole number of cents"))?;
                if monthly_limit <= 0 {
                    return Err(bad("a budget is not more than nothing"));
                }
                ledger.budgets.push(Budget {
                    category,
                    monthly_limit,
                });
            }
            [
                "tx",
                id,
                date,
                amount,
                category,
                account,
                recurring,
                description,
                notes,
            ] => {
                let id: u32 = id
                    .parse()
                    .map_err(|_| bad("a transaction's number is not one"))?;
                if ledger.transactions.iter().any(|t| t.id == id) {
                    return Err(bad("two transactions have one number"));
                }
                ledger.transactions.push(Transaction {
                    id,
                    date: SimpleDate::parse(date).ok_or_else(|| bad("a date is not one"))?,
                    amount: amount
                        .parse()
                        .map_err(|_| bad("an amount is not a whole number of cents"))?,
                    category: Category::from_key(category)
                        .ok_or_else(|| bad("a category this program does not know"))?,
                    account_id: account
                        .parse()
                        .map_err(|_| bad("an account's number is not one"))?,
                    recurring: match *recurring {
                        "y" => true,
                        "n" => false,
                        _ => return Err(bad("recurring is neither y nor n")),
                    },
                    description: tsv::unescape(description)
                        .ok_or_else(|| bad("a description holds an unknown escape"))?,
                    notes: tsv::unescape(notes)
                        .ok_or_else(|| bad("a note holds an unknown escape"))?,
                });
                tx_lines.push(n);
            }
            _ => return Err(bad("not a line this program writes")),
        }
    }
    for (tx, n) in ledger.transactions.iter().zip(&tx_lines) {
        if !ledger.accounts.iter().any(|a| a.id == tx.account_id) {
            return Err(format!(
                "line {n}: a transaction is in account {}, which the ledger does not have",
                tx.account_id
            ));
        }
    }
    Ok(ledger)
}

/// Row heights of the three lists.
const TX_ROW_H: f32 = 36.0;
const ACCOUNT_ROW_H: f32 = 56.0;
const BUDGET_ROW_H: f32 = 64.0;

/// Today's date from the system clock, or `None` if it cannot be read.
fn today_from_clock() -> Option<SimpleDate> {
    clock_now().map(|(date, _)| date)
}

/// Today's date, and how many seconds are left of it.
///
/// The zone comes from `tzrules` as in `apps/habits`, so a real local zone is
/// used on the day `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ` is fixed.
fn clock_now() -> Option<(SimpleDate, u64)> {
    let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    let utc = i64::try_from(since_epoch.as_secs()).ok()?;
    let zone = tzrules::Tz::utc();
    let local = utc.saturating_add(i64::from(zone.lookup(utc).gmtoff));
    let into_day = u64::try_from(local.rem_euclid(86_400)).ok()?;
    let (year, month, day) = guitk::date::Date::from_unix_utc(local).ymd();
    let date = SimpleDate::new(
        u16::try_from(year).ok()?,
        u8::try_from(month).ok()?,
        u8::try_from(day).ok()?,
    );
    Some((date, 86_400_u64.saturating_sub(into_day)))
}

/// An amount of money typed as `12`, `12.5`, `1,234.56` or `$12.34`, in
/// cents. A leading `-` is taken only where `negative` allows it (an
/// opening balance may be owed); elsewhere the form's Income/Expense says
/// which way the money went.
fn parse_cents(text: &str, negative: bool) -> Result<i64, String> {
    let mut t = text.trim();
    let minus = t.starts_with('-');
    if minus {
        if !negative {
            return Err(String::from(
                "Enter the amount without a sign; choose Income or Expense",
            ));
        }
        t = t.get(1..).unwrap_or("").trim_start();
    }
    let t = t.strip_prefix('$').unwrap_or(t);
    let t: String = t.chars().filter(|c| *c != ',').collect();
    if t.is_empty() {
        // Nothing is nothing; a sign on its own is not an amount.
        return if minus {
            Err(String::from("That is not an amount"))
        } else {
            Ok(0)
        };
    }
    let (whole, frac) = t.split_once('.').unwrap_or((&t, ""));
    if whole.is_empty() && frac.is_empty() {
        return Err(String::from("That is not an amount"));
    }
    if !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) {
        return Err(String::from("That is not an amount"));
    }
    if frac.len() > 2 {
        return Err(String::from("An amount has at most two decimals"));
    }
    let whole: i64 = if whole.is_empty() {
        0
    } else {
        whole
            .parse()
            .map_err(|_| String::from("That amount is too large"))?
    };
    let cents: i64 = match frac.len() {
        0 => 0,
        1 => frac.parse::<i64>().unwrap_or(0).saturating_mul(10),
        _ => frac.parse().unwrap_or(0),
    };
    let total = whole
        .checked_mul(100)
        .and_then(|w| w.checked_add(cents))
        .ok_or_else(|| String::from("That amount is too large"))?;
    Ok(if minus { total.saturating_neg() } else { total })
}

/// `at` moved by `delta` in a list of `len`, stopping at the ends; the first
/// row when nothing was chosen.
fn step_index(at: Option<usize>, delta: isize, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let Some(at) = at else {
        return Some(0);
    };
    let moved = if delta < 0 {
        at.saturating_sub(delta.unsigned_abs())
    } else {
        at.saturating_add(delta.unsigned_abs())
    };
    Some(moved.min(len.saturating_sub(1)))
}

/// The character a letter or digit key types, shifted or not; `/`.
fn key_char(key: Key, shift: bool) -> Option<char> {
    const KEYS: [(Key, char); 37] = [
        (Key::A, 'a'),
        (Key::B, 'b'),
        (Key::C, 'c'),
        (Key::D, 'd'),
        (Key::E, 'e'),
        (Key::F, 'f'),
        (Key::G, 'g'),
        (Key::H, 'h'),
        (Key::I, 'i'),
        (Key::J, 'j'),
        (Key::K, 'k'),
        (Key::L, 'l'),
        (Key::M, 'm'),
        (Key::N, 'n'),
        (Key::O, 'o'),
        (Key::P, 'p'),
        (Key::Q, 'q'),
        (Key::R, 'r'),
        (Key::S, 's'),
        (Key::T, 't'),
        (Key::U, 'u'),
        (Key::V, 'v'),
        (Key::W, 'w'),
        (Key::X, 'x'),
        (Key::Y, 'y'),
        (Key::Z, 'z'),
        (Key::Num0, '0'),
        (Key::Num1, '1'),
        (Key::Num2, '2'),
        (Key::Num3, '3'),
        (Key::Num4, '4'),
        (Key::Num5, '5'),
        (Key::Num6, '6'),
        (Key::Num7, '7'),
        (Key::Num8, '8'),
        (Key::Num9, '9'),
        (Key::Slash, '/'),
    ];
    let &(_, c) = KEYS.iter().find(|(k, _)| *k == key)?;
    Some(if shift { c.to_ascii_uppercase() } else { c })
}

/// What one keystroke did to a one-line field.
struct LineEdit {
    handled: bool,
    copied: Option<String>,
}

/// Apply a keystroke to a one-line field, as `apps/flashcards` does (see
/// `requests/e-c-a-text-field-that-takes-its-own-keys.md`).
fn edit_line(input: &mut TextInput, key: &KeyEvent, capacity: usize, clipboard: &str) -> LineEdit {
    let shift = key.modifiers.shift;
    let ctrl = key.modifiers.ctrl;
    let mut copied = None;
    match key.key {
        Key::Left => input.move_cursor_left(shift, 13.0, FontWeightHint::Regular),
        Key::Right => input.move_cursor_right(shift, 13.0, FontWeightHint::Regular),
        Key::Home => input.move_home(shift),
        Key::End => input.move_end(shift),
        Key::Backspace => input.backspace(),
        Key::Delete => input.delete(),
        Key::A if ctrl => input.select_all(),
        Key::C if ctrl => {
            if input.has_selection() {
                copied = Some(input.selected_text().to_string());
            }
        }
        Key::X if ctrl => {
            if input.has_selection() {
                copied = Some(input.selected_text().to_string());
                input.delete_selection();
            }
        }
        Key::V if ctrl => insert_limited(input, clipboard, capacity),
        _ => {
            if key.text.is_empty() || ctrl {
                return LineEdit {
                    handled: false,
                    copied: None,
                };
            }
            insert_limited(input, &key.text, capacity);
        }
    }
    LineEdit {
        handled: true,
        copied,
    }
}

/// Type `typed` into `input` over its selection, up to `capacity`
/// characters, leaving control characters out.
fn insert_limited(input: &mut TextInput, typed: &str, capacity: usize) {
    if input.has_selection() {
        input.delete_selection();
    }
    for ch in typed.chars() {
        if ch.is_control() {
            continue;
        }
        if input.text().chars().count() >= capacity {
            break;
        }
        input.insert_char(ch);
    }
}

fn main() -> ExitCode {
    app::launch("finance", &mut FinanceApp::from_settings())
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

    /// A fresh app holds no accounts and no money.
    ///
    /// `new` called `create_sample_data`, so the app opened on a "Main
    /// Checking" holding 3,500, a "Savings" holding 12,000, a credit card, a
    /// cash account, seven budget lines and a month of transactions including
    /// a 5,000 salary.
    ///
    /// Nobody would mistake these for their own accounts on sight, which is
    /// why it is worth being precise about where the harm is. It is
    /// downstream: the moment the user adds one real transaction, every total
    /// this app computes -- net worth, budget remaining, category spend -- is
    /// summed over their figure *and* the invented ones, and the result looks
    /// like arithmetic rather than like a mistake.
    #[test]
    fn a_fresh_app_holds_no_accounts_and_no_money() {
        let app = FinanceApp::new();
        assert!(app.accounts.is_empty(), "accounts appeared from nowhere");
        assert!(
            app.transactions.is_empty(),
            "transactions appeared from nowhere"
        );
        assert_eq!(
            app.total_balance(),
            0,
            "a balance was computed over invented money"
        );
    }

    /// And the window says why it is empty, and how to begin.
    ///
    /// The first version of this message said "add an account and a
    /// transaction, and every figure below will be yours" while nothing in
    /// the program could add either: **a fix that promises a capability the
    /// program does not have is the same defect it was fixing, pointed one
    /// step further into the future.** The second version said so, from under
    /// the sidebar and the header, which painted over it. The capability
    /// exists now, and the message names the control that begins -- which
    /// this test presses, so the promise is checked rather than trusted.
    #[test]
    fn the_window_says_the_emptiness_is_correct() {
        let mut app = FinanceApp::new();
        let drawn = texts(&app);
        for line in NO_DATA_LINES {
            assert!(
                drawn.iter().any(|t| t == line),
                "the window never said {line:?}"
            );
        }
        assert!(
            drawn.iter().any(|t| t == "Nothing you enter here is kept."),
            "a window that keeps nothing did not say so"
        );
        assert!(
            NO_DATA_LINES.iter().any(|l| l.contains("+ Account")),
            "the message does not name the control that begins"
        );
        probe::click(&mut app, Target::NewAccount);
        assert!(
            matches!(app.form, Some(Form::Account { id: None, .. })),
            "the control the message names does not begin anything"
        );
    }

    #[test]
    fn test_new_app() {
        let app = FinanceApp::with_sample_data();
        assert!(!app.transactions.is_empty());
        assert!(!app.accounts.is_empty());
        assert!(!app.budgets.is_empty());
        assert_eq!(app.screen, Screen::Dashboard);
    }

    #[test]
    fn test_sample_data_accounts() {
        let app = FinanceApp::with_sample_data();
        assert_eq!(app.accounts.len(), 4);
    }

    #[test]
    fn test_sample_data_budgets() {
        let app = FinanceApp::with_sample_data();
        assert_eq!(app.budgets.len(), 7);
    }

    #[test]
    fn test_add_account() {
        let mut app = FinanceApp::with_sample_data();
        let n = app.accounts.len();
        app.add_account("Test", AccountType::Cash, 1000);
        assert_eq!(app.accounts.len(), n + 1);
    }

    #[test]
    fn test_add_transaction() {
        let mut app = FinanceApp::with_sample_data();
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
        let mut app = FinanceApp::with_sample_data();
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
        let mut app = FinanceApp::with_sample_data();
        app.screen = Screen::Transactions;
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
        let mut app = FinanceApp::with_sample_data();
        app.screen = Screen::Transactions;
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
        let mut app = FinanceApp::with_sample_data();
        let n = app.transactions.len();
        assert!(app.selected_id.is_none(), "a fresh window selects nothing");
        app.handle_key("d", true, false);
        assert_eq!(app.transactions.len(), n);
    }

    #[test]
    fn test_delete_out_of_bounds() {
        let mut app = FinanceApp::with_sample_data();
        let n = app.transactions.len();
        app.delete_transaction(999);
        assert_eq!(app.transactions.len(), n);
    }

    #[test]
    fn test_set_budget_new() {
        let mut app = FinanceApp::with_sample_data();
        let n = app.budgets.len();
        app.set_budget(Category::Savings, 100_000);
        assert_eq!(app.budgets.len(), n + 1);
    }

    #[test]
    fn test_month_income() {
        let app = FinanceApp::with_sample_data();
        let income = app.month_income();
        assert!(income > 0);
    }

    #[test]
    fn test_month_expenses() {
        let app = FinanceApp::with_sample_data();
        let expenses = app.month_expenses();
        assert!(expenses > 0);
    }

    #[test]
    fn test_month_savings() {
        let app = FinanceApp::with_sample_data();
        let savings = app.month_savings();
        let income = app.month_income();
        let expenses = app.month_expenses();
        assert_eq!(savings, income - expenses);
    }

    #[test]
    fn test_category_spending() {
        let app = FinanceApp::with_sample_data();
        let food = app.category_spending(Category::Food);
        assert!(food > 0);
    }

    #[test]
    fn test_account_balance() {
        let app = FinanceApp::with_sample_data();
        let bal = app.account_balance(1);
        assert!(bal != 0);
    }

    #[test]
    fn test_total_balance() {
        let app = FinanceApp::with_sample_data();
        let total = app.total_balance();
        assert!(total > 0);
    }

    #[test]
    fn test_filtered_all() {
        let app = FinanceApp::with_sample_data();
        let f = app.filtered_transactions();
        assert_eq!(f.len(), app.transactions.len());
    }

    #[test]
    fn test_filtered_by_category() {
        let mut app = FinanceApp::with_sample_data();
        app.category_filter = Some(Category::Food);
        let f = app.filtered_transactions();
        assert!(f.len() < app.transactions.len());
        for (_, tx) in &f {
            assert_eq!(tx.category, Category::Food);
        }
    }

    /// A key press with the text a real keyboard would send with it.
    fn keyed(k: Key, text: &str) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: text.to_owned(),
        })
    }

    /// **Backspacing the search box has to read as a redraw.**
    ///
    /// `handle_key` reports nothing, so `state_fingerprint` decides whether a
    /// frame is drawn, and `search_query` is not in it. Typing is safe --
    /// that path answers `Consumed` outright -- but `Backspace` goes through
    /// the fingerprint, changes only the query, and so answered `Ignored`:
    /// the search bar draws the query with a caret after it, and the deleted
    /// character stayed on screen.
    ///
    /// `apps/jsonviewer` had three of these and `apps/flashcards` one; this is
    /// the third app in the tree that decides redraws by comparing a snapshot.
    #[test]
    fn deleting_a_character_from_the_search_is_a_redraw() {
        let mut app = FinanceApp::with_sample_data();
        app.handle_event(&keyed(Key::Slash, "/"));
        assert!(app.search_active, "`/` did not open the search box");

        app.handle_event(&keyed(Key::Unknown(0), "g"));
        assert_eq!(app.search_query, "g", "the box did not take the character");

        assert_eq!(
            app.handle_event(&keyed(Key::Backspace, "")),
            EventResult::Consumed,
            "backspacing did not read as a redraw, so the deleted character stays on screen"
        );
        assert!(app.search_query.is_empty(), "the character was not deleted");
    }

    #[test]
    fn test_filtered_by_search() {
        let mut app = FinanceApp::with_sample_data();
        app.search_query = String::from("grocery");
        let f = app.filtered_transactions();
        assert!(!f.is_empty());
        for (_, tx) in &f {
            assert!(tx.description.to_ascii_lowercase().contains("grocery"));
        }
    }

    #[test]
    fn test_top_expense_categories() {
        let app = FinanceApp::with_sample_data();
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let (_, c1) = FinanceApp::format_currency_colored(100, &pal);
        let (_, c2) = FinanceApp::format_currency_colored(-100, &pal);
        let (_, c3) = FinanceApp::format_currency_colored(0, &pal);
        assert_eq!(c1.r, pal.green.r);
        assert_eq!(c2.r, pal.red.r);
        assert_eq!(c3.r, pal.text.r);
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
        let mut app = FinanceApp::with_sample_data();
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
        let mut app = FinanceApp::with_sample_data();
        let month = app.view_month.month;
        app.handle_key("Left", false, false);
        assert_eq!(app.view_month.month, month - 1);
        app.handle_key("Right", false, false);
        assert_eq!(app.view_month.month, month);
    }

    #[test]
    fn test_handle_key_search() {
        let mut app = FinanceApp::with_sample_data();
        app.handle_key("/", false, false);
        assert!(app.search_active);
        app.handle_key("Escape", false, false);
        assert!(!app.search_active);
    }

    #[test]
    fn test_handle_key_category_filter() {
        let mut app = FinanceApp::with_sample_data();
        assert!(app.category_filter.is_none());
        app.handle_key("c", false, false);
        assert!(app.category_filter.is_some());
    }

    #[test]
    fn test_set_budget() {
        let mut app = FinanceApp::with_sample_data();
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
        let mut app = FinanceApp::with_sample_data();
        app.screen = Screen::Transactions;
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
        let mut app = FinanceApp::with_sample_data();
        app.search_active = true;
        app.handle_search_text("test");
        assert_eq!(app.search_query, "test");
    }

    #[test]
    fn test_render_dashboard() {
        let app = FinanceApp::with_sample_data();
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_transactions() {
        let mut app = FinanceApp::with_sample_data();
        app.screen = Screen::Transactions;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_budgets() {
        let mut app = FinanceApp::with_sample_data();
        app.screen = Screen::Budgets;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_accounts() {
        let mut app = FinanceApp::with_sample_data();
        app.screen = Screen::Accounts;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_reports() {
        let mut app = FinanceApp::with_sample_data();
        app.screen = Screen::Reports;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_search() {
        let mut app = FinanceApp::with_sample_data();
        app.screen = Screen::Transactions;
        app.search_active = true;
        app.search_query = String::from("grocery");
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_filter() {
        let mut app = FinanceApp::with_sample_data();
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
        let app = FinanceApp::with_sample_data();
        let txs = app.month_transactions();
        for tx in &txs {
            assert!(tx.date.same_month(&app.view_month));
        }
    }

    #[test]
    fn test_different_month_no_transactions() {
        let mut app = FinanceApp::with_sample_data();
        app.view_month = SimpleDate::new(2025, 1, 1);
        let txs = app.month_transactions();
        assert!(txs.is_empty());
    }

    /// Ctrl+D deleted the chosen transaction outright, from any screen.
    #[test]
    fn ctrl_d_asks_before_deleting_and_only_y_deletes() {
        let mut app = FinanceApp::with_sample_data();
        app.screen = Screen::Transactions;
        let n = app.transactions.len();
        app.handle_key("Down", false, false);
        let chosen = app.selected_id.expect("Down chose nothing");
        probe::key(&mut app, &probe::ctrl(Key::D));
        assert_eq!(app.pending_delete, Some(Doomed::Transaction(chosen)));
        assert_eq!(app.transactions.len(), n, "deleted without asking");
        assert!(
            texts(&app).iter().any(|t| t == "Delete this transaction?"),
            "the question was not drawn"
        );
        probe::key(&mut app, &probe::typing("n"));
        assert_eq!(app.transactions.len(), n, "an answer of N deleted");
        assert!(
            app.pending_delete.is_none(),
            "the question outlived its answer"
        );
        probe::key(&mut app, &probe::press(Key::Delete));
        probe::key(&mut app, &probe::typing("y"));
        assert_eq!(app.transactions.len(), n - 1);
        assert!(!app.transactions.iter().any(|t| t.id == chosen));
    }

    // -- Following the user's theme -------------------------------------------

    /// The window draws in the user's colours rather than in constants of its
    /// own.
    ///
    /// Asserted on the rectangles emitted, not on the `palette` field: a field
    /// that was assigned proves nothing a user would see.
    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
        fn theme(
            mode: appearance::ThemeMode,
            contrast: Option<appearance::HighContrastScheme>,
        ) -> Palette {
            Palette::from_settings(&appearance::AppearanceSettings {
                theme_mode: mode,
                high_contrast: contrast,
                ..appearance::AppearanceSettings::default()
            })
        }

        fn fills(app: &mut FinanceApp) -> Vec<Color> {
            app.render(1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = FinanceApp::with_sample_data();

        app.theme_changed(&theme(appearance::ThemeMode::Dark, None));
        let dark = fills(&mut app);
        assert!(!dark.is_empty(), "the window drew no filled rectangles");

        app.theme_changed(&theme(appearance::ThemeMode::Light, None));
        let light = fills(&mut app);
        assert_eq!(dark.len(), light.len(), "the theme changed the layout");
        assert_ne!(
            dark, light,
            "the window drew identically on the dark and light themes, so it \
             is still painting from constants"
        );

        // High contrast is the case a hardcoded palette fails silently: the
        // user asks for maximum legibility and this window alone ignores them.
        app.theme_changed(&theme(
            appearance::ThemeMode::Dark,
            Some(appearance::HighContrastScheme::WhiteOnBlack),
        ));
        assert_ne!(
            dark,
            fills(&mut app),
            "high contrast reached every other surface but not this window"
        );
    }

    // ── Entering, changing and deleting, and the pointer ─────────────

    use guitk::probe::{self, Probe};

    impl Probe for FinanceApp {
        type Target = Target;
        type Outcome = EventResult;
        const SIZE: (f32, f32) = (1100.0, 750.0);

        /// Drawn at the app's own size, which these tests leave at `SIZE`.
        fn draw(&self, _size: (f32, f32)) -> Frame<Target> {
            self.frame()
        }

        fn click_at(
            &mut self,
            x: f32,
            y: f32,
            button: MouseButton,
            _size: (f32, f32),
        ) -> EventResult {
            self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }))
        }

        fn key_at(&mut self, key: &KeyEvent, _size: (f32, f32)) -> EventResult {
            self.handle_event(&Event::Key(key.clone()))
        }

        fn scroll_at(&mut self, x: f32, y: f32, dy: f32, _size: (f32, f32)) -> Option<EventResult> {
            Some(self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            })))
        }
    }

    /// Every string the window draws.
    fn texts(app: &FinanceApp) -> Vec<String> {
        app.frame()
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// An app holding one account and nothing else, on the transactions
    /// screen.
    fn one_account() -> FinanceApp {
        let mut app = FinanceApp::new();
        app.add_account("Everyday", AccountType::Checking, 10_000);
        app.screen = Screen::Transactions;
        app
    }

    /// A left press at `(x, y)`.
    fn press_at(app: &mut FinanceApp, x: f32, y: f32) -> EventResult {
        app.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }))
    }

    /// **Nothing could be entered**: `add_account`, `add_transaction` and
    /// `set_budget` had no caller outside the tests.
    #[test]
    fn a_transaction_is_entered_from_the_keyboard() {
        let mut app = one_account();
        probe::key(&mut app, &probe::typing("n"));
        assert!(
            matches!(app.form, Some(Form::Transaction { id: None, .. })),
            "N opened no form"
        );
        assert_eq!(
            app.field,
            FormField::Description,
            "the keys did not start in the description"
        );
        probe::type_str(&mut app, "Coffee");
        probe::key(&mut app, &probe::press(Key::Tab));
        assert_eq!(app.field, FormField::Amount);
        probe::type_str(&mut app, "4.50");
        probe::key(&mut app, &probe::press(Key::Enter));
        assert!(
            app.form.is_none(),
            "the form stayed up: {:?}",
            app.form_error
        );
        let tx = app.transactions.last().expect("nothing was added");
        assert_eq!(tx.description, "Coffee");
        assert_eq!(tx.amount, -450, "an expense is money out");
        assert_eq!(
            tx.date, app.current_date,
            "a new transaction is not dated today"
        );
        assert_eq!(tx.account_id, app.accounts[0].id);
        assert_eq!(tx.category, Category::Food);
        assert_eq!(
            app.selected_id,
            Some(tx.id),
            "the new row is not the chosen one"
        );
        assert!(
            app.visible_ids().contains(&tx.id),
            "the new row is not in the list"
        );
        assert_eq!(app.account_balance(app.accounts[0].id), 10_000 - 450);
    }

    /// A refusal says why, in the form, and keeps what was typed.
    #[test]
    fn a_form_says_what_is_wrong_and_keeps_what_was_typed() {
        let mut app = one_account();
        app.open_new_transaction();
        probe::key(&mut app, &probe::press(Key::Enter));
        assert!(
            app.form.is_some(),
            "a transaction with no description was saved"
        );
        assert_eq!(
            app.form_error.as_deref(),
            Some("A transaction needs a description")
        );
        assert!(
            texts(&app)
                .iter()
                .any(|t| t == "A transaction needs a description"),
            "the reason was not drawn"
        );

        probe::type_str(&mut app, "Rent");
        probe::key(&mut app, &probe::press(Key::Tab));
        probe::type_str(&mut app, "12.345");
        probe::key(&mut app, &probe::press(Key::Enter));
        assert_eq!(
            app.form_error.as_deref(),
            Some("An amount has at most two decimals")
        );
        assert!(app.transactions.is_empty(), "a bad amount was saved");

        app.field = FormField::Amount;
        probe::key(&mut app, &probe::ctrl(Key::A));
        probe::type_str(&mut app, "1,200");
        app.field = FormField::Date;
        probe::key(&mut app, &probe::ctrl(Key::A));
        probe::type_str(&mut app, "2026-02-30");
        probe::key(&mut app, &probe::press(Key::Enter));
        assert_eq!(
            app.form_error.as_deref(),
            Some("That is not a date; write it YYYY-MM-DD")
        );
        let Some(Form::Transaction { description, .. }) = &app.form else {
            panic!("the form went away on a refusal");
        };
        assert_eq!(description.text(), "Rent", "a refusal lost what was typed");

        probe::key(&mut app, &probe::ctrl(Key::A));
        probe::type_str(&mut app, "2026-02-28");
        assert!(
            app.form_error.is_none(),
            "typing did not clear the complaint"
        );
        probe::key(&mut app, &probe::press(Key::Enter));
        assert!(app.form.is_none(), "{:?}", app.form_error);
        let tx = app.transactions.last().expect("nothing was added");
        assert_eq!(
            (tx.amount, tx.date),
            (-120_000, SimpleDate::new(2026, 2, 28))
        );
    }

    #[test]
    fn amounts_are_read_as_people_write_them() {
        assert_eq!(parse_cents("12", false), Ok(1200));
        assert_eq!(parse_cents("12.5", false), Ok(1250));
        assert_eq!(parse_cents("12.05", false), Ok(1205));
        assert_eq!(parse_cents(" 1,234.56 ", false), Ok(123_456));
        assert_eq!(parse_cents("$3.07", false), Ok(307));
        assert_eq!(parse_cents(".5", false), Ok(50));
        assert_eq!(parse_cents("7.", false), Ok(700));
        assert_eq!(parse_cents("", false), Ok(0));
        assert_eq!(parse_cents("-25", true), Ok(-2500));
        assert_eq!(parse_cents("-$25.10", true), Ok(-2510));
        assert!(
            parse_cents("-25", false).is_err(),
            "a sign was taken where Income or Expense says the direction"
        );
        for nonsense in ["abc", "1.234", ".", "1.2.3", "12a", "-", "--5", "1 000"] {
            assert!(
                parse_cents(nonsense, true).is_err(),
                "{nonsense:?} was read as an amount"
            );
        }
        assert!(
            parse_cents("99999999999999999999", false).is_err(),
            "an overflow was not refused"
        );
        assert!(
            parse_cents("92233720368547758.08", false).is_err(),
            "an overflow was not refused"
        );
        assert_eq!(
            parse_cents("92233720368547758.07", false),
            Ok(i64::MAX),
            "the largest amount there is was refused"
        );
    }

    #[test]
    fn dates_are_read_back_and_impossible_ones_refused() {
        assert_eq!(
            SimpleDate::parse("2026-09-25"),
            Some(SimpleDate::new(2026, 9, 25))
        );
        assert_eq!(
            SimpleDate::parse(" 2026-9-5 "),
            Some(SimpleDate::new(2026, 9, 5))
        );
        assert_eq!(
            SimpleDate::parse("2024-02-29"),
            Some(SimpleDate::new(2024, 2, 29))
        );
        for impossible in [
            "2026-02-29",
            "2026-13-01",
            "2026-04-31",
            "2026-00-10",
            "2026-01-00",
            "25/09/2026",
            "2026-09",
            "2026-09-25-1",
            "",
        ] {
            assert_eq!(
                SimpleDate::parse(impossible),
                None,
                "{impossible:?} was read as a date"
            );
        }
        for d in [SimpleDate::new(2026, 1, 31), SimpleDate::new(1999, 12, 1)] {
            assert_eq!(
                SimpleDate::parse(&d.format()),
                Some(d),
                "{d:?} did not survive a round trip"
            );
        }
    }

    #[test]
    fn a_transaction_is_changed_in_place() {
        let mut app = FinanceApp::with_sample_data();
        app.screen = Screen::Transactions;
        let n = app.transactions.len();
        let id = app.visible_ids()[2];
        app.selected_id = Some(id);
        probe::key(&mut app, &probe::press(Key::Enter));
        let Some(Form::Transaction {
            id: Some(editing),
            description,
            amount,
            ..
        }) = app.form.clone()
        else {
            panic!("Enter did not open the chosen transaction: {:?}", app.form);
        };
        let before = app
            .transactions
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .clone();
        assert_eq!(editing, id);
        assert_eq!(description.text(), before.description);
        assert_eq!(
            amount.text(),
            FinanceApp::format_currency(before.amount.abs()).trim_start_matches('$')
        );
        app.field = FormField::Amount;
        probe::key(&mut app, &probe::ctrl(Key::A));
        probe::type_str(&mut app, "99.99");
        probe::key(&mut app, &probe::press(Key::Enter));
        assert!(app.form.is_none(), "{:?}", app.form_error);
        let after = app.transactions.iter().find(|t| t.id == id).unwrap();
        assert_eq!(after.amount, if before.amount > 0 { 9999 } else { -9999 });
        assert_eq!(after.description, before.description);
        assert_eq!(after.date, before.date);
        assert_eq!(
            app.transactions.len(),
            n,
            "a change added or lost a transaction"
        );
    }

    #[test]
    fn money_in_is_not_filed_under_food() {
        let mut app = one_account();
        app.open_new_transaction();
        app.field = FormField::Kind;
        probe::key(&mut app, &probe::press(Key::Right));
        let Some(Form::Transaction {
            income, category, ..
        }) = app.form.clone()
        else {
            panic!("the form went away");
        };
        assert!(income, "Right did not make it income");
        assert_eq!(category, Category::Income);
        probe::key(&mut app, &probe::press(Key::Space));
        let Some(Form::Transaction {
            income, category, ..
        }) = app.form.clone()
        else {
            panic!("the form went away");
        };
        assert!(!income);
        assert_eq!(category, Category::Food);
        // A category that goes either way is left where the user put it.
        if let Some(Form::Transaction { category, .. }) = app.form.as_mut() {
            *category = Category::Savings;
        }
        probe::key(&mut app, &probe::press(Key::Left));
        let Some(Form::Transaction {
            income, category, ..
        }) = app.form.clone()
        else {
            panic!("the form went away");
        };
        assert!(income);
        assert_eq!(category, Category::Savings);
    }

    #[test]
    fn a_zero_amount_is_refused() {
        let mut app = one_account();
        app.open_new_transaction();
        probe::type_str(&mut app, "Nothing");
        probe::key(&mut app, &probe::press(Key::Tab));
        probe::type_str(&mut app, "0.00");
        probe::key(&mut app, &probe::press(Key::Enter));
        assert_eq!(app.form_error.as_deref(), Some("The amount is zero"));
        assert!(
            app.transactions.is_empty(),
            "a transaction of nothing was saved"
        );
    }

    /// Outside the question a press keeps what was asked about, and reaches
    /// nothing behind it.
    #[test]
    fn a_press_outside_the_question_keeps_and_reaches_nothing() {
        let mut app = FinanceApp::with_sample_data();
        app.screen = Screen::Transactions;
        let ids = app.visible_ids();
        let (chosen, other) = (ids[0], ids[1]);
        app.selected_id = Some(chosen);
        let behind = probe::rect_of(&app, Target::TxRow(other)).unwrap();
        probe::key(&mut app, &probe::press(Key::Delete));
        assert_eq!(app.pending_delete, Some(Doomed::Transaction(chosen)));
        let n = app.transactions.len();
        let (x, y) = (behind.x + 10.0, behind.y + behind.h / 2.0);
        assert_eq!(app.frame().hit_test(x, y), Some(Target::QuestionBackdrop));
        press_at(&mut app, x, y);
        assert!(
            app.pending_delete.is_none(),
            "a press outside left the question up"
        );
        assert_eq!(app.transactions.len(), n, "a press outside deleted");
        assert_eq!(
            app.selected_id,
            Some(chosen),
            "the press reached the row behind"
        );
        probe::key(&mut app, &probe::press(Key::Delete));
        let card = probe::rect_of(&app, Target::QuestionCard).unwrap();
        assert_eq!(
            press_at(&mut app, card.x + 4.0, card.y + 4.0),
            EventResult::Ignored
        );
        assert!(
            app.pending_delete.is_some(),
            "a press on the card answered it"
        );
    }

    #[test]
    fn income_is_money_in() {
        let mut app = one_account();
        app.open_new_transaction();
        probe::type_str(&mut app, "Salary");
        probe::key(&mut app, &probe::press(Key::Tab));
        probe::type_str(&mut app, "2500");
        probe::key(&mut app, &probe::press(Key::Tab));
        assert_eq!(app.field, FormField::Kind);
        probe::key(&mut app, &probe::press(Key::Right));
        probe::key(&mut app, &probe::press(Key::Enter));
        let tx = app.transactions.last().expect("nothing was added");
        assert_eq!(tx.amount, 250_000);
        assert_eq!(tx.category, Category::Income);
        assert_eq!(app.month_income(), 250_000);
    }

    #[test]
    fn tab_walks_the_fields_and_comes_round() {
        let mut app = one_account();
        app.open_new_transaction();
        let fields = app.form.as_ref().unwrap().fields().to_vec();
        let start = fields.iter().position(|f| *f == app.field).unwrap();
        for step in 1..=fields.len() {
            probe::key(&mut app, &probe::press(Key::Tab));
            assert_eq!(app.field, fields[(start + step) % fields.len()]);
        }
        probe::key(&mut app, &probe::shift(Key::Tab));
        assert_eq!(app.field, fields[(start + fields.len() - 1) % fields.len()]);
    }

    /// A form takes every key: a `3` in an amount switched to the budgets.
    #[test]
    fn a_digit_typed_into_a_form_is_not_a_screen_switch() {
        let mut app = one_account();
        app.open_new_transaction();
        app.field = FormField::Amount;
        probe::type_str(&mut app, "3");
        assert_eq!(app.screen, Screen::Transactions);
        let Some(Form::Transaction { amount, .. }) = &app.form else {
            panic!("the form went away");
        };
        assert_eq!(amount.text(), "3");
    }

    #[test]
    fn escape_leaves_a_form_without_saving() {
        let mut app = one_account();
        app.open_new_transaction();
        probe::type_str(&mut app, "Half typed");
        probe::key(&mut app, &probe::press(Key::Escape));
        assert!(app.form.is_none());
        assert!(app.transactions.is_empty(), "Escape saved");
    }

    #[test]
    fn a_transaction_with_nowhere_to_go_asks_for_an_account_first() {
        let mut app = FinanceApp::new();
        app.screen = Screen::Transactions;
        probe::key(&mut app, &probe::typing("n"));
        assert!(matches!(app.form, Some(Form::Account { id: None, .. })));
        assert!(
            app.form_error
                .as_deref()
                .is_some_and(|e| e.contains("account")),
            "the account form came up without saying why"
        );
    }

    #[test]
    fn an_account_is_entered_with_what_it_held() {
        let mut app = FinanceApp::new();
        app.screen = Screen::Accounts;
        probe::key(&mut app, &probe::typing("n"));
        assert!(matches!(app.form, Some(Form::Account { id: None, .. })));
        probe::type_str(&mut app, "Visa");
        probe::key(&mut app, &probe::press(Key::Tab));
        probe::key(&mut app, &probe::press(Key::Right));
        probe::key(&mut app, &probe::press(Key::Right));
        probe::key(&mut app, &probe::press(Key::Tab));
        probe::type_str(&mut app, "-250.00");
        probe::key(&mut app, &probe::press(Key::Enter));
        assert!(app.form.is_none(), "{:?}", app.form_error);
        let account = &app.accounts[0];
        assert_eq!(account.name, "Visa");
        assert_eq!(account.account_type, AccountType::CreditCard);
        assert_eq!(account.initial_balance, -25_000);
        assert_eq!(app.selected_account, Some(account.id));
        assert_eq!(app.total_balance(), -25_000);
    }

    /// On the accounts screen N adds an account, whether or not there is
    /// one to put a transaction in.
    #[test]
    fn n_on_the_accounts_screen_adds_an_account() {
        let mut app = one_account();
        app.screen = Screen::Accounts;
        probe::key(&mut app, &probe::typing("n"));
        assert!(
            matches!(app.form, Some(Form::Account { id: None, .. })),
            "N on the accounts screen opened {:?}",
            app.form
        );
        app.form = None;
        app.screen = Screen::Transactions;
        probe::key(&mut app, &probe::typing("n"));
        assert!(matches!(app.form, Some(Form::Transaction { id: None, .. })));
    }

    #[test]
    fn an_account_is_changed_in_place() {
        let mut app = FinanceApp::with_sample_data();
        app.screen = Screen::Accounts;
        let id = app.accounts[1].id;
        probe::click(&mut app, Target::AccountRow(id));
        assert_eq!(app.selected_account, Some(id));
        assert!(app.form.is_none(), "one press opened the form");
        probe::click(&mut app, Target::AccountRow(id));
        assert!(
            matches!(app.form, Some(Form::Account { id: Some(x), .. }) if x == id),
            "a second press did not open the account"
        );
        probe::key(&mut app, &probe::ctrl(Key::A));
        probe::type_str(&mut app, "Rainy day");
        probe::click(&mut app, Target::Save);
        assert!(app.form.is_none(), "{:?}", app.form_error);
        assert_eq!(app.accounts[1].name, "Rainy day");
        assert_eq!(app.accounts.len(), 4);
    }

    #[test]
    fn deleting_an_account_asks_and_takes_its_transactions() {
        let mut app = FinanceApp::with_sample_data();
        app.screen = Screen::Accounts;
        let id = app.accounts[2].id;
        let in_it = app
            .transactions
            .iter()
            .filter(|t| t.account_id == id)
            .count();
        assert!(in_it > 1, "the sample's credit card should hold several");
        probe::click(&mut app, Target::AccountRow(id));
        probe::click(&mut app, Target::Delete);
        assert_eq!(app.pending_delete, Some(Doomed::Account(id)));
        let said = format!("Its {in_it} transactions go with it, and this cannot be undone.");
        assert!(
            texts(&app).contains(&said),
            "the question did not say what goes with it"
        );
        probe::click(&mut app, Target::KeepIt);
        assert_eq!(app.accounts.len(), 4, "Keep deleted");
        probe::click(&mut app, Target::Delete);
        probe::click(&mut app, Target::ConfirmDelete);
        assert!(!app.accounts.iter().any(|a| a.id == id));
        assert!(
            !app.transactions.iter().any(|t| t.account_id == id),
            "its transactions were left behind, in no account"
        );
        assert!(
            app.selected_account.is_some_and(|s| s != id),
            "nothing is chosen after the delete"
        );
    }

    #[test]
    fn nothing_is_deleted_from_a_screen_that_does_not_show_it() {
        let mut app = FinanceApp::with_sample_data();
        app.screen = Screen::Transactions;
        app.handle_key("Down", false, false);
        assert!(app.selected_id.is_some());
        for screen in [Screen::Dashboard, Screen::Budgets, Screen::Reports] {
            app.screen = screen;
            // Each key on its own: a question the first raised would take
            // the second as its answer, and hide that it had been asked.
            for key in [probe::ctrl(Key::D), probe::press(Key::Delete)] {
                probe::key(&mut app, &key);
                assert!(
                    app.pending_delete.is_none(),
                    "{:?} on {screen:?} asked to delete a transaction it does not show",
                    key.key
                );
            }
            probe::key(&mut app, &probe::press(Key::Enter));
            if screen != Screen::Budgets {
                assert!(
                    app.form.is_none(),
                    "Enter on {screen:?} opened a transaction it does not show"
                );
            }
            app.form = None;
        }
    }

    /// Only budgets already set were listed, so none could be set at all.
    #[test]
    fn every_category_can_be_given_a_budget() {
        let mut app = one_account();
        app.screen = Screen::Budgets;
        for i in 0..Category::EXPENSE_CATS.len() {
            assert!(
                probe::rect_of(&app, Target::BudgetRow(i)).is_some(),
                "{:?} has no row",
                Category::EXPENSE_CATS[i]
            );
        }
        assert!(texts(&app).iter().any(|t| t == "No budget"));
        let last = Category::EXPENSE_CATS.len() - 1;
        probe::click(&mut app, Target::BudgetRow(last));
        assert_eq!(app.selected_budget, last);
        assert!(app.form.is_none(), "one press opened the form");
        probe::click(&mut app, Target::BudgetRow(last));
        assert!(matches!(
            app.form,
            Some(Form::Budget {
                category: Category::Other,
                ..
            })
        ));
        probe::type_str(&mut app, "300");
        probe::key(&mut app, &probe::press(Key::Enter));
        let limit = |app: &FinanceApp| {
            app.budgets
                .iter()
                .find(|b| b.category == Category::Other)
                .map(|b| b.monthly_limit)
        };
        assert_eq!(limit(&app), Some(30_000));
        // Emptied, it comes off.
        probe::key(&mut app, &probe::press(Key::Enter));
        probe::key(&mut app, &probe::ctrl(Key::A));
        probe::key(&mut app, &probe::press(Key::Backspace));
        probe::key(&mut app, &probe::press(Key::Enter));
        assert_eq!(limit(&app), None, "an emptied budget stayed");
    }

    #[test]
    fn the_sidebar_switches_screens() {
        let mut app = FinanceApp::with_sample_data();
        for screen in Screen::ALL.iter().rev() {
            assert_eq!(
                probe::click(&mut app, Target::Screen(*screen)),
                EventResult::Consumed
            );
            assert_eq!(app.screen, *screen);
        }
    }

    #[test]
    fn the_month_arrows_are_buttons() {
        let mut app = FinanceApp::with_sample_data();
        assert!(
            probe::rect_of(&app, Target::ThisMonth).is_none(),
            "This month is offered on this month"
        );
        probe::click(&mut app, Target::MonthPrev);
        assert_eq!((app.view_month.year, app.view_month.month), (2026, 4));
        probe::click(&mut app, Target::MonthNext);
        probe::click(&mut app, Target::MonthNext);
        assert_eq!((app.view_month.year, app.view_month.month), (2026, 6));
        probe::click(&mut app, Target::ThisMonth);
        assert_eq!((app.view_month.year, app.view_month.month), (2026, 5));
    }

    #[test]
    fn a_row_press_chooses_it_and_a_second_changes_it() {
        let mut app = FinanceApp::with_sample_data();
        app.screen = Screen::Transactions;
        let id = app.visible_ids()[3];
        probe::click(&mut app, Target::TxRow(id));
        assert_eq!(app.selected_id, Some(id));
        assert!(app.form.is_none(), "one press opened the form");
        probe::click(&mut app, Target::TxRow(id));
        assert!(matches!(app.form, Some(Form::Transaction { id: Some(x), .. }) if x == id));
    }

    #[test]
    fn a_press_behind_a_form_reaches_nothing() {
        let mut app = FinanceApp::with_sample_data();
        app.screen = Screen::Transactions;
        let row = app.visible_ids()[0];
        let behind = probe::rect_of(&app, Target::TxRow(row)).unwrap();
        app.open_new_transaction();
        let (x, y) = (behind.x + 10.0, behind.y + behind.h / 2.0);
        assert_eq!(app.frame().hit_test(x, y), Some(Target::FormBackdrop));
        let chosen = app.selected_id;
        assert_eq!(press_at(&mut app, x, y), EventResult::Ignored);
        assert!(app.form.is_some(), "a press behind the form closed it");
        assert_eq!(
            app.selected_id, chosen,
            "a press behind the form chose a row"
        );
    }

    #[test]
    fn a_form_answers_the_pointer() {
        let mut app = one_account();
        app.add_account("Savings", AccountType::Savings, 0);
        probe::click(&mut app, Target::NewTransaction);
        probe::type_str(&mut app, "Lunch");
        probe::click(&mut app, Target::Field(FormField::Amount));
        assert_eq!(
            app.field,
            FormField::Amount,
            "a press did not put the keys in the field"
        );
        probe::type_str(&mut app, "12");
        probe::click(&mut app, Target::StepForward(FormField::Category));
        probe::click(&mut app, Target::StepForward(FormField::Category));
        probe::click(&mut app, Target::StepBack(FormField::Category));
        probe::click(&mut app, Target::Field(FormField::Account));
        probe::click(&mut app, Target::Field(FormField::Recurring));
        probe::click(&mut app, Target::Save);
        assert!(app.form.is_none(), "{:?}", app.form_error);
        let tx = app.transactions.last().expect("nothing was added");
        assert_eq!(tx.description, "Lunch");
        assert_eq!(tx.amount, -1200);
        assert_eq!(tx.category, Category::ALL[1]);
        assert_eq!(
            tx.account_id, app.accounts[1].id,
            "the account press did not step it"
        );
        assert!(tx.recurring);
        probe::click(&mut app, Target::NewTransaction);
        probe::click(&mut app, Target::Cancel);
        assert!(app.form.is_none(), "Cancel left the form up");
    }

    #[test]
    fn a_recent_transaction_opens_in_the_list() {
        let mut app = FinanceApp::with_sample_data();
        app.category_filter = Some(Category::Housing);
        let newest = app
            .month_transactions()
            .iter()
            .max_by(|a, b| a.date.cmp(&b.date).then(a.id.cmp(&b.id)))
            .unwrap()
            .id;
        probe::click(&mut app, Target::RecentRow(newest));
        assert_eq!(app.screen, Screen::Transactions);
        assert_eq!(app.selected_id, Some(newest));
        assert!(
            app.visible_ids().contains(&newest),
            "the filter still hides it"
        );
    }

    #[test]
    fn the_search_box_takes_the_keys_and_reaches_every_month() {
        let mut app = FinanceApp::with_sample_data();
        app.screen = Screen::Transactions;
        probe::click(&mut app, Target::MonthNext);
        probe::click(&mut app, Target::MonthNext);
        assert!(app.visible_ids().is_empty(), "July has something in it");
        probe::click(&mut app, Target::Search);
        assert!(app.search_active);
        probe::type_str(&mut app, "grocery");
        assert_eq!(app.search_query, "grocery");
        let found = app.visible_ids();
        assert_eq!(found.len(), 2, "the search did not reach May from July");
        assert!(texts(&app).iter().any(|t| t == "every month"));
        // A press elsewhere takes the keys out of the box and keeps the search.
        probe::click(&mut app, Target::TxRow(found[0]));
        assert!(!app.search_active);
        assert_eq!(app.search_query, "grocery");
        assert_eq!(app.selected_id, Some(found[0]));
    }

    /// It listed every month in the order things were typed in, under a
    /// header naming one month.
    #[test]
    fn the_list_is_the_month_on_screen_newest_first() {
        let mut app = FinanceApp::with_sample_data();
        app.add_transaction(
            SimpleDate::new(2026, 6, 3),
            "June thing",
            -100,
            Category::Other,
            1,
            "",
            false,
        );
        let late = app.add_transaction(
            SimpleDate::new(2026, 5, 9),
            "Entered late",
            -100,
            Category::Other,
            1,
            "",
            false,
        );
        let listed: Vec<(SimpleDate, u32)> = app
            .filtered_transactions()
            .iter()
            .map(|(_, t)| (t.date, t.id))
            .collect();
        assert!(
            listed.iter().all(|(d, _)| d.month == 5),
            "another month is in May's list"
        );
        assert!(
            listed.windows(2).all(|w| w[0] >= w[1]),
            "not newest first: {listed:?}"
        );
        let at = listed.iter().position(|(_, id)| *id == late).unwrap();
        assert_eq!(
            listed[at].0,
            SimpleDate::new(2026, 5, 9),
            "listed by when it was typed"
        );
        assert!(listed[at + 1].0 <= SimpleDate::new(2026, 5, 9));
        assert!(listed[at - 1].0 >= SimpleDate::new(2026, 5, 9));
        probe::click(&mut app, Target::MonthNext);
        assert_eq!(app.visible_ids().len(), 1, "June's list is not June's");
    }

    #[test]
    fn the_category_chip_shows_one_category_then_the_next() {
        let mut app = FinanceApp::with_sample_data();
        app.screen = Screen::Transactions;
        probe::click(&mut app, Target::FilterChip);
        assert_eq!(app.category_filter, Some(Category::ALL[0]));
        probe::click(&mut app, Target::FilterChip);
        assert_eq!(app.category_filter, Some(Category::ALL[1]));
        for _ in 2..Category::ALL.len() {
            probe::click(&mut app, Target::FilterChip);
        }
        assert_eq!(app.category_filter, Some(Category::Other));
        probe::click(&mut app, Target::FilterChip);
        assert_eq!(app.category_filter, None);
    }

    /// **Each row is a key this program actually answers.**
    #[test]
    fn every_advertised_key_does_something() {
        let states = || {
            let dash = FinanceApp::with_sample_data();
            let mut list = FinanceApp::with_sample_data();
            list.screen = Screen::Transactions;
            list.handle_key("Down", false, false);
            list.handle_key("Down", false, false);
            let mut form = one_account();
            form.open_new_transaction();
            let mut away = FinanceApp::with_sample_data();
            away.view_month = SimpleDate::new(2026, 1, 1);
            vec![dash, list, form, away]
        };
        for (row, what) in SHORTCUTS {
            let strokes = guitk::shortcut::keystrokes(row).unwrap_or_else(|e| panic!("{e}"));
            for stroke in strokes {
                let taken = states().iter_mut().any(|app| {
                    app.handle_event(&Event::Key(stroke.clone())) == EventResult::Consumed
                });
                assert!(
                    taken,
                    "the list offers {row:?} ({what}) and nothing takes {:?}",
                    stroke.key
                );
            }
        }
    }

    #[test]
    fn f1_shows_the_keys_and_a_press_puts_them_away() {
        let mut app = FinanceApp::with_sample_data();
        probe::key(&mut app, &probe::press(Key::F1));
        assert!(app.show_help);
        assert!(texts(&app).iter().any(|t| t == "This list"));
        let screen = app.screen;
        let nav = FinanceApp::nav_rect(2);
        press_at(&mut app, nav.x + 10.0, nav.y + 10.0);
        assert!(!app.show_help, "a press left the card up");
        assert_eq!(app.screen, screen, "the press went through the card");
        probe::click(&mut app, Target::Help);
        assert!(app.show_help, "the Keys button does not show them");
    }

    #[test]
    fn a_button_lights_under_the_pointer() {
        let mut app = FinanceApp::with_sample_data();
        let r = probe::rect_of(&app, Target::MonthNext).unwrap();
        let fill_at = |app: &FinanceApp| {
            app.frame().commands().iter().find_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } if (*x - r.x).abs() < 0.01
                    && (*y - r.y).abs() < 0.01
                    && (*width - r.w).abs() < 0.01
                    && (*height - r.h).abs() < 0.01 =>
                {
                    Some(*color)
                }
                _ => None,
            })
        };
        let before = fill_at(&app);
        assert!(before.is_some(), "no fill under the button");
        let moved = app.handle_event(&Event::Mouse(MouseEvent {
            x: r.x + 2.0,
            y: r.y + 2.0,
            kind: MouseEventKind::Move,
        }));
        assert_eq!(moved, EventResult::Consumed);
        assert_ne!(fill_at(&app), before, "the button did not light");
        let left = app.handle_event(&Event::Mouse(MouseEvent {
            x: 0.0,
            y: 0.0,
            kind: MouseEventKind::Leave,
        }));
        assert_eq!(left, EventResult::Consumed);
        assert_eq!(fill_at(&app), before);
    }

    /// A list of forty in a month: the rows past the bottom were not drawn,
    /// and the arrows stopped at the last row on screen.
    fn forty_this_month() -> FinanceApp {
        let mut app = one_account();
        let (today, account) = (app.current_date, app.accounts[0].id);
        for i in 0..40 {
            app.add_transaction(
                today,
                &format!("Item {i}"),
                -100,
                Category::Other,
                account,
                "",
                false,
            );
        }
        app
    }

    #[test]
    fn the_wheel_scrolls_a_long_list() {
        let mut app = forty_this_month();
        let (_, rows) = app.tx_pane();
        assert!(rows < 40, "the list is not long enough to scroll");
        assert_eq!(
            probe::scroll_at_point(&mut app, Target::TxList, -3.0),
            EventResult::Consumed
        );
        assert!(app.tx_scroll > 0, "the wheel did not scroll");
        // Held to the list's own area: the rows ran on under the status bar.
        for (target, r) in app.frame().hits() {
            if matches!(target, Target::TxRow(_)) {
                assert!(
                    r.bottom() <= app.content_bottom() + 0.5,
                    "{target:?} reaches under the status bar: {r:?}"
                );
            }
        }
        let first = app.visible_ids()[app.tx_scroll];
        assert!(probe::rect_of(&app, Target::TxRow(first)).is_some());
        let top = app.visible_ids()[0];
        assert!(
            probe::rect_of(&app, Target::TxRow(top)).is_none(),
            "a row scrolled away is still there to press"
        );
        for _ in 0..50 {
            probe::scroll_at_point(&mut app, Target::TxList, -3.0);
        }
        assert_eq!(app.tx_scroll, 40 - rows, "the wheel ran past the end");
        for _ in 0..50 {
            probe::scroll_at_point(&mut app, Target::TxList, 3.0);
        }
        assert_eq!(app.tx_scroll, 0);
    }

    #[test]
    fn the_chosen_row_stays_on_screen() {
        let mut app = forty_this_month();
        for _ in 0..30 {
            probe::key(&mut app, &probe::press(Key::Down));
        }
        let chosen = app.selected_id.unwrap();
        assert_eq!(
            app.visible_ids().iter().position(|v| *v == chosen),
            Some(29)
        );
        let (pane, _) = app.tx_pane();
        let r = probe::rect_of(&app, Target::TxRow(chosen)).expect("the chosen row is off screen");
        assert!(
            r.bottom() <= pane.bottom() + 0.5,
            "the chosen row is cut off: {r:?}"
        );
        probe::key(&mut app, &probe::press(Key::PageUp));
        probe::key(&mut app, &probe::press(Key::PageUp));
        probe::key(&mut app, &probe::press(Key::PageUp));
        assert_eq!(app.selected_id, app.visible_ids().first().copied());
        assert_eq!(app.tx_scroll, 0);
        probe::key(&mut app, &probe::press(Key::PageDown));
        assert!(probe::rect_of(&app, Target::TxRow(app.selected_id.unwrap())).is_some());
    }

    #[test]
    fn the_budgets_scroll_in_a_short_window() {
        let mut app = one_account();
        app.screen = Screen::Budgets;
        app.height = 420.0;
        let (_, rows) = app.budget_pane();
        assert!(rows < Category::EXPENSE_CATS.len());
        for _ in 0..Category::EXPENSE_CATS.len() {
            probe::key(&mut app, &probe::press(Key::Down));
        }
        let last = Category::EXPENSE_CATS.len() - 1;
        assert_eq!(app.selected_budget, last);
        let r =
            probe::rect_of(&app, Target::BudgetRow(last)).expect("the chosen budget is off screen");
        assert!(r.bottom() <= app.content_bottom() + 0.5);
        assert_eq!(
            probe::scroll_at_point(&mut app, Target::BudgetList, 30.0),
            EventResult::Consumed
        );
        assert_eq!(app.budget_scroll, 0);
    }

    /// The notice was drawn at the top of the window and then the sidebar
    /// and the header were drawn over it.
    #[test]
    fn the_first_run_notice_is_drawn_where_it_can_be_read() {
        let app = FinanceApp::new();
        let cmds = app.frame().into_tree().commands;
        for line in NO_DATA_LINES {
            let at = cmds
                .iter()
                .position(|c| matches!(c, RenderCommand::Text { text, .. } if text == line))
                .unwrap_or_else(|| panic!("never drew {line:?}"));
            let RenderCommand::Text { x, y, .. } = &cmds[at] else {
                unreachable!()
            };
            assert!(
                *x >= FinanceApp::SIDEBAR_W && *y >= FinanceApp::HEADER_H,
                "{line:?} is drawn under the sidebar or the header, at ({x}, {y})"
            );
            for later in &cmds[at + 1..] {
                if let RenderCommand::FillRect {
                    x: fx,
                    y: fy,
                    width,
                    height,
                    color,
                    ..
                } = later
                {
                    let covers = *fx <= *x && *x < fx + width && *fy <= *y && *y < fy + height;
                    assert!(!(covers && color.a == 255), "{line:?} is painted over");
                }
            }
        }
    }

    #[test]
    fn a_fresh_window_begins_with_an_account() {
        let mut app = FinanceApp::new();
        probe::click(&mut app, Target::NewAccount);
        probe::type_str(&mut app, "Everyday");
        probe::click(&mut app, Target::Save);
        assert_eq!(app.accounts.len(), 1, "{:?}", app.form_error);
        assert!(
            !texts(&app).iter().any(|t| t == NO_DATA_LINES[0]),
            "the empty-window notice outlived the emptiness"
        );
        probe::click(&mut app, Target::NewTransaction);
        assert!(
            matches!(app.form, Some(Form::Transaction { account: Some(a), .. }) if a == app.accounts[0].id),
            "a new transaction does not go in the account there is"
        );
    }

    /// The total was drawn half under the status bar.
    #[test]
    fn the_total_is_above_the_status_bar() {
        let app = FinanceApp::with_sample_data();
        let total = FinanceApp::format_currency(app.total_balance());
        let y = app
            .frame()
            .commands()
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text {
                    text, y, font_size, ..
                } if *text == total => Some(y + font_size),
                _ => None,
            })
            .expect("the total is not drawn");
        assert!(
            y <= app.height - FinanceApp::STATUS_H,
            "the total runs under the status bar"
        );
    }

    #[test]
    fn today_is_the_clocks() {
        let before = today_from_clock();
        let app = FinanceApp::new();
        let after = today_from_clock();
        assert!(before.is_some(), "the clock cannot be read");
        assert!(
            Some(app.current_date) == before || Some(app.current_date) == after,
            "today is {:?}, the clock says {before:?}",
            app.current_date
        );
        assert!(app.view_month.same_month(&app.current_date));
        assert_eq!(app.view_month.day, 1);
    }

    #[test]
    fn a_tick_on_the_same_day_draws_nothing() {
        let mut app = FinanceApp::new();
        app.handle_event(&Event::Tick { elapsed_ms: 1000 });
        assert_eq!(
            app.handle_event(&Event::Tick { elapsed_ms: 1000 }),
            EventResult::Ignored,
            "a tick that changed nothing drew a frame"
        );
        let wake = app
            .tick_interval()
            .expect("the window never wakes to notice midnight");
        assert!(
            wake >= Duration::from_secs(1) && wake <= Duration::from_hours(1),
            "{wake:?}"
        );
    }

    #[test]
    fn midnight_moves_today_and_the_month_on_screen_with_it() {
        let mut app = FinanceApp::new();
        app.current_date = SimpleDate::new(2000, 1, 31);
        app.view_month = SimpleDate::new(2000, 1, 1);
        assert_eq!(
            app.handle_event(&Event::Tick { elapsed_ms: 1000 }),
            EventResult::Consumed
        );
        assert_eq!(Some(app.current_date), today_from_clock());
        assert!(
            app.view_month.same_month(&app.current_date),
            "the view stayed on the old month"
        );
        // A view the user moved elsewhere stays where they put it.
        app.current_date = SimpleDate::new(2000, 1, 31);
        app.view_month = SimpleDate::new(1999, 6, 1);
        app.handle_event(&Event::Tick { elapsed_ms: 1000 });
        assert_eq!(app.view_month, SimpleDate::new(1999, 6, 1));
    }

    // ── Keeping the ledger ──────────────────────────────────────────

    /// **Nothing was kept**: not a single `fs::` call in the crate, so a
    /// month of entries was gone when the window closed.
    #[test]
    fn what_is_entered_is_there_next_time() {
        settingsfile::testing::with_scratch_config("finance-kept", |_| {
            let mut app = FinanceApp::from_settings();
            assert!(app.ledger_error.is_none(), "{:?}", app.ledger_error);
            assert!(
                texts(&app)
                    .iter()
                    .any(|t| t.starts_with("What you enter is kept in ")),
                "the first-run card does not say where things are kept"
            );
            probe::click(&mut app, Target::NewAccount);
            probe::type_str(&mut app, "Everyday");
            probe::key(&mut app, &probe::press(Key::Enter));
            app.screen = Screen::Transactions;
            probe::key(&mut app, &probe::typing("n"));
            probe::type_str(&mut app, "Caf\u{e9} au lait");
            probe::key(&mut app, &probe::press(Key::Tab));
            probe::type_str(&mut app, "3.20");
            probe::key(&mut app, &probe::press(Key::Enter));
            app.screen = Screen::Budgets;
            probe::key(&mut app, &probe::press(Key::Enter));
            probe::type_str(&mut app, "450");
            probe::key(&mut app, &probe::press(Key::Enter));
            assert!(app.form.is_none(), "{:?}", app.form_error);
            assert!(app.ledger_error.is_none(), "{:?}", app.ledger_error);
            assert_eq!(
                (
                    app.accounts.len(),
                    app.transactions.len(),
                    app.budgets.len()
                ),
                (1, 1, 1)
            );

            let mut again = FinanceApp::from_settings();
            assert!(again.ledger_error.is_none(), "{:?}", again.ledger_error);
            assert_eq!(again.accounts, app.accounts);
            assert_eq!(again.transactions, app.transactions);
            assert_eq!(again.budgets, app.budgets);
            let (today, account) = (again.current_date, again.accounts[0].id);
            let id = again.add_transaction(today, "Next", -1, Category::Other, account, "", false);
            assert!(
                !app.transactions.iter().any(|t| t.id == id),
                "a new transaction reused a kept one's number"
            );
        });
    }

    #[test]
    fn deleting_is_kept_too() {
        settingsfile::testing::with_scratch_config("finance-deleted", |_| {
            let mut app = FinanceApp::from_settings();
            let today = app.current_date;
            let account = app.add_account("Wallet", AccountType::Cash, 0);
            let kept =
                app.add_transaction(today, "Kept", -100, Category::Other, account, "", false);
            let gone =
                app.add_transaction(today, "Gone", -200, Category::Other, account, "", false);
            app.after_change();
            app.screen = Screen::Transactions;
            app.selected_id = Some(gone);
            probe::key(&mut app, &probe::press(Key::Delete));
            probe::key(&mut app, &probe::typing("y"));
            let again = FinanceApp::from_settings();
            let ids: Vec<u32> = again.transactions.iter().map(|t| t.id).collect();
            assert_eq!(ids, vec![kept]);
        });
    }

    #[test]
    fn a_window_made_by_new_keeps_nothing() {
        settingsfile::testing::with_scratch_config("finance-quiet", |dir| {
            let mut app = one_account();
            app.open_new_transaction();
            probe::type_str(&mut app, "Test");
            probe::key(&mut app, &probe::press(Key::Tab));
            probe::type_str(&mut app, "1");
            probe::key(&mut app, &probe::press(Key::Enter));
            assert_eq!(app.transactions.len(), 1, "{:?}", app.form_error);
            assert!(
                !dir.join("slateos").join("finance").exists(),
                "a window made by new() wrote the user's ledger"
            );
        });
    }

    #[test]
    fn a_ledger_that_cannot_be_read_is_left_as_it_is() {
        settingsfile::testing::with_scratch_config("finance-broken", |_| {
            let path = ledger_path().unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let broken = format!(
                "{LEDGER_HEADER}\naccount\t1\tchecking\t0\tEveryday\ntx\t1\t2026-02-30\t-100\tfood\t1\tn\tLunch\t\n"
            );
            std::fs::write(&path, &broken).unwrap();
            let mut app = FinanceApp::from_settings();
            let error = app
                .ledger_error
                .clone()
                .expect("an unreadable ledger was taken without a word");
            assert!(error.contains("line 3"), "{error}");
            assert!(texts(&app).contains(&error), "the refusal is not on screen");
            assert!(
                app.accounts.is_empty(),
                "half a ledger was taken for the whole"
            );
            app.add_account("New", AccountType::Cash, 0);
            app.after_change();
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                broken,
                "the unreadable ledger was saved over"
            );
        });
    }

    #[test]
    fn a_save_that_fails_says_so_and_the_next_one_clears_it() {
        settingsfile::testing::with_scratch_config("finance-refused", |_| {
            let mut app = FinanceApp::from_settings();
            let path = ledger_path().unwrap();
            // A directory where the file goes, so the write cannot land.
            std::fs::create_dir_all(&path).unwrap();
            app.add_account("Wallet", AccountType::Cash, 0);
            app.after_change();
            let error = app
                .ledger_error
                .clone()
                .expect("a failed save said nothing");
            assert!(error.starts_with("Not saved to "), "{error}");
            assert!(texts(&app).contains(&error), "the failure is not on screen");
            std::fs::remove_dir(&path).unwrap();
            app.after_change();
            assert!(app.ledger_error.is_none(), "{:?}", app.ledger_error);
            assert!(path.is_file(), "the second save wrote nothing");
        });
    }

    #[test]
    fn the_ledger_reads_back_what_it_wrote_whatever_the_text() {
        let awkward = [
            "plain",
            "tab\there",
            "new\nline",
            "back\\slash",
            "\\t written out",
            "caf\u{e9} \u{1F4B0}",
            "",
            "cr\rhere",
            "ends in \\",
        ];
        let mut app = FinanceApp::new();
        for (i, name) in awkward.iter().enumerate() {
            let account = app.add_account(name, AccountType::ALL[i % 5], -1000 * i as i64);
            app.add_transaction(
                SimpleDate::new(2026, 9, 25),
                name,
                -1 - i as i64,
                Category::ALL[i % 12],
                account,
                name,
                i % 2 == 0,
            );
        }
        app.set_budget(Category::Food, 12_345);
        app.set_budget(Category::Other, 1);
        let text = ledger_text(&app.accounts, &app.budgets, &app.transactions);
        assert_eq!(
            text.lines().count(),
            1 + awkward.len() * 2 + 2,
            "a field broke a line"
        );
        let back = parse_ledger(&text).unwrap();
        assert_eq!(back.accounts, app.accounts);
        assert_eq!(back.transactions, app.transactions);
        assert_eq!(back.budgets, app.budgets);
    }

    #[test]
    fn a_ledger_is_refused_whole_and_says_where() {
        let head = LEDGER_HEADER;
        let account = "account\t1\tchecking\t0\tA";
        let cases: [(String, &str); 15] = [
            (String::new(), "first line"),
            (format!("{account}\n"), "first line"),
            (
                String::from("# SlateOS finance ledger, format 2\n"),
                "newer",
            ),
            (format!("{head}\nwhat\t1\n"), "line 2"),
            (format!("{head}\naccount\tx\tchecking\t0\tA\n"), "line 2"),
            (format!("{head}\naccount\t1\tgold\t0\tA\n"), "line 2"),
            (
                format!("{head}\n{account}\naccount\t1\tcash\t0\tB\n"),
                "line 3",
            ),
            (format!("{head}\naccount\t1\tchecking\t0\tA\\q\n"), "line 2"),
            (format!("{head}\naccount\t1\tchecking\t0.5\tA\n"), "line 2"),
            (format!("{head}\nbudget\tfood\t0\n"), "line 2"),
            (
                format!("{head}\nbudget\tfood\t100\nbudget\tfood\t200\n"),
                "line 3",
            ),
            (
                format!("{head}\ntx\t1\t2026-09-25\t-100\tfood\t9\tn\tLunch\t\n"),
                "account 9",
            ),
            (
                format!("{head}\n{account}\ntx\t1\t2026-09-25\t-100\tfood\t1\tmaybe\tLunch\t\n"),
                "line 3",
            ),
            (
                format!("{head}\n{account}\ntx\t1\t2026-09-25\t-1.00\tfood\t1\tn\tLunch\t\n"),
                "line 3",
            ),
            (
                format!("{head}\n{account}\ntx\t1\t2026-09-25\t-100\tfood\t1\tn\tLunch\n"),
                "line 3",
            ),
        ];
        for (text, says) in cases {
            let err = parse_ledger(&text)
                .err()
                .unwrap_or_else(|| panic!("{text:?} was read"));
            assert!(err.contains(says), "{text:?}: {err}");
        }
        let fine = parse_ledger(&format!("{head}\n\n# a note\n{account}\n")).unwrap();
        assert_eq!(
            fine.accounts.len(),
            1,
            "a blank line or a comment was refused"
        );
    }
}
