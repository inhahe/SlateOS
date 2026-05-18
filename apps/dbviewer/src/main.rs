//! OurOS SQLite Database Viewer / Browser
//!
//! A database viewer/browser tool (like DB Browser for SQLite) with:
//! - SQL parser: basic SELECT, INSERT, UPDATE, DELETE, CREATE TABLE, DROP TABLE
//! - Data types: INTEGER, REAL, TEXT, BLOB, NULL
//! - In-memory table storage (simulated SQLite engine)
//! - Table schema viewer with column names, types, constraints
//! - Paginated data browser with column sorting
//! - SQL query editor with syntax highlighting hints
//! - Query history with favorites
//! - Multiple database connections (tabs)
//! - Object tree sidebar (tables, indexes, views, triggers)
//! - Export: CSV, JSON, SQL INSERT statements
//! - Import: CSV with header detection
//! - Row editing: insert/update/delete individual rows
//! - WHERE clause builder (column, operator, value)
//! - Aggregate functions: COUNT, SUM, AVG, MIN, MAX
//! - Schema diagram: FK relationship visualization
//! - Multi-panel UI: sidebar, data grid, SQL editor, results
//!
//! Uses the guitk library for UI rendering.

#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::similar_names)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::wildcard_imports)]

use guitk::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
// ============================================================================
// Layout constants
// ============================================================================

const TOOLBAR_HEIGHT: f32 = 36.0;
const STATUS_BAR_HEIGHT: f32 = 22.0;
const SIDEBAR_WIDTH: f32 = 220.0;
const TAB_HEIGHT: f32 = 30.0;
const ROW_HEIGHT: f32 = 26.0;
const HEADER_HEIGHT: f32 = 28.0;
const EDITOR_HEIGHT: f32 = 140.0;
const CORNER_RADIUS: f32 = 4.0;
const CELL_PADDING: f32 = 8.0;
const PAGE_SIZE: usize = 50;
const DEFAULT_COL_WIDTH: f32 = 140.0;

// ============================================================================
// SQL keywords for syntax highlighting
// ============================================================================

const SQL_KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "INSERT", "INTO", "VALUES", "UPDATE", "SET",
    "DELETE", "CREATE", "TABLE", "DROP", "ALTER", "ADD", "COLUMN", "INDEX",
    "VIEW", "TRIGGER", "PRIMARY", "KEY", "NOT", "NULL", "UNIQUE", "DEFAULT",
    "CHECK", "FOREIGN", "REFERENCES", "ON", "CASCADE", "RESTRICT", "AND",
    "OR", "IN", "BETWEEN", "LIKE", "IS", "AS", "ORDER", "BY", "ASC", "DESC",
    "LIMIT", "OFFSET", "GROUP", "HAVING", "JOIN", "LEFT", "RIGHT", "INNER",
    "OUTER", "CROSS", "DISTINCT", "COUNT", "SUM", "AVG", "MIN", "MAX",
    "INTEGER", "REAL", "TEXT", "BLOB", "IF", "EXISTS",
];

// ============================================================================
// Data types
// ============================================================================

/// SQLite-compatible data types.
#[derive(Clone, Debug, PartialEq)]
pub enum DataType {
    Integer,
    Real,
    Text,
    Blob,
    Null,
}

impl DataType {
    fn label(&self) -> &'static str {
        match self {
            Self::Integer => "INTEGER",
            Self::Real => "REAL",
            Self::Text => "TEXT",
            Self::Blob => "BLOB",
            Self::Null => "NULL",
        }
    }

    fn from_str_loose(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "INTEGER" | "INT" | "BIGINT" | "SMALLINT" | "TINYINT" => Self::Integer,
            "REAL" | "FLOAT" | "DOUBLE" | "NUMERIC" | "DECIMAL" => Self::Real,
            "TEXT" | "VARCHAR" | "CHAR" | "STRING" | "CLOB" => Self::Text,
            "BLOB" | "BINARY" | "VARBINARY" => Self::Blob,
            "NULL" => Self::Null,
            _ => Self::Text,
        }
    }

    fn color(&self) -> Color {
        match self {
            Self::Integer => BLUE,
            Self::Real => PEACH,
            Self::Text => GREEN,
            Self::Blob => MAUVE,
            Self::Null => OVERLAY0,
        }
    }
}

// ============================================================================
// Cell value
// ============================================================================

/// A single cell value in the database.
#[derive(Clone, Debug, PartialEq)]
pub enum CellValue {
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
    Null,
}

impl CellValue {
    fn display(&self) -> String {
        match self {
            Self::Integer(v) => v.to_string(),
            Self::Real(v) => format!("{v:.6}"),
            Self::Text(s) => s.clone(),
            Self::Blob(b) => format!("<BLOB {} bytes>", b.len()),
            Self::Null => "NULL".to_owned(),
        }
    }

    fn as_sort_key(&self) -> SortKey<'_> {
        match self {
            Self::Null => SortKey::Null,
            Self::Integer(v) => SortKey::Int(*v),
            Self::Real(v) => SortKey::Float(*v),
            Self::Text(s) => SortKey::Str(s.as_str()),
            Self::Blob(b) => SortKey::Bytes(b.as_slice()),
        }
    }

    /// Parse a string into a `CellValue` given a target data type.
    fn parse_as(s: &str, dtype: &DataType) -> Self {
        if s.eq_ignore_ascii_case("null") || s.is_empty() {
            return Self::Null;
        }
        match dtype {
            DataType::Integer => s.parse::<i64>().map_or(Self::Text(s.to_owned()), Self::Integer),
            DataType::Real => s.parse::<f64>().map_or(Self::Text(s.to_owned()), Self::Real),
            DataType::Text => Self::Text(s.to_owned()),
            DataType::Blob => Self::Blob(s.as_bytes().to_vec()),
            DataType::Null => Self::Null,
        }
    }
}

/// Sort key for ordering cell values.
#[derive(Debug)]
enum SortKey<'a> {
    Null,
    Int(i64),
    Float(f64),
    Str(&'a str),
    Bytes(&'a [u8]),
}

impl PartialEq for SortKey<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp_value(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for SortKey<'_> {}

impl PartialOrd for SortKey<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp_value(other))
    }
}

impl Ord for SortKey<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.cmp_value(other)
    }
}

impl SortKey<'_> {
    fn cmp_value(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self, other) {
            (Self::Null, Self::Null) => Ordering::Equal,
            (Self::Null, _) => Ordering::Less,
            (_, Self::Null) => Ordering::Greater,
            (Self::Int(a), Self::Int(b)) => a.cmp(b),
            (Self::Float(a), Self::Float(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
            (Self::Str(a), Self::Str(b)) => a.cmp(b),
            (Self::Bytes(a), Self::Bytes(b)) => a.cmp(b),
            (Self::Int(a), Self::Float(b)) => (*a as f64).partial_cmp(b).unwrap_or(Ordering::Equal),
            (Self::Float(a), Self::Int(b)) => a.partial_cmp(&(*b as f64)).unwrap_or(Ordering::Equal),
            _ => Ordering::Equal,
        }
    }
}

// ============================================================================
// Column constraints
// ============================================================================

/// Column constraint flags.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColumnConstraints {
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default_value: Option<String>,
    pub auto_increment: bool,
}

impl ColumnConstraints {
    fn describe(&self) -> String {
        let mut parts = Vec::new();
        if self.primary_key {
            parts.push("PK");
        }
        if self.auto_increment {
            parts.push("AI");
        }
        if self.not_null {
            parts.push("NN");
        }
        if self.unique {
            parts.push("UQ");
        }
        if let Some(ref def) = self.default_value {
            parts.push("DEF=");
            // We just push separate parts; the caller joins them
            return format!("{} DEF={def}", parts[..parts.len().saturating_sub(1)].join(" ")).trim().to_owned();
        }
        parts.join(" ")
    }
}

// ============================================================================
// Column definition
// ============================================================================

/// A column in a table schema.
#[derive(Clone, Debug)]
pub struct ColumnDef {
    pub name: String,
    pub data_type: DataType,
    pub constraints: ColumnConstraints,
}

impl ColumnDef {
    fn new(name: &str, data_type: DataType) -> Self {
        Self {
            name: name.to_owned(),
            data_type,
            constraints: ColumnConstraints::default(),
        }
    }

    fn with_primary_key(mut self) -> Self {
        self.constraints.primary_key = true;
        self.constraints.not_null = true;
        self
    }

    fn with_not_null(mut self) -> Self {
        self.constraints.not_null = true;
        self
    }

    fn with_unique(mut self) -> Self {
        self.constraints.unique = true;
        self
    }

    fn with_default(mut self, default: &str) -> Self {
        self.constraints.default_value = Some(default.to_owned());
        self
    }

    fn with_auto_increment(mut self) -> Self {
        self.constraints.auto_increment = true;
        self
    }
}

// ============================================================================
// Foreign key reference
// ============================================================================

/// A foreign key constraint between tables.
#[derive(Clone, Debug, PartialEq)]
pub struct ForeignKey {
    pub from_table: String,
    pub from_column: String,
    pub to_table: String,
    pub to_column: String,
}

// ============================================================================
// Index definition
// ============================================================================

/// An index on a table.
#[derive(Clone, Debug)]
pub struct IndexDef {
    pub name: String,
    pub table_name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

// ============================================================================
// View definition
// ============================================================================

/// A view (stored query).
#[derive(Clone, Debug)]
pub struct ViewDef {
    pub name: String,
    pub sql: String,
}

// ============================================================================
// Trigger definition
// ============================================================================

/// A trigger on a table.
#[derive(Clone, Debug)]
pub struct TriggerDef {
    pub name: String,
    pub table_name: String,
    pub event: String,
    pub sql: String,
}

// ============================================================================
// Table
// ============================================================================

/// An in-memory database table.
#[derive(Clone, Debug)]
pub struct Table {
    pub name: String,
    pub columns: Vec<ColumnDef>,
    pub rows: Vec<Vec<CellValue>>,
    pub auto_increment_counter: i64,
}

impl Table {
    fn new(name: &str, columns: Vec<ColumnDef>) -> Self {
        Self {
            name: name.to_owned(),
            columns,
            rows: Vec::new(),
            auto_increment_counter: 0,
        }
    }

    /// Insert a row, filling in auto-increment values and defaults.
    fn insert_row(&mut self, values: Vec<CellValue>) -> Result<(), String> {
        if values.len() != self.columns.len() {
            return Err(format!(
                "Column count mismatch: expected {}, got {}",
                self.columns.len(),
                values.len()
            ));
        }

        // Validate NOT NULL constraints
        for (i, val) in values.iter().enumerate() {
            if let Some(col) = self.columns.get(i) {
                if col.constraints.not_null && *val == CellValue::Null && !col.constraints.auto_increment {
                    return Err(format!("Column '{}' cannot be NULL", col.name));
                }
            }
        }

        // Handle auto-increment
        let mut final_values = values;
        for (i, col) in self.columns.iter().enumerate() {
            if col.constraints.auto_increment {
                if let Some(v) = final_values.get(i) {
                    if *v == CellValue::Null {
                        self.auto_increment_counter = self.auto_increment_counter.saturating_add(1);
                        if let Some(cell) = final_values.get_mut(i) {
                            *cell = CellValue::Integer(self.auto_increment_counter);
                        }
                    }
                }
            }
        }

        // Check UNIQUE constraints
        for (i, col) in self.columns.iter().enumerate() {
            if col.constraints.unique || col.constraints.primary_key {
                if let Some(new_val) = final_values.get(i) {
                    for existing_row in &self.rows {
                        if let Some(existing_val) = existing_row.get(i) {
                            if *existing_val != CellValue::Null && *existing_val == *new_val {
                                return Err(format!(
                                    "UNIQUE constraint failed: column '{}'",
                                    col.name
                                ));
                            }
                        }
                    }
                }
            }
        }

        self.rows.push(final_values);
        Ok(())
    }

    /// Find column index by name (case-insensitive).
    fn column_index(&self, name: &str) -> Option<usize> {
        let name_upper = name.to_uppercase();
        self.columns.iter().position(|c| c.name.to_uppercase() == name_upper)
    }

    /// Delete rows matching a predicate on a specific column.
    fn delete_where(&mut self, col_idx: usize, op: &FilterOp, value: &CellValue) -> usize {
        let before = self.rows.len();
        self.rows.retain(|row| {
            row.get(col_idx)
                .map_or(true, |cell| !matches_filter(cell, op, value))
        });
        before.saturating_sub(self.rows.len())
    }

    /// Update rows matching a predicate.
    fn update_where(
        &mut self,
        set_col: usize,
        set_value: &CellValue,
        where_col: usize,
        op: &FilterOp,
        where_value: &CellValue,
    ) -> usize {
        let mut count = 0usize;
        for row in &mut self.rows {
            let matches = row
                .get(where_col)
                .map_or(false, |cell| matches_filter(cell, op, where_value));
            if matches {
                if let Some(cell) = row.get_mut(set_col) {
                    *cell = set_value.clone();
                    count = count.saturating_add(1);
                }
            }
        }
        count
    }

    /// Get column count.
    fn col_count(&self) -> usize {
        self.columns.len()
    }

    /// Get row count.
    fn row_count(&self) -> usize {
        self.rows.len()
    }
}

// ============================================================================
// Database
// ============================================================================

/// An in-memory database containing tables, indexes, views, and triggers.
#[derive(Clone, Debug)]
pub struct Database {
    pub name: String,
    pub tables: Vec<Table>,
    pub indexes: Vec<IndexDef>,
    pub views: Vec<ViewDef>,
    pub triggers: Vec<TriggerDef>,
    pub foreign_keys: Vec<ForeignKey>,
}

impl Database {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            tables: Vec::new(),
            indexes: Vec::new(),
            views: Vec::new(),
            triggers: Vec::new(),
            foreign_keys: Vec::new(),
        }
    }

    fn find_table(&self, name: &str) -> Option<&Table> {
        let name_upper = name.to_uppercase();
        self.tables.iter().find(|t| t.name.to_uppercase() == name_upper)
    }

    fn find_table_mut(&mut self, name: &str) -> Option<&mut Table> {
        let name_upper = name.to_uppercase();
        self.tables.iter_mut().find(|t| t.name.to_uppercase() == name_upper)
    }

    fn create_table(&mut self, table: Table) -> Result<(), String> {
        if self.find_table(&table.name).is_some() {
            return Err(format!("Table '{}' already exists", table.name));
        }
        self.tables.push(table);
        Ok(())
    }

    fn drop_table(&mut self, name: &str) -> Result<(), String> {
        let name_upper = name.to_uppercase();
        let idx = self
            .tables
            .iter()
            .position(|t| t.name.to_uppercase() == name_upper)
            .ok_or_else(|| format!("Table '{name}' not found"))?;
        self.tables.remove(idx);
        // Also remove related indexes, triggers, foreign keys
        self.indexes.retain(|i| i.table_name.to_uppercase() != name_upper);
        self.triggers.retain(|t| t.table_name.to_uppercase() != name_upper);
        self.foreign_keys.retain(|fk| {
            fk.from_table.to_uppercase() != name_upper
                && fk.to_table.to_uppercase() != name_upper
        });
        Ok(())
    }

    fn table_names(&self) -> Vec<String> {
        self.tables.iter().map(|t| t.name.clone()).collect()
    }

    /// Create a sample database for demonstration.
    fn sample() -> Self {
        let mut db = Self::new("sample.db");

        // Users table
        let users = Table::new(
            "users",
            vec![
                ColumnDef::new("id", DataType::Integer)
                    .with_primary_key()
                    .with_auto_increment(),
                ColumnDef::new("name", DataType::Text).with_not_null(),
                ColumnDef::new("email", DataType::Text)
                    .with_not_null()
                    .with_unique(),
                ColumnDef::new("age", DataType::Integer),
                ColumnDef::new("score", DataType::Real).with_default("0.0"),
            ],
        );
        let _ = db.create_table(users);

        // Insert sample users
        if let Some(table) = db.find_table_mut("users") {
            let sample_users: &[(&str, &str, i64, f64)] = &[
                ("Alice", "alice@example.com", 30, 95.5),
                ("Bob", "bob@example.com", 25, 82.3),
                ("Charlie", "charlie@example.com", 35, 91.0),
                ("Diana", "diana@example.com", 28, 88.7),
                ("Eve", "eve@example.com", 32, 76.2),
                ("Frank", "frank@example.com", 45, 99.1),
                ("Grace", "grace@example.com", 22, 67.8),
                ("Hank", "hank@example.com", 38, 84.5),
                ("Ivy", "ivy@example.com", 29, 92.3),
                ("Jack", "jack@example.com", 41, 71.6),
            ];
            for (name, email, age, score) in sample_users {
                let _ = table.insert_row(vec![
                    CellValue::Null, // auto-increment id
                    CellValue::Text((*name).to_owned()),
                    CellValue::Text((*email).to_owned()),
                    CellValue::Integer(*age),
                    CellValue::Real(*score),
                ]);
            }
        }

        // Products table
        let products = Table::new(
            "products",
            vec![
                ColumnDef::new("id", DataType::Integer)
                    .with_primary_key()
                    .with_auto_increment(),
                ColumnDef::new("name", DataType::Text).with_not_null(),
                ColumnDef::new("price", DataType::Real).with_not_null(),
                ColumnDef::new("category", DataType::Text),
                ColumnDef::new("stock", DataType::Integer).with_default("0"),
            ],
        );
        let _ = db.create_table(products);

        if let Some(table) = db.find_table_mut("products") {
            let sample_products: &[(&str, f64, &str, i64)] = &[
                ("Laptop", 999.99, "Electronics", 50),
                ("Keyboard", 79.99, "Electronics", 200),
                ("Mouse", 29.99, "Electronics", 300),
                ("Desk", 249.99, "Furniture", 30),
                ("Chair", 199.99, "Furniture", 45),
                ("Monitor", 399.99, "Electronics", 80),
                ("Headset", 59.99, "Electronics", 150),
                ("Webcam", 49.99, "Electronics", 100),
            ];
            for (name, price, category, stock) in sample_products {
                let _ = table.insert_row(vec![
                    CellValue::Null,
                    CellValue::Text((*name).to_owned()),
                    CellValue::Real(*price),
                    CellValue::Text((*category).to_owned()),
                    CellValue::Integer(*stock),
                ]);
            }
        }

        // Orders table with FK
        let orders = Table::new(
            "orders",
            vec![
                ColumnDef::new("id", DataType::Integer)
                    .with_primary_key()
                    .with_auto_increment(),
                ColumnDef::new("user_id", DataType::Integer).with_not_null(),
                ColumnDef::new("product_id", DataType::Integer).with_not_null(),
                ColumnDef::new("quantity", DataType::Integer).with_default("1"),
                ColumnDef::new("total", DataType::Real),
            ],
        );
        let _ = db.create_table(orders);

        if let Some(table) = db.find_table_mut("orders") {
            let sample_orders: &[(i64, i64, i64, f64)] = &[
                (1, 1, 1, 999.99),
                (1, 3, 2, 59.98),
                (2, 2, 1, 79.99),
                (3, 6, 1, 399.99),
                (4, 5, 2, 399.98),
                (5, 4, 1, 249.99),
            ];
            for (uid, pid, qty, total) in sample_orders {
                let _ = table.insert_row(vec![
                    CellValue::Null,
                    CellValue::Integer(*uid),
                    CellValue::Integer(*pid),
                    CellValue::Integer(*qty),
                    CellValue::Real(*total),
                ]);
            }
        }

        // Foreign keys
        db.foreign_keys.push(ForeignKey {
            from_table: "orders".to_owned(),
            from_column: "user_id".to_owned(),
            to_table: "users".to_owned(),
            to_column: "id".to_owned(),
        });
        db.foreign_keys.push(ForeignKey {
            from_table: "orders".to_owned(),
            from_column: "product_id".to_owned(),
            to_table: "products".to_owned(),
            to_column: "id".to_owned(),
        });

        // Indexes
        db.indexes.push(IndexDef {
            name: "idx_users_email".to_owned(),
            table_name: "users".to_owned(),
            columns: vec!["email".to_owned()],
            unique: true,
        });
        db.indexes.push(IndexDef {
            name: "idx_orders_user".to_owned(),
            table_name: "orders".to_owned(),
            columns: vec!["user_id".to_owned()],
            unique: false,
        });

        // Views
        db.views.push(ViewDef {
            name: "user_orders_view".to_owned(),
            sql: "SELECT u.name, p.name AS product, o.quantity, o.total FROM orders o JOIN users u ON o.user_id = u.id JOIN products p ON o.product_id = p.id".to_owned(),
        });

        // Triggers
        db.triggers.push(TriggerDef {
            name: "update_stock".to_owned(),
            table_name: "orders".to_owned(),
            event: "AFTER INSERT".to_owned(),
            sql: "UPDATE products SET stock = stock - NEW.quantity WHERE id = NEW.product_id"
                .to_owned(),
        });

        db
    }
}

// ============================================================================
// Filter operations
// ============================================================================

/// Comparison operators for WHERE clauses.
#[derive(Clone, Debug, PartialEq)]
pub enum FilterOp {
    Equal,
    NotEqual,
    LessThan,
    GreaterThan,
    LessOrEqual,
    GreaterOrEqual,
    Like,
    IsNull,
    IsNotNull,
}

impl FilterOp {
    fn label(&self) -> &'static str {
        match self {
            Self::Equal => "=",
            Self::NotEqual => "!=",
            Self::LessThan => "<",
            Self::GreaterThan => ">",
            Self::LessOrEqual => "<=",
            Self::GreaterOrEqual => ">=",
            Self::Like => "LIKE",
            Self::IsNull => "IS NULL",
            Self::IsNotNull => "IS NOT NULL",
        }
    }

    fn all() -> &'static [Self] {
        &[
            Self::Equal,
            Self::NotEqual,
            Self::LessThan,
            Self::GreaterThan,
            Self::LessOrEqual,
            Self::GreaterOrEqual,
            Self::Like,
            Self::IsNull,
            Self::IsNotNull,
        ]
    }
}

/// Check if a cell value matches a filter condition.
fn matches_filter(cell: &CellValue, op: &FilterOp, value: &CellValue) -> bool {
    match op {
        FilterOp::IsNull => *cell == CellValue::Null,
        FilterOp::IsNotNull => *cell != CellValue::Null,
        FilterOp::Equal => cell.as_sort_key() == value.as_sort_key(),
        FilterOp::NotEqual => cell.as_sort_key() != value.as_sort_key(),
        FilterOp::LessThan => cell.as_sort_key() < value.as_sort_key(),
        FilterOp::GreaterThan => cell.as_sort_key() > value.as_sort_key(),
        FilterOp::LessOrEqual => cell.as_sort_key() <= value.as_sort_key(),
        FilterOp::GreaterOrEqual => cell.as_sort_key() >= value.as_sort_key(),
        FilterOp::Like => {
            if let (CellValue::Text(cell_str), CellValue::Text(pattern)) = (cell, value) {
                simple_like_match(cell_str, pattern)
            } else {
                false
            }
        }
    }
}

/// Simple LIKE pattern matcher supporting % and _ wildcards.
fn simple_like_match(text: &str, pattern: &str) -> bool {
    let text = text.to_lowercase();
    let pattern = pattern.to_lowercase();
    like_match_inner(text.as_bytes(), pattern.as_bytes())
}

fn like_match_inner(text: &[u8], pattern: &[u8]) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }
    if let Some(&first_p) = pattern.first() {
        if first_p == b'%' {
            // % matches any sequence
            let rest_pattern = pattern.get(1..).unwrap_or_default();
            for i in 0..=text.len() {
                if like_match_inner(text.get(i..).unwrap_or_default(), rest_pattern) {
                    return true;
                }
            }
            false
        } else if first_p == b'_' {
            // _ matches exactly one character
            if text.is_empty() {
                return false;
            }
            like_match_inner(
                text.get(1..).unwrap_or_default(),
                pattern.get(1..).unwrap_or_default(),
            )
        } else {
            // Literal match
            if text.is_empty() || text.first() != pattern.first() {
                return false;
            }
            like_match_inner(
                text.get(1..).unwrap_or_default(),
                pattern.get(1..).unwrap_or_default(),
            )
        }
    } else {
        text.is_empty()
    }
}

// ============================================================================
// Active filter
// ============================================================================

/// An active filter rule for the WHERE clause builder.
#[derive(Clone, Debug)]
pub struct ActiveFilter {
    pub column_idx: usize,
    pub op: FilterOp,
    pub value_str: String,
}

// ============================================================================
// Sort state
// ============================================================================

/// Sorting direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SortDir {
    Ascending,
    Descending,
}

/// Current sort state for a table view.
#[derive(Clone, Debug)]
pub struct SortState {
    pub column_idx: usize,
    pub direction: SortDir,
}

// ============================================================================
// Query result
// ============================================================================

/// Result of executing a SQL query.
#[derive(Clone, Debug)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<CellValue>>,
    pub message: String,
    pub affected_rows: usize,
    pub is_error: bool,
}

impl QueryResult {
    fn success(msg: &str) -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            message: msg.to_owned(),
            affected_rows: 0,
            is_error: false,
        }
    }

    fn error(msg: &str) -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            message: msg.to_owned(),
            affected_rows: 0,
            is_error: true,
        }
    }

    fn with_data(columns: Vec<String>, rows: Vec<Vec<CellValue>>) -> Self {
        let row_count = rows.len();
        Self {
            columns,
            rows,
            message: format!("{row_count} row(s) returned"),
            affected_rows: row_count,
            is_error: false,
        }
    }
}

// ============================================================================
// Query history entry
// ============================================================================

/// An entry in the query history.
#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub sql: String,
    pub success: bool,
    pub message: String,
    pub favorite: bool,
    pub timestamp_counter: u64,
}

// ============================================================================
// SQL parser — tokenizer
// ============================================================================

/// A SQL token.
#[derive(Clone, Debug, PartialEq)]
pub enum SqlToken {
    Keyword(String),
    Identifier(String),
    StringLiteral(String),
    NumberLiteral(String),
    Operator(String),
    Comma,
    Semicolon,
    LeftParen,
    RightParen,
    Star,
    Dot,
    Whitespace,
}

/// Tokenize a SQL string into tokens.
fn tokenize_sql(input: &str) -> Vec<SqlToken> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let ch = chars.get(i).copied().unwrap_or(' ');

        // Skip whitespace
        if ch.is_whitespace() {
            tokens.push(SqlToken::Whitespace);
            while i < len && chars.get(i).map_or(false, |c| c.is_whitespace()) {
                i = i.saturating_add(1);
            }
            continue;
        }

        // String literals
        if ch == '\'' {
            let mut s = String::new();
            i = i.saturating_add(1);
            while i < len {
                let c = chars.get(i).copied().unwrap_or(' ');
                if c == '\'' {
                    // Check for escaped quote
                    if i.saturating_add(1) < len && chars.get(i.saturating_add(1)) == Some(&'\'') {
                        s.push('\'');
                        i = i.saturating_add(2);
                    } else {
                        i = i.saturating_add(1);
                        break;
                    }
                } else {
                    s.push(c);
                    i = i.saturating_add(1);
                }
            }
            tokens.push(SqlToken::StringLiteral(s));
            continue;
        }

        // Numbers
        if ch.is_ascii_digit() || (ch == '.' && i.saturating_add(1) < len && chars.get(i.saturating_add(1)).map_or(false, |c| c.is_ascii_digit())) {
            let mut num = String::new();
            while i < len && chars.get(i).map_or(false, |c| c.is_ascii_digit() || *c == '.') {
                num.push(chars.get(i).copied().unwrap_or('0'));
                i = i.saturating_add(1);
            }
            tokens.push(SqlToken::NumberLiteral(num));
            continue;
        }

        // Identifiers / keywords
        if ch.is_ascii_alphabetic() || ch == '_' {
            let mut ident = String::new();
            while i < len && chars.get(i).map_or(false, |c| c.is_ascii_alphanumeric() || *c == '_') {
                ident.push(chars.get(i).copied().unwrap_or('_'));
                i = i.saturating_add(1);
            }
            let upper = ident.to_uppercase();
            if SQL_KEYWORDS.contains(&upper.as_str()) {
                tokens.push(SqlToken::Keyword(upper));
            } else {
                tokens.push(SqlToken::Identifier(ident));
            }
            continue;
        }

        // Operators
        match ch {
            '=' => { tokens.push(SqlToken::Operator("=".to_owned())); i = i.saturating_add(1); }
            '!' if i.saturating_add(1) < len && chars.get(i.saturating_add(1)) == Some(&'=') => {
                tokens.push(SqlToken::Operator("!=".to_owned()));
                i = i.saturating_add(2);
            }
            '<' if i.saturating_add(1) < len && chars.get(i.saturating_add(1)) == Some(&'=') => {
                tokens.push(SqlToken::Operator("<=".to_owned()));
                i = i.saturating_add(2);
            }
            '<' if i.saturating_add(1) < len && chars.get(i.saturating_add(1)) == Some(&'>') => {
                tokens.push(SqlToken::Operator("<>".to_owned()));
                i = i.saturating_add(2);
            }
            '<' => { tokens.push(SqlToken::Operator("<".to_owned())); i = i.saturating_add(1); }
            '>' if i.saturating_add(1) < len && chars.get(i.saturating_add(1)) == Some(&'=') => {
                tokens.push(SqlToken::Operator(">=".to_owned()));
                i = i.saturating_add(2);
            }
            '>' => { tokens.push(SqlToken::Operator(">".to_owned())); i = i.saturating_add(1); }
            '(' => { tokens.push(SqlToken::LeftParen); i = i.saturating_add(1); }
            ')' => { tokens.push(SqlToken::RightParen); i = i.saturating_add(1); }
            ',' => { tokens.push(SqlToken::Comma); i = i.saturating_add(1); }
            ';' => { tokens.push(SqlToken::Semicolon); i = i.saturating_add(1); }
            '*' => { tokens.push(SqlToken::Star); i = i.saturating_add(1); }
            '.' => { tokens.push(SqlToken::Dot); i = i.saturating_add(1); }
            _ => { i = i.saturating_add(1); } // Skip unknown chars
        }
    }

    tokens
}

// ============================================================================
// SQL parser — statement types
// ============================================================================

/// Parsed SQL statement.
#[derive(Clone, Debug)]
pub enum SqlStatement {
    Select {
        columns: Vec<SelectColumn>,
        table: String,
        where_clause: Option<WhereClause>,
        order_by: Option<(String, SortDir)>,
        limit: Option<usize>,
        offset: Option<usize>,
        group_by: Option<String>,
    },
    Insert {
        table: String,
        columns: Vec<String>,
        values: Vec<Vec<String>>,
    },
    Update {
        table: String,
        set_clauses: Vec<(String, String)>,
        where_clause: Option<WhereClause>,
    },
    Delete {
        table: String,
        where_clause: Option<WhereClause>,
    },
    CreateTable {
        name: String,
        columns: Vec<ParsedColumnDef>,
        if_not_exists: bool,
    },
    DropTable {
        name: String,
        if_exists: bool,
    },
}

/// A column in a SELECT statement.
#[derive(Clone, Debug)]
pub enum SelectColumn {
    AllColumns,
    Named(String),
    Aggregate { func: AggFunc, column: String, alias: Option<String> },
}

/// Aggregate functions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AggFunc {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

impl AggFunc {
    fn label(&self) -> &'static str {
        match self {
            Self::Count => "COUNT",
            Self::Sum => "SUM",
            Self::Avg => "AVG",
            Self::Min => "MIN",
            Self::Max => "MAX",
        }
    }

    fn from_keyword(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "COUNT" => Some(Self::Count),
            "SUM" => Some(Self::Sum),
            "AVG" => Some(Self::Avg),
            "MIN" => Some(Self::Min),
            "MAX" => Some(Self::Max),
            _ => None,
        }
    }
}

/// A WHERE clause condition.
#[derive(Clone, Debug)]
pub struct WhereClause {
    pub column: String,
    pub op: FilterOp,
    pub value: String,
}

/// A column definition from a CREATE TABLE statement.
#[derive(Clone, Debug)]
pub struct ParsedColumnDef {
    pub name: String,
    pub data_type: String,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub auto_increment: bool,
    pub default_value: Option<String>,
}

// ============================================================================
// SQL parser — parse functions
// ============================================================================

/// Parse a SQL string into a statement.
fn parse_sql(input: &str) -> Result<SqlStatement, String> {
    let tokens: Vec<SqlToken> = tokenize_sql(input)
        .into_iter()
        .filter(|t| *t != SqlToken::Whitespace)
        .collect();

    if tokens.is_empty() {
        return Err("Empty query".to_owned());
    }

    let first = tokens.first().ok_or_else(|| "Empty query".to_owned())?;
    match first {
        SqlToken::Keyword(k) => match k.as_str() {
            "SELECT" => parse_select(&tokens),
            "INSERT" => parse_insert(&tokens),
            "UPDATE" => parse_update(&tokens),
            "DELETE" => parse_delete(&tokens),
            "CREATE" => parse_create_table(&tokens),
            "DROP" => parse_drop_table(&tokens),
            _ => Err(format!("Unsupported statement: {k}")),
        },
        _ => Err("Expected SQL keyword at start".to_owned()),
    }
}

fn parse_select(tokens: &[SqlToken]) -> Result<SqlStatement, String> {
    let mut pos = 1; // Skip SELECT

    // Parse column list
    let mut columns = Vec::new();
    loop {
        if pos >= tokens.len() {
            return Err("Expected column list".to_owned());
        }
        let tok = tokens.get(pos).ok_or("Unexpected end of input")?;
        match tok {
            SqlToken::Star => {
                columns.push(SelectColumn::AllColumns);
                pos = pos.saturating_add(1);
            }
            SqlToken::Keyword(k) if AggFunc::from_keyword(k).is_some() => {
                let func = AggFunc::from_keyword(k).ok_or("Invalid aggregate")?;
                pos = pos.saturating_add(1);
                // Expect (column)
                expect_token(tokens, pos, &SqlToken::LeftParen)?;
                pos = pos.saturating_add(1);
                let col_name = match tokens.get(pos) {
                    Some(SqlToken::Identifier(name)) => name.clone(),
                    Some(SqlToken::Star) => "*".to_owned(),
                    _ => return Err("Expected column name in aggregate".to_owned()),
                };
                pos = pos.saturating_add(1);
                expect_token(tokens, pos, &SqlToken::RightParen)?;
                pos = pos.saturating_add(1);
                // Optional alias
                let alias = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "AS") {
                    pos = pos.saturating_add(1);
                    let a = extract_identifier(tokens, pos)?;
                    pos = pos.saturating_add(1);
                    Some(a)
                } else {
                    None
                };
                columns.push(SelectColumn::Aggregate { func, column: col_name, alias });
            }
            SqlToken::Identifier(name) => {
                columns.push(SelectColumn::Named(name.clone()));
                pos = pos.saturating_add(1);
            }
            _ => return Err("Expected column name or *".to_owned()),
        }
        if matches!(tokens.get(pos), Some(SqlToken::Comma)) {
            pos = pos.saturating_add(1);
        } else {
            break;
        }
    }

    // FROM clause
    if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "FROM") {
        return Err("Expected FROM".to_owned());
    }
    pos = pos.saturating_add(1);
    let table = extract_identifier(tokens, pos)?;
    pos = pos.saturating_add(1);

    // Optional WHERE
    let where_clause = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "WHERE") {
        pos = pos.saturating_add(1);
        let (wc, new_pos) = parse_where_clause(tokens, pos)?;
        pos = new_pos;
        Some(wc)
    } else {
        None
    };

    // Optional GROUP BY
    let group_by = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "GROUP") {
        pos = pos.saturating_add(1);
        if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "BY") {
            return Err("Expected BY after GROUP".to_owned());
        }
        pos = pos.saturating_add(1);
        let col = extract_identifier(tokens, pos)?;
        pos = pos.saturating_add(1);
        Some(col)
    } else {
        None
    };

    // Optional ORDER BY
    let order_by = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "ORDER") {
        pos = pos.saturating_add(1);
        if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "BY") {
            return Err("Expected BY after ORDER".to_owned());
        }
        pos = pos.saturating_add(1);
        let col = extract_identifier(tokens, pos)?;
        pos = pos.saturating_add(1);
        let dir = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "DESC") {
            pos = pos.saturating_add(1);
            SortDir::Descending
        } else {
            if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "ASC") {
                pos = pos.saturating_add(1);
            }
            SortDir::Ascending
        };
        Some((col, dir))
    } else {
        None
    };

    // Optional LIMIT
    let limit = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "LIMIT") {
        pos = pos.saturating_add(1);
        let n = extract_number(tokens, pos)?;
        pos = pos.saturating_add(1);
        Some(n)
    } else {
        None
    };

    // Optional OFFSET
    let offset = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "OFFSET") {
        pos = pos.saturating_add(1);
        let n = extract_number(tokens, pos)?;
        pos = pos.saturating_add(1);
        Some(n)
    } else {
        None
    };

    let _ = pos; // suppress "pos unused" - we've parsed everything we need

    Ok(SqlStatement::Select {
        columns,
        table,
        where_clause,
        order_by,
        limit,
        offset,
        group_by,
    })
}

fn parse_insert(tokens: &[SqlToken]) -> Result<SqlStatement, String> {
    let mut pos = 1; // Skip INSERT

    if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "INTO") {
        return Err("Expected INTO after INSERT".to_owned());
    }
    pos = pos.saturating_add(1);

    let table = extract_identifier(tokens, pos)?;
    pos = pos.saturating_add(1);

    // Optional column list
    let mut col_names = Vec::new();
    if matches!(tokens.get(pos), Some(SqlToken::LeftParen)) {
        pos = pos.saturating_add(1);
        loop {
            let name = extract_identifier(tokens, pos)?;
            col_names.push(name);
            pos = pos.saturating_add(1);
            if matches!(tokens.get(pos), Some(SqlToken::Comma)) {
                pos = pos.saturating_add(1);
            } else {
                break;
            }
        }
        expect_token(tokens, pos, &SqlToken::RightParen)?;
        pos = pos.saturating_add(1);
    }

    if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "VALUES") {
        return Err("Expected VALUES".to_owned());
    }
    pos = pos.saturating_add(1);

    // Parse value lists
    let mut all_values = Vec::new();
    loop {
        expect_token(tokens, pos, &SqlToken::LeftParen)?;
        pos = pos.saturating_add(1);
        let mut row_values = Vec::new();
        loop {
            let val = extract_value_str(tokens, pos)?;
            row_values.push(val);
            pos = pos.saturating_add(1);
            if matches!(tokens.get(pos), Some(SqlToken::Comma)) {
                pos = pos.saturating_add(1);
            } else {
                break;
            }
        }
        expect_token(tokens, pos, &SqlToken::RightParen)?;
        pos = pos.saturating_add(1);
        all_values.push(row_values);

        if matches!(tokens.get(pos), Some(SqlToken::Comma)) {
            pos = pos.saturating_add(1);
        } else {
            break;
        }
    }

    Ok(SqlStatement::Insert {
        table,
        columns: col_names,
        values: all_values,
    })
}

fn parse_update(tokens: &[SqlToken]) -> Result<SqlStatement, String> {
    let mut pos = 1; // Skip UPDATE

    let table = extract_identifier(tokens, pos)?;
    pos = pos.saturating_add(1);

    if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "SET") {
        return Err("Expected SET".to_owned());
    }
    pos = pos.saturating_add(1);

    let mut set_clauses = Vec::new();
    loop {
        let col = extract_identifier(tokens, pos)?;
        pos = pos.saturating_add(1);
        if !matches!(tokens.get(pos), Some(SqlToken::Operator(op)) if op == "=") {
            return Err("Expected = in SET clause".to_owned());
        }
        pos = pos.saturating_add(1);
        let val = extract_value_str(tokens, pos)?;
        pos = pos.saturating_add(1);
        set_clauses.push((col, val));
        if matches!(tokens.get(pos), Some(SqlToken::Comma)) {
            pos = pos.saturating_add(1);
        } else {
            break;
        }
    }

    let where_clause = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "WHERE") {
        pos = pos.saturating_add(1);
        let (wc, _new_pos) = parse_where_clause(tokens, pos)?;
        Some(wc)
    } else {
        None
    };

    Ok(SqlStatement::Update {
        table,
        set_clauses,
        where_clause,
    })
}

fn parse_delete(tokens: &[SqlToken]) -> Result<SqlStatement, String> {
    let mut pos = 1; // Skip DELETE

    if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "FROM") {
        return Err("Expected FROM after DELETE".to_owned());
    }
    pos = pos.saturating_add(1);

    let table = extract_identifier(tokens, pos)?;
    pos = pos.saturating_add(1);

    let where_clause = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "WHERE") {
        pos = pos.saturating_add(1);
        let (wc, _new_pos) = parse_where_clause(tokens, pos)?;
        Some(wc)
    } else {
        None
    };

    Ok(SqlStatement::Delete {
        table,
        where_clause,
    })
}

fn parse_create_table(tokens: &[SqlToken]) -> Result<SqlStatement, String> {
    let mut pos = 1; // Skip CREATE

    if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "TABLE") {
        return Err("Expected TABLE after CREATE".to_owned());
    }
    pos = pos.saturating_add(1);

    let if_not_exists = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "IF") {
        pos = pos.saturating_add(1);
        if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "NOT") {
            return Err("Expected NOT after IF".to_owned());
        }
        pos = pos.saturating_add(1);
        if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "EXISTS") {
            return Err("Expected EXISTS after NOT".to_owned());
        }
        pos = pos.saturating_add(1);
        true
    } else {
        false
    };

    let name = extract_identifier(tokens, pos)?;
    pos = pos.saturating_add(1);

    expect_token(tokens, pos, &SqlToken::LeftParen)?;
    pos = pos.saturating_add(1);

    let mut columns = Vec::new();
    loop {
        if matches!(tokens.get(pos), Some(SqlToken::RightParen)) {
            break;
        }
        let col_name = extract_identifier(tokens, pos)?;
        pos = pos.saturating_add(1);
        let col_type = extract_identifier_or_keyword(tokens, pos)?;
        pos = pos.saturating_add(1);

        let mut col = ParsedColumnDef {
            name: col_name,
            data_type: col_type,
            primary_key: false,
            not_null: false,
            unique: false,
            auto_increment: false,
            default_value: None,
        };

        // Parse column constraints
        loop {
            match tokens.get(pos) {
                Some(SqlToken::Keyword(k)) if k == "PRIMARY" => {
                    pos = pos.saturating_add(1);
                    if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "KEY") {
                        pos = pos.saturating_add(1);
                    }
                    col.primary_key = true;
                    col.not_null = true;
                }
                Some(SqlToken::Keyword(k)) if k == "NOT" => {
                    pos = pos.saturating_add(1);
                    if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "NULL") {
                        pos = pos.saturating_add(1);
                    }
                    col.not_null = true;
                }
                Some(SqlToken::Keyword(k)) if k == "UNIQUE" => {
                    pos = pos.saturating_add(1);
                    col.unique = true;
                }
                Some(SqlToken::Keyword(k)) if k == "DEFAULT" => {
                    pos = pos.saturating_add(1);
                    let val = extract_value_str(tokens, pos)?;
                    pos = pos.saturating_add(1);
                    col.default_value = Some(val);
                }
                Some(SqlToken::Identifier(s)) if s.to_uppercase() == "AUTOINCREMENT" => {
                    pos = pos.saturating_add(1);
                    col.auto_increment = true;
                }
                _ => break,
            }
        }

        columns.push(col);

        if matches!(tokens.get(pos), Some(SqlToken::Comma)) {
            pos = pos.saturating_add(1);
        } else {
            break;
        }
    }

    Ok(SqlStatement::CreateTable {
        name,
        columns,
        if_not_exists,
    })
}

fn parse_drop_table(tokens: &[SqlToken]) -> Result<SqlStatement, String> {
    let mut pos = 1; // Skip DROP

    if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "TABLE") {
        return Err("Expected TABLE after DROP".to_owned());
    }
    pos = pos.saturating_add(1);

    let if_exists = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "IF") {
        pos = pos.saturating_add(1);
        if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "EXISTS") {
            return Err("Expected EXISTS after IF".to_owned());
        }
        pos = pos.saturating_add(1);
        true
    } else {
        false
    };

    let name = extract_identifier(tokens, pos)?;

    Ok(SqlStatement::DropTable { name, if_exists })
}

// ============================================================================
// Parser helpers
// ============================================================================

fn expect_token(tokens: &[SqlToken], pos: usize, expected: &SqlToken) -> Result<(), String> {
    match tokens.get(pos) {
        Some(tok) if std::mem::discriminant(tok) == std::mem::discriminant(expected) => Ok(()),
        Some(tok) => Err(format!("Expected {expected:?}, got {tok:?}")),
        None => Err(format!("Unexpected end of input, expected {expected:?}")),
    }
}

fn extract_identifier(tokens: &[SqlToken], pos: usize) -> Result<String, String> {
    match tokens.get(pos) {
        Some(SqlToken::Identifier(name)) => Ok(name.clone()),
        Some(tok) => Err(format!("Expected identifier, got {tok:?}")),
        None => Err("Unexpected end of input".to_owned()),
    }
}

fn extract_identifier_or_keyword(tokens: &[SqlToken], pos: usize) -> Result<String, String> {
    match tokens.get(pos) {
        Some(SqlToken::Identifier(name)) => Ok(name.clone()),
        Some(SqlToken::Keyword(name)) => Ok(name.clone()),
        Some(tok) => Err(format!("Expected identifier or keyword, got {tok:?}")),
        None => Err("Unexpected end of input".to_owned()),
    }
}

fn extract_value_str(tokens: &[SqlToken], pos: usize) -> Result<String, String> {
    match tokens.get(pos) {
        Some(SqlToken::StringLiteral(s)) => Ok(s.clone()),
        Some(SqlToken::NumberLiteral(s)) => Ok(s.clone()),
        Some(SqlToken::Identifier(s)) => Ok(s.clone()),
        Some(SqlToken::Keyword(k)) if k == "NULL" => Ok("NULL".to_owned()),
        Some(tok) => Err(format!("Expected value, got {tok:?}")),
        None => Err("Unexpected end of input".to_owned()),
    }
}

fn extract_number(tokens: &[SqlToken], pos: usize) -> Result<usize, String> {
    match tokens.get(pos) {
        Some(SqlToken::NumberLiteral(s)) => s.parse::<usize>().map_err(|e| format!("Invalid number: {e}")),
        Some(tok) => Err(format!("Expected number, got {tok:?}")),
        None => Err("Unexpected end of input".to_owned()),
    }
}

fn parse_where_clause(tokens: &[SqlToken], pos: usize) -> Result<(WhereClause, usize), String> {
    let col = extract_identifier(tokens, pos)?;
    let mut p = pos.saturating_add(1);

    // Check for IS NULL / IS NOT NULL
    if matches!(tokens.get(p), Some(SqlToken::Keyword(k)) if k == "IS") {
        p = p.saturating_add(1);
        if matches!(tokens.get(p), Some(SqlToken::Keyword(k)) if k == "NOT") {
            p = p.saturating_add(1);
            if matches!(tokens.get(p), Some(SqlToken::Keyword(k)) if k == "NULL") {
                p = p.saturating_add(1);
            }
            return Ok((WhereClause { column: col, op: FilterOp::IsNotNull, value: String::new() }, p));
        }
        if matches!(tokens.get(p), Some(SqlToken::Keyword(k)) if k == "NULL") {
            p = p.saturating_add(1);
        }
        return Ok((WhereClause { column: col, op: FilterOp::IsNull, value: String::new() }, p));
    }

    // Check for LIKE
    if matches!(tokens.get(p), Some(SqlToken::Keyword(k)) if k == "LIKE") {
        p = p.saturating_add(1);
        let val = extract_value_str(tokens, p)?;
        p = p.saturating_add(1);
        return Ok((WhereClause { column: col, op: FilterOp::Like, value: val }, p));
    }

    // Regular operator
    let op = match tokens.get(p) {
        Some(SqlToken::Operator(s)) => match s.as_str() {
            "=" => FilterOp::Equal,
            "!=" | "<>" => FilterOp::NotEqual,
            "<" => FilterOp::LessThan,
            ">" => FilterOp::GreaterThan,
            "<=" => FilterOp::LessOrEqual,
            ">=" => FilterOp::GreaterOrEqual,
            _ => return Err(format!("Unknown operator: {s}")),
        },
        _ => return Err("Expected operator in WHERE clause".to_owned()),
    };
    p = p.saturating_add(1);

    let val = extract_value_str(tokens, p)?;
    p = p.saturating_add(1);

    Ok((WhereClause { column: col, op, value: val }, p))
}

// ============================================================================
// SQL execution engine
// ============================================================================

/// Execute a parsed SQL statement against a database.
fn execute_sql(db: &mut Database, stmt: &SqlStatement) -> QueryResult {
    match stmt {
        SqlStatement::Select { columns, table, where_clause, order_by, limit, offset, group_by } => {
            execute_select(db, columns, table, where_clause.as_ref(), order_by.as_ref(), *limit, *offset, group_by.as_deref())
        }
        SqlStatement::Insert { table, columns, values } => {
            execute_insert(db, table, columns, values)
        }
        SqlStatement::Update { table, set_clauses, where_clause } => {
            execute_update(db, table, set_clauses, where_clause.as_ref())
        }
        SqlStatement::Delete { table, where_clause } => {
            execute_delete(db, table, where_clause.as_ref())
        }
        SqlStatement::CreateTable { name, columns, if_not_exists } => {
            execute_create_table(db, name, columns, *if_not_exists)
        }
        SqlStatement::DropTable { name, if_exists } => {
            execute_drop_table(db, name, *if_exists)
        }
    }
}

fn execute_select(
    db: &Database,
    columns: &[SelectColumn],
    table_name: &str,
    where_clause: Option<&WhereClause>,
    order_by: Option<&(String, SortDir)>,
    limit: Option<usize>,
    offset: Option<usize>,
    group_by: Option<&str>,
) -> QueryResult {
    let table = match db.find_table(table_name) {
        Some(t) => t,
        None => return QueryResult::error(&format!("Table '{table_name}' not found")),
    };

    // Apply WHERE filter
    let mut filtered_rows: Vec<&Vec<CellValue>> = table.rows.iter().collect();
    if let Some(wc) = where_clause {
        let col_idx = match table.column_index(&wc.column) {
            Some(idx) => idx,
            None => return QueryResult::error(&format!("Column '{}' not found", wc.column)),
        };
        let filter_value = if wc.value.is_empty() {
            CellValue::Null
        } else if let Some(col) = table.columns.get(col_idx) {
            CellValue::parse_as(&wc.value, &col.data_type)
        } else {
            CellValue::Text(wc.value.clone())
        };
        filtered_rows.retain(|row| {
            row.get(col_idx)
                .map_or(false, |cell| matches_filter(cell, &wc.op, &filter_value))
        });
    }

    // Handle GROUP BY with aggregates
    if let Some(group_col_name) = group_by {
        return execute_grouped_select(table, columns, &filtered_rows, group_col_name, order_by, limit, offset);
    }

    // Check if we have aggregate functions without GROUP BY
    let has_aggregates = columns.iter().any(|c| matches!(c, SelectColumn::Aggregate { .. }));
    if has_aggregates {
        return execute_aggregate_select(table, columns, &filtered_rows);
    }

    // Determine output columns
    let (out_col_names, col_indices) = resolve_columns(table, columns);

    // Build result rows
    let mut result_rows: Vec<Vec<CellValue>> = filtered_rows
        .iter()
        .map(|row| {
            col_indices
                .iter()
                .map(|&idx| row.get(idx).cloned().unwrap_or(CellValue::Null))
                .collect()
        })
        .collect();

    // ORDER BY
    if let Some((col_name, dir)) = order_by {
        if let Some(sort_idx) = out_col_names.iter().position(|n| n.to_uppercase() == col_name.to_uppercase()) {
            result_rows.sort_by(|a, b| {
                let va = a.get(sort_idx).map_or(SortKey::Null, |v| v.as_sort_key());
                let vb = b.get(sort_idx).map_or(SortKey::Null, |v| v.as_sort_key());
                match dir {
                    SortDir::Ascending => va.cmp(&vb),
                    SortDir::Descending => vb.cmp(&va),
                }
            });
        }
    }

    // OFFSET
    if let Some(off) = offset {
        if off < result_rows.len() {
            result_rows = result_rows.into_iter().skip(off).collect();
        } else {
            result_rows.clear();
        }
    }

    // LIMIT
    if let Some(lim) = limit {
        result_rows.truncate(lim);
    }

    QueryResult::with_data(out_col_names, result_rows)
}

fn resolve_columns(table: &Table, columns: &[SelectColumn]) -> (Vec<String>, Vec<usize>) {
    let mut names = Vec::new();
    let mut indices = Vec::new();

    for col in columns {
        match col {
            SelectColumn::AllColumns => {
                for (i, c) in table.columns.iter().enumerate() {
                    names.push(c.name.clone());
                    indices.push(i);
                }
            }
            SelectColumn::Named(name) => {
                if let Some(idx) = table.column_index(name) {
                    if let Some(c) = table.columns.get(idx) {
                        names.push(c.name.clone());
                    }
                    indices.push(idx);
                }
            }
            SelectColumn::Aggregate { .. } => {
                // Handled separately
            }
        }
    }

    (names, indices)
}

fn execute_aggregate_select(
    table: &Table,
    columns: &[SelectColumn],
    rows: &[&Vec<CellValue>],
) -> QueryResult {
    let mut out_names = Vec::new();
    let mut out_values = Vec::new();

    for col in columns {
        if let SelectColumn::Aggregate { func, column, alias } = col {
            let name = alias.clone().unwrap_or_else(|| format!("{}({})", func.label(), column));
            out_names.push(name);

            let col_idx = if column == "*" {
                None
            } else {
                table.column_index(column)
            };

            let value = compute_aggregate(*func, rows, col_idx);
            out_values.push(value);
        }
    }

    QueryResult::with_data(out_names, vec![out_values])
}

fn execute_grouped_select(
    table: &Table,
    columns: &[SelectColumn],
    rows: &[&Vec<CellValue>],
    group_col_name: &str,
    order_by: Option<&(String, SortDir)>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> QueryResult {
    let group_idx = match table.column_index(group_col_name) {
        Some(idx) => idx,
        None => return QueryResult::error(&format!("Column '{group_col_name}' not found")),
    };

    // Group rows by the group column value
    let mut groups: Vec<(CellValue, Vec<&Vec<CellValue>>)> = Vec::new();
    for row in rows {
        let key = row.get(group_idx).cloned().unwrap_or(CellValue::Null);
        let found = groups.iter_mut().find(|(k, _)| k.as_sort_key() == key.as_sort_key());
        if let Some((_, group_rows)) = found {
            group_rows.push(row);
        } else {
            groups.push((key, vec![row]));
        }
    }

    // Build result
    let mut out_names = Vec::new();
    let mut result_rows = Vec::new();

    // Determine column names
    for col in columns {
        match col {
            SelectColumn::Named(name) => out_names.push(name.clone()),
            SelectColumn::Aggregate { func, column, alias } => {
                out_names.push(alias.clone().unwrap_or_else(|| format!("{}({})", func.label(), column)));
            }
            SelectColumn::AllColumns => {
                for c in &table.columns {
                    out_names.push(c.name.clone());
                }
            }
        }
    }

    for (group_key, group_rows) in &groups {
        let mut row_values = Vec::new();
        for col in columns {
            match col {
                SelectColumn::Named(name) => {
                    if name.to_uppercase() == group_col_name.to_uppercase() {
                        row_values.push(group_key.clone());
                    } else if let Some(idx) = table.column_index(name) {
                        row_values.push(
                            group_rows.first()
                                .and_then(|r| r.get(idx))
                                .cloned()
                                .unwrap_or(CellValue::Null)
                        );
                    }
                }
                SelectColumn::Aggregate { func, column, .. } => {
                    let col_idx = if column == "*" {
                        None
                    } else {
                        table.column_index(column)
                    };
                    let group_row_refs: Vec<&Vec<CellValue>> = group_rows.iter().copied().collect();
                    let value = compute_aggregate(*func, &group_row_refs, col_idx);
                    row_values.push(value);
                }
                SelectColumn::AllColumns => {
                    if let Some(first) = group_rows.first() {
                        row_values.extend(first.iter().cloned());
                    }
                }
            }
        }
        result_rows.push(row_values);
    }

    // ORDER BY
    if let Some((col_name, dir)) = order_by {
        if let Some(sort_idx) = out_names.iter().position(|n| n.to_uppercase() == col_name.to_uppercase()) {
            result_rows.sort_by(|a, b| {
                let va = a.get(sort_idx).map_or(SortKey::Null, |v| v.as_sort_key());
                let vb = b.get(sort_idx).map_or(SortKey::Null, |v| v.as_sort_key());
                match dir {
                    SortDir::Ascending => va.cmp(&vb),
                    SortDir::Descending => vb.cmp(&va),
                }
            });
        }
    }

    // OFFSET
    if let Some(off) = offset {
        if off < result_rows.len() {
            result_rows = result_rows.into_iter().skip(off).collect();
        } else {
            result_rows.clear();
        }
    }

    // LIMIT
    if let Some(lim) = limit {
        result_rows.truncate(lim);
    }

    QueryResult::with_data(out_names, result_rows)
}

fn compute_aggregate(func: AggFunc, rows: &[&Vec<CellValue>], col_idx: Option<usize>) -> CellValue {
    match func {
        AggFunc::Count => {
            if let Some(idx) = col_idx {
                let count = rows.iter()
                    .filter(|r| r.get(idx).map_or(false, |v| *v != CellValue::Null))
                    .count();
                CellValue::Integer(count as i64)
            } else {
                CellValue::Integer(rows.len() as i64)
            }
        }
        AggFunc::Sum => {
            let idx = col_idx.unwrap_or(0);
            let sum: f64 = rows.iter().filter_map(|r| r.get(idx)).map(|v| match v {
                CellValue::Integer(n) => *n as f64,
                CellValue::Real(n) => *n,
                _ => 0.0,
            }).sum();
            CellValue::Real(sum)
        }
        AggFunc::Avg => {
            let idx = col_idx.unwrap_or(0);
            let values: Vec<f64> = rows.iter().filter_map(|r| r.get(idx)).filter_map(|v| match v {
                CellValue::Integer(n) => Some(*n as f64),
                CellValue::Real(n) => Some(*n),
                _ => None,
            }).collect();
            if values.is_empty() {
                CellValue::Null
            } else {
                let sum: f64 = values.iter().sum();
                CellValue::Real(sum / values.len() as f64)
            }
        }
        AggFunc::Min => {
            let idx = col_idx.unwrap_or(0);
            rows.iter()
                .filter_map(|r| r.get(idx))
                .filter(|v| **v != CellValue::Null)
                .min_by(|a, b| a.as_sort_key().cmp(&b.as_sort_key()))
                .cloned()
                .unwrap_or(CellValue::Null)
        }
        AggFunc::Max => {
            let idx = col_idx.unwrap_or(0);
            rows.iter()
                .filter_map(|r| r.get(idx))
                .filter(|v| **v != CellValue::Null)
                .max_by(|a, b| a.as_sort_key().cmp(&b.as_sort_key()))
                .cloned()
                .unwrap_or(CellValue::Null)
        }
    }
}

fn execute_insert(db: &mut Database, table_name: &str, col_names: &[String], values: &[Vec<String>]) -> QueryResult {
    let table = match db.find_table_mut(table_name) {
        Some(t) => t,
        None => return QueryResult::error(&format!("Table '{table_name}' not found")),
    };

    let col_count = table.col_count();
    let mut inserted = 0usize;

    // If column names provided, map values to correct positions
    let col_indices: Vec<usize> = if col_names.is_empty() {
        (0..col_count).collect()
    } else {
        let mut indices = Vec::new();
        for name in col_names {
            match table.column_index(name) {
                Some(idx) => indices.push(idx),
                None => return QueryResult::error(&format!("Column '{name}' not found")),
            }
        }
        indices
    };

    for val_row in values {
        let mut row = vec![CellValue::Null; col_count];
        for (vi, &ci) in col_indices.iter().enumerate() {
            if let Some(val_str) = val_row.get(vi) {
                if let Some(col) = table.columns.get(ci) {
                    if let Some(cell) = row.get_mut(ci) {
                        *cell = CellValue::parse_as(val_str, &col.data_type);
                    }
                }
            }
        }

        // Fill defaults
        for (i, col) in table.columns.iter().enumerate() {
            if let Some(cell) = row.get(i) {
                if *cell == CellValue::Null {
                    if let Some(ref def) = col.constraints.default_value {
                        if let Some(cell_mut) = row.get_mut(i) {
                            *cell_mut = CellValue::parse_as(def, &col.data_type);
                        }
                    }
                }
            }
        }

        match table.insert_row(row) {
            Ok(()) => inserted = inserted.saturating_add(1),
            Err(e) => return QueryResult::error(&format!("Insert failed: {e}")),
        }
    }

    let mut result = QueryResult::success(&format!("{inserted} row(s) inserted"));
    result.affected_rows = inserted;
    result
}

fn execute_update(db: &mut Database, table_name: &str, set_clauses: &[(String, String)], where_clause: Option<&WhereClause>) -> QueryResult {
    let table = match db.find_table_mut(table_name) {
        Some(t) => t,
        None => return QueryResult::error(&format!("Table '{table_name}' not found")),
    };

    let mut total_updated = 0usize;

    for (set_col_name, set_val_str) in set_clauses {
        let set_col_idx = match table.column_index(set_col_name) {
            Some(idx) => idx,
            None => return QueryResult::error(&format!("Column '{set_col_name}' not found")),
        };
        let set_value = if let Some(col) = table.columns.get(set_col_idx) {
            CellValue::parse_as(set_val_str, &col.data_type)
        } else {
            CellValue::Text(set_val_str.clone())
        };

        if let Some(wc) = where_clause {
            let where_col_idx = match table.column_index(&wc.column) {
                Some(idx) => idx,
                None => return QueryResult::error(&format!("Column '{}' not found", wc.column)),
            };
            let where_value = if let Some(col) = table.columns.get(where_col_idx) {
                CellValue::parse_as(&wc.value, &col.data_type)
            } else {
                CellValue::Text(wc.value.clone())
            };
            total_updated = total_updated.saturating_add(
                table.update_where(set_col_idx, &set_value, where_col_idx, &wc.op, &where_value)
            );
        } else {
            // Update all rows
            for row in &mut table.rows {
                if let Some(cell) = row.get_mut(set_col_idx) {
                    *cell = set_value.clone();
                    total_updated = total_updated.saturating_add(1);
                }
            }
        }
    }

    let mut result = QueryResult::success(&format!("{total_updated} row(s) updated"));
    result.affected_rows = total_updated;
    result
}

fn execute_delete(db: &mut Database, table_name: &str, where_clause: Option<&WhereClause>) -> QueryResult {
    let table = match db.find_table_mut(table_name) {
        Some(t) => t,
        None => return QueryResult::error(&format!("Table '{table_name}' not found")),
    };

    let deleted = if let Some(wc) = where_clause {
        let col_idx = match table.column_index(&wc.column) {
            Some(idx) => idx,
            None => return QueryResult::error(&format!("Column '{}' not found", wc.column)),
        };
        let filter_value = if let Some(col) = table.columns.get(col_idx) {
            CellValue::parse_as(&wc.value, &col.data_type)
        } else {
            CellValue::Text(wc.value.clone())
        };
        table.delete_where(col_idx, &wc.op, &filter_value)
    } else {
        let count = table.rows.len();
        table.rows.clear();
        count
    };

    let mut result = QueryResult::success(&format!("{deleted} row(s) deleted"));
    result.affected_rows = deleted;
    result
}

fn execute_create_table(db: &mut Database, name: &str, columns: &[ParsedColumnDef], if_not_exists: bool) -> QueryResult {
    if if_not_exists && db.find_table(name).is_some() {
        return QueryResult::success("Table already exists (IF NOT EXISTS)");
    }

    let col_defs: Vec<ColumnDef> = columns.iter().map(|pc| {
        let mut cd = ColumnDef::new(&pc.name, DataType::from_str_loose(&pc.data_type));
        cd.constraints.primary_key = pc.primary_key;
        cd.constraints.not_null = pc.not_null;
        cd.constraints.unique = pc.unique;
        cd.constraints.auto_increment = pc.auto_increment;
        cd.constraints.default_value = pc.default_value.clone();
        cd
    }).collect();

    let table = Table::new(name, col_defs);
    match db.create_table(table) {
        Ok(()) => QueryResult::success(&format!("Table '{name}' created")),
        Err(e) => QueryResult::error(&e),
    }
}

fn execute_drop_table(db: &mut Database, name: &str, if_exists: bool) -> QueryResult {
    if if_exists && db.find_table(name).is_none() {
        return QueryResult::success("Table does not exist (IF EXISTS)");
    }
    match db.drop_table(name) {
        Ok(()) => QueryResult::success(&format!("Table '{name}' dropped")),
        Err(e) => QueryResult::error(&e),
    }
}

// ============================================================================
// Export functions
// ============================================================================

/// Export table data as CSV.
pub fn export_csv(table: &Table) -> String {
    let mut out = String::new();
    // Header
    let headers: Vec<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();
    out.push_str(&headers.join(","));
    out.push('\n');

    // Data
    for row in &table.rows {
        let vals: Vec<String> = row.iter().map(|v| {
            match v {
                CellValue::Text(s) => format!("\"{}\"", s.replace('"', "\"\"")),
                other => other.display(),
            }
        }).collect();
        out.push_str(&vals.join(","));
        out.push('\n');
    }
    out
}

/// Export table data as JSON.
pub fn export_json(table: &Table) -> String {
    let mut out = String::from("[\n");
    for (ri, row) in table.rows.iter().enumerate() {
        out.push_str("  {");
        for (ci, val) in row.iter().enumerate() {
            if ci > 0 {
                out.push_str(", ");
            }
            let col_name = table.columns.get(ci).map_or("?", |c| c.name.as_str());
            match val {
                CellValue::Integer(n) => out.push_str(&format!("\"{col_name}\": {n}")),
                CellValue::Real(n) => out.push_str(&format!("\"{col_name}\": {n}")),
                CellValue::Text(s) => out.push_str(&format!("\"{col_name}\": \"{}\"", s.replace('"', "\\\""))),
                CellValue::Blob(b) => out.push_str(&format!("\"{col_name}\": \"<blob:{}>\"", b.len())),
                CellValue::Null => out.push_str(&format!("\"{col_name}\": null")),
            }
        }
        out.push('}');
        if ri < table.rows.len().saturating_sub(1) {
            out.push(',');
        }
        out.push('\n');
    }
    out.push(']');
    out
}

/// Export table data as SQL INSERT statements.
pub fn export_sql_inserts(table: &Table) -> String {
    let mut out = String::new();
    let col_names: Vec<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();
    let cols_str = col_names.join(", ");

    for row in &table.rows {
        let vals: Vec<String> = row.iter().map(|v| match v {
            CellValue::Integer(n) => n.to_string(),
            CellValue::Real(n) => format!("{n}"),
            CellValue::Text(s) => format!("'{}'", s.replace('\'', "''")),
            CellValue::Blob(_) => "X''".to_owned(),
            CellValue::Null => "NULL".to_owned(),
        }).collect();
        out.push_str(&format!(
            "INSERT INTO {} ({}) VALUES ({});\n",
            table.name,
            cols_str,
            vals.join(", ")
        ));
    }
    out
}

// ============================================================================
// Import CSV
// ============================================================================

/// Import CSV data into a table. Returns the parsed table.
pub fn import_csv(name: &str, csv_data: &str) -> Result<Table, String> {
    let mut lines = csv_data.lines();

    // Detect header
    let header_line = lines.next().ok_or_else(|| "Empty CSV data".to_owned())?;
    let headers: Vec<&str> = header_line.split(',').map(str::trim).collect();

    if headers.is_empty() {
        return Err("No columns found in CSV header".to_owned());
    }

    let columns: Vec<ColumnDef> = headers
        .iter()
        .map(|h| ColumnDef::new(h, DataType::Text))
        .collect();

    let mut table = Table::new(name, columns);

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let values: Vec<CellValue> = parse_csv_line(line, headers.len());
        if values.len() == table.col_count() {
            let _ = table.insert_row(values);
        }
    }

    // Attempt type inference on the data
    infer_column_types(&mut table);

    Ok(table)
}

fn parse_csv_line(line: &str, expected_cols: usize) -> Vec<CellValue> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars.get(i).copied().unwrap_or(' ');
        if in_quotes {
            if ch == '"' {
                if i.saturating_add(1) < chars.len() && chars.get(i.saturating_add(1)) == Some(&'"') {
                    current.push('"');
                    i = i.saturating_add(2);
                } else {
                    in_quotes = false;
                    i = i.saturating_add(1);
                }
            } else {
                current.push(ch);
                i = i.saturating_add(1);
            }
        } else if ch == '"' {
            in_quotes = true;
            i = i.saturating_add(1);
        } else if ch == ',' {
            values.push(CellValue::Text(current.trim().to_owned()));
            current.clear();
            i = i.saturating_add(1);
        } else {
            current.push(ch);
            i = i.saturating_add(1);
        }
    }
    values.push(CellValue::Text(current.trim().to_owned()));

    // Pad to expected length
    while values.len() < expected_cols {
        values.push(CellValue::Null);
    }

    values
}

/// Infer and convert column types based on data patterns.
fn infer_column_types(table: &mut Table) {
    for col_idx in 0..table.col_count() {
        let all_int = table.rows.iter().all(|row| {
            row.get(col_idx).map_or(true, |v| match v {
                CellValue::Text(s) => s.is_empty() || s.parse::<i64>().is_ok(),
                CellValue::Null => true,
                _ => false,
            })
        });

        if all_int && !table.rows.is_empty() {
            if let Some(col) = table.columns.get_mut(col_idx) {
                col.data_type = DataType::Integer;
            }
            for row in &mut table.rows {
                if let Some(cell) = row.get_mut(col_idx) {
                    if let CellValue::Text(s) = cell {
                        if let Ok(n) = s.parse::<i64>() {
                            *cell = CellValue::Integer(n);
                        }
                    }
                }
            }
            continue;
        }

        let all_real = table.rows.iter().all(|row| {
            row.get(col_idx).map_or(true, |v| match v {
                CellValue::Text(s) => s.is_empty() || s.parse::<f64>().is_ok(),
                CellValue::Null => true,
                _ => false,
            })
        });

        if all_real && !table.rows.is_empty() {
            if let Some(col) = table.columns.get_mut(col_idx) {
                col.data_type = DataType::Real;
            }
            for row in &mut table.rows {
                if let Some(cell) = row.get_mut(col_idx) {
                    if let CellValue::Text(s) = cell {
                        if let Ok(n) = s.parse::<f64>() {
                            *cell = CellValue::Real(n);
                        }
                    }
                }
            }
        }
    }
}

// ============================================================================
// Object tree sidebar items
// ============================================================================

/// Sidebar tree node types.
#[derive(Clone, Debug, PartialEq)]
pub enum TreeNodeKind {
    TablesHeader,
    Table(String),
    IndexesHeader,
    Index(String),
    ViewsHeader,
    View(String),
    TriggersHeader,
    Trigger(String),
}

/// A node in the sidebar object tree.
#[derive(Clone, Debug)]
pub struct TreeNode {
    pub kind: TreeNodeKind,
    pub expanded: bool,
    pub depth: usize,
}

fn build_tree_nodes(db: &Database) -> Vec<TreeNode> {
    let mut nodes = Vec::new();

    // Tables
    nodes.push(TreeNode {
        kind: TreeNodeKind::TablesHeader,
        expanded: true,
        depth: 0,
    });
    for name in db.table_names() {
        nodes.push(TreeNode {
            kind: TreeNodeKind::Table(name),
            expanded: false,
            depth: 1,
        });
    }

    // Indexes
    nodes.push(TreeNode {
        kind: TreeNodeKind::IndexesHeader,
        expanded: true,
        depth: 0,
    });
    for idx in &db.indexes {
        nodes.push(TreeNode {
            kind: TreeNodeKind::Index(idx.name.clone()),
            expanded: false,
            depth: 1,
        });
    }

    // Views
    nodes.push(TreeNode {
        kind: TreeNodeKind::ViewsHeader,
        expanded: true,
        depth: 0,
    });
    for view in &db.views {
        nodes.push(TreeNode {
            kind: TreeNodeKind::View(view.name.clone()),
            expanded: false,
            depth: 1,
        });
    }

    // Triggers
    nodes.push(TreeNode {
        kind: TreeNodeKind::TriggersHeader,
        expanded: true,
        depth: 0,
    });
    for trigger in &db.triggers {
        nodes.push(TreeNode {
            kind: TreeNodeKind::Trigger(trigger.name.clone()),
            expanded: false,
            depth: 1,
        });
    }

    nodes
}

// ============================================================================
// Active panels
// ============================================================================

/// Which bottom panel is active.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BottomPanel {
    SqlEditor,
    Results,
    Schema,
    Diagram,
}

impl BottomPanel {
    fn label(&self) -> &'static str {
        match self {
            Self::SqlEditor => "SQL Editor",
            Self::Results => "Results",
            Self::Schema => "Schema",
            Self::Diagram => "Diagram",
        }
    }

    fn all() -> &'static [Self] {
        &[Self::SqlEditor, Self::Results, Self::Schema, Self::Diagram]
    }
}

// ============================================================================
// Database tab (connection)
// ============================================================================

/// A database connection tab.
#[derive(Clone, Debug)]
pub struct DbTab {
    pub db: Database,
    pub selected_table: Option<String>,
    pub sort_state: Option<SortState>,
    pub page: usize,
    pub filters: Vec<ActiveFilter>,
    pub tree_nodes: Vec<TreeNode>,
}

impl DbTab {
    fn new(db: Database) -> Self {
        let tree_nodes = build_tree_nodes(&db);
        let first_table = db.tables.first().map(|t| t.name.clone());
        Self {
            db,
            selected_table: first_table,
            sort_state: None,
            page: 0,
            filters: Vec::new(),
            tree_nodes,
        }
    }

    fn refresh_tree(&mut self) {
        self.tree_nodes = build_tree_nodes(&self.db);
    }

    /// Get the current table data with sorting and filtering applied.
    fn current_table_data(&self) -> Option<(Vec<String>, Vec<Vec<CellValue>>)> {
        let table_name = self.selected_table.as_ref()?;
        let table = self.db.find_table(table_name)?;

        let col_names: Vec<String> = table.columns.iter().map(|c| c.name.clone()).collect();
        let mut rows: Vec<Vec<CellValue>> = table.rows.clone();

        // Apply filters
        for filter in &self.filters {
            if let Some(col) = table.columns.get(filter.column_idx) {
                let filter_value = CellValue::parse_as(&filter.value_str, &col.data_type);
                rows.retain(|row| {
                    row.get(filter.column_idx)
                        .map_or(false, |cell| matches_filter(cell, &filter.op, &filter_value))
                });
            }
        }

        // Apply sorting
        if let Some(ref sort) = self.sort_state {
            let sort_idx = sort.column_idx;
            let dir = sort.direction;
            rows.sort_by(|a, b| {
                let va = a.get(sort_idx).map_or(SortKey::Null, |v| v.as_sort_key());
                let vb = b.get(sort_idx).map_or(SortKey::Null, |v| v.as_sort_key());
                match dir {
                    SortDir::Ascending => va.cmp(&vb),
                    SortDir::Descending => vb.cmp(&va),
                }
            });
        }

        Some((col_names, rows))
    }
}

// ============================================================================
// Application state
// ============================================================================

/// Main application state.
pub struct DbViewerApp {
    pub tabs: Vec<DbTab>,
    pub active_tab: usize,
    pub sql_input: String,
    pub query_result: Option<QueryResult>,
    pub history: Vec<HistoryEntry>,
    pub history_counter: u64,
    pub bottom_panel: BottomPanel,
    pub show_filter_builder: bool,
    pub filter_column_idx: usize,
    pub filter_op_idx: usize,
    pub filter_value: String,
}

impl DbViewerApp {
    pub fn new() -> Self {
        let sample_db = Database::sample();
        let tab = DbTab::new(sample_db);

        Self {
            tabs: vec![tab],
            active_tab: 0,
            sql_input: String::from("SELECT * FROM users"),
            query_result: None,
            history: Vec::new(),
            history_counter: 0,
            bottom_panel: BottomPanel::SqlEditor,
            show_filter_builder: false,
            filter_column_idx: 0,
            filter_op_idx: 0,
            filter_value: String::new(),
        }
    }

    /// Get the active tab.
    fn active_db_tab(&self) -> Option<&DbTab> {
        self.tabs.get(self.active_tab)
    }

    /// Get the active tab mutably.
    fn active_db_tab_mut(&mut self) -> Option<&mut DbTab> {
        self.tabs.get_mut(self.active_tab)
    }

    /// Execute the current SQL query.
    pub fn execute_query(&mut self) {
        let sql = self.sql_input.clone();
        if sql.trim().is_empty() {
            self.query_result = Some(QueryResult::error("Empty query"));
            return;
        }

        let result = match parse_sql(&sql) {
            Ok(stmt) => {
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    let result = execute_sql(&mut tab.db, &stmt);
                    tab.refresh_tree();
                    result
                } else {
                    QueryResult::error("No active database")
                }
            }
            Err(e) => QueryResult::error(&format!("Parse error: {e}")),
        };

        // Add to history
        self.history_counter = self.history_counter.saturating_add(1);
        self.history.push(HistoryEntry {
            sql: sql.clone(),
            success: !result.is_error,
            message: result.message.clone(),
            favorite: false,
            timestamp_counter: self.history_counter,
        });

        self.query_result = Some(result);
        self.bottom_panel = BottomPanel::Results;
    }

    /// Toggle a history entry's favorite status.
    pub fn toggle_favorite(&mut self, idx: usize) {
        if let Some(entry) = self.history.get_mut(idx) {
            entry.favorite = !entry.favorite;
        }
    }

    /// Add a new empty database tab.
    pub fn add_tab(&mut self, name: &str) {
        let db = Database::new(name);
        self.tabs.push(DbTab::new(db));
        self.active_tab = self.tabs.len().saturating_sub(1);
    }

    /// Close a tab.
    pub fn close_tab(&mut self, idx: usize) {
        if self.tabs.len() <= 1 {
            return; // Keep at least one tab
        }
        if idx < self.tabs.len() {
            self.tabs.remove(idx);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len().saturating_sub(1);
            }
        }
    }

    /// Select a table in the current tab's sidebar.
    pub fn select_table(&mut self, name: &str) {
        if let Some(tab) = self.active_db_tab_mut() {
            tab.selected_table = Some(name.to_owned());
            tab.page = 0;
            tab.sort_state = None;
            tab.filters.clear();
        }
    }

    /// Toggle sort on a column.
    pub fn toggle_sort(&mut self, col_idx: usize) {
        if let Some(tab) = self.active_db_tab_mut() {
            tab.sort_state = Some(match &tab.sort_state {
                Some(s) if s.column_idx == col_idx => SortState {
                    column_idx: col_idx,
                    direction: match s.direction {
                        SortDir::Ascending => SortDir::Descending,
                        SortDir::Descending => SortDir::Ascending,
                    },
                },
                _ => SortState {
                    column_idx: col_idx,
                    direction: SortDir::Ascending,
                },
            });
        }
    }

    /// Navigate to next page.
    pub fn next_page(&mut self) {
        if let Some(tab) = self.active_db_tab_mut() {
            if let Some(table_name) = &tab.selected_table {
                if let Some(table) = tab.db.find_table(table_name) {
                    let max_page = table.row_count().saturating_sub(1) / PAGE_SIZE;
                    if tab.page < max_page {
                        tab.page = tab.page.saturating_add(1);
                    }
                }
            }
        }
    }

    /// Navigate to previous page.
    pub fn prev_page(&mut self) {
        if let Some(tab) = self.active_db_tab_mut() {
            tab.page = tab.page.saturating_sub(1);
        }
    }

    /// Add a filter from the filter builder.
    pub fn add_filter(&mut self) {
        let op = FilterOp::all()
            .get(self.filter_op_idx)
            .cloned()
            .unwrap_or(FilterOp::Equal);

        let filter = ActiveFilter {
            column_idx: self.filter_column_idx,
            op,
            value_str: self.filter_value.clone(),
        };

        if let Some(tab) = self.active_db_tab_mut() {
            tab.filters.push(filter);
            tab.page = 0;
        }
        self.filter_value.clear();
    }

    /// Remove a filter.
    pub fn remove_filter(&mut self, idx: usize) {
        if let Some(tab) = self.active_db_tab_mut() {
            if idx < tab.filters.len() {
                tab.filters.remove(idx);
                tab.page = 0;
            }
        }
    }

    /// Delete a row from the selected table.
    pub fn delete_row(&mut self, row_idx: usize) {
        if let Some(tab) = self.active_db_tab_mut() {
            if let Some(table_name) = tab.selected_table.clone() {
                if let Some(table) = tab.db.find_table_mut(&table_name) {
                    if row_idx < table.rows.len() {
                        table.rows.remove(row_idx);
                    }
                }
            }
        }
    }

    /// Export current table data in the specified format.
    pub fn export_current_table(&self, format: ExportFormat) -> Option<String> {
        let tab = self.active_db_tab()?;
        let table_name = tab.selected_table.as_ref()?;
        let table = tab.db.find_table(table_name)?;

        Some(match format {
            ExportFormat::Csv => export_csv(table),
            ExportFormat::Json => export_json(table),
            ExportFormat::SqlInserts => export_sql_inserts(table),
        })
    }

    /// Import CSV data into the active database.
    pub fn import_csv_data(&mut self, name: &str, csv_data: &str) -> Result<(), String> {
        let table = import_csv(name, csv_data)?;
        if let Some(tab) = self.active_db_tab_mut() {
            tab.db.create_table(table)?;
            tab.refresh_tree();
        }
        Ok(())
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the entire application UI. Returns a flat list of render commands.
    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Toolbar
        self.render_toolbar(&mut cmds, width);

        // Database tabs
        let tabs_y = TOOLBAR_HEIGHT;
        self.render_db_tabs(&mut cmds, 0.0, tabs_y, width);

        let content_y = tabs_y + TAB_HEIGHT;
        let content_height = height - content_y - STATUS_BAR_HEIGHT;

        // Sidebar
        self.render_sidebar(&mut cmds, 0.0, content_y, SIDEBAR_WIDTH, content_height);

        // Main content area
        let main_x = SIDEBAR_WIDTH;
        let main_width = width - SIDEBAR_WIDTH;

        // Data grid (top portion)
        let grid_height = content_height - EDITOR_HEIGHT;
        self.render_data_grid(&mut cmds, main_x, content_y, main_width, grid_height);

        // Bottom panels
        let bottom_y = content_y + grid_height;
        self.render_bottom_panels(&mut cmds, main_x, bottom_y, main_width, EDITOR_HEIGHT);

        // Status bar
        self.render_status_bar(&mut cmds, 0.0, height - STATUS_BAR_HEIGHT, width);

        cmds
    }

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>, width: f32) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height: TOOLBAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: 10.0,
            text: "DB Viewer".to_owned(),
            color: BLUE,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(100.0),
        });

        // Toolbar buttons
        let buttons = ["Execute", "New Tab", "Export", "Import"];
        let colors = [GREEN, BLUE, PEACH, TEAL];
        let mut bx = 130.0;
        for (i, label) in buttons.iter().enumerate() {
            let color = colors.get(i).copied().unwrap_or(SUBTEXT0);
            let btn_w = label.len() as f32 * 8.0 + 16.0;
            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: 6.0,
                width: btn_w,
                height: 24.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 8.0,
                y: 10.0,
                text: (*label).to_owned(),
                color,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(btn_w),
            });
            bx += btn_w + 8.0;
        }
    }

    fn render_db_tabs(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: TAB_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let mut tx = x + 4.0;
        for (i, tab) in self.tabs.iter().enumerate() {
            let is_active = i == self.active_tab;
            let tab_label = &tab.db.name;
            let tw = tab_label.len() as f32 * 7.5 + 32.0;

            let bg = if is_active { BASE } else { CRUST };
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: y + 4.0,
                width: tw,
                height: TAB_HEIGHT - 4.0,
                color: bg,
                corner_radii: CornerRadii {
                    top_left: CORNER_RADIUS,
                    top_right: CORNER_RADIUS,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
            });

            cmds.push(RenderCommand::Text {
                x: tx + 8.0,
                y: y + 10.0,
                text: tab_label.clone(),
                color: if is_active { TEXT } else { SUBTEXT0 },
                font_size: 11.0,
                font_weight: if is_active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(tw - 16.0),
            });

            // Close button
            cmds.push(RenderCommand::Text {
                x: tx + tw - 16.0,
                y: y + 10.0,
                text: "x".to_owned(),
                color: OVERLAY0,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(12.0),
            });

            tx += tw + 2.0;
        }

        // New tab button
        cmds.push(RenderCommand::FillRect {
            x: tx,
            y: y + 6.0,
            width: 24.0,
            height: 20.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: tx + 7.0,
            y: y + 9.0,
            text: "+".to_owned(),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(12.0),
        });
    }

    fn render_sidebar(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32, height: f32) {
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Sidebar border
        cmds.push(RenderCommand::Line {
            x1: x + width,
            y1: y,
            x2: x + width,
            y2: y + height,
            color: SURFACE0,
            width: 1.0,
        });

        let tab = match self.active_db_tab() {
            Some(t) => t,
            None => return,
        };

        let mut ny = y + 8.0;

        // Database name
        cmds.push(RenderCommand::Text {
            x: x + 10.0,
            y: ny,
            text: tab.db.name.clone(),
            color: BLUE,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 20.0),
        });
        ny += 22.0;

        // Separator
        cmds.push(RenderCommand::Line {
            x1: x + 8.0,
            y1: ny,
            x2: x + width - 8.0,
            y2: ny,
            color: SURFACE0,
            width: 1.0,
        });
        ny += 8.0;

        // Tree nodes
        for node in &tab.tree_nodes {
            if ny > y + height {
                break;
            }

            let indent = node.depth as f32 * 16.0;
            let is_selected = match &node.kind {
                TreeNodeKind::Table(name) => tab.selected_table.as_deref() == Some(name.as_str()),
                _ => false,
            };

            let (icon, label, color) = match &node.kind {
                TreeNodeKind::TablesHeader => ("T", "Tables".to_owned(), BLUE),
                TreeNodeKind::Table(name) => ("  ", name.clone(), if is_selected { TEXT } else { SUBTEXT1 }),
                TreeNodeKind::IndexesHeader => ("I", "Indexes".to_owned(), PEACH),
                TreeNodeKind::Index(name) => ("  ", name.clone(), SUBTEXT0),
                TreeNodeKind::ViewsHeader => ("V", "Views".to_owned(), GREEN),
                TreeNodeKind::View(name) => ("  ", name.clone(), SUBTEXT0),
                TreeNodeKind::TriggersHeader => ("!", "Triggers".to_owned(), RED),
                TreeNodeKind::Trigger(name) => ("  ", name.clone(), SUBTEXT0),
            };

            // Highlight selected
            if is_selected {
                cmds.push(RenderCommand::FillRect {
                    x: x + 4.0,
                    y: ny - 2.0,
                    width: width - 8.0,
                    height: 20.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(3.0),
                });
            }

            // Header styling
            let is_header = matches!(
                node.kind,
                TreeNodeKind::TablesHeader
                    | TreeNodeKind::IndexesHeader
                    | TreeNodeKind::ViewsHeader
                    | TreeNodeKind::TriggersHeader
            );

            let font_weight = if is_header { FontWeightHint::Bold } else { FontWeightHint::Regular };
            let font_size = if is_header { 10.0 } else { 11.0 };

            // Icon
            cmds.push(RenderCommand::Text {
                x: x + 10.0 + indent,
                y: ny,
                text: icon.to_owned(),
                color,
                font_size,
                font_weight,
                max_width: Some(16.0),
            });

            // Label
            cmds.push(RenderCommand::Text {
                x: x + 26.0 + indent,
                y: ny,
                text: label,
                color,
                font_size,
                font_weight,
                max_width: Some(width - 36.0 - indent),
            });

            ny += 22.0;
        }

        // Filter builder section
        if self.show_filter_builder {
            ny += 8.0;
            cmds.push(RenderCommand::Line {
                x1: x + 8.0,
                y1: ny,
                x2: x + width - 8.0,
                y2: ny,
                color: SURFACE0,
                width: 1.0,
            });
            ny += 8.0;

            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: ny,
                text: "FILTER BUILDER".to_owned(),
                color: YELLOW,
                font_size: 10.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 20.0),
            });
            ny += 18.0;

            // Show active filters
            if let Some(t) = self.active_db_tab() {
                for (fi, filter) in t.filters.iter().enumerate() {
                    let col_name = t.db.find_table(t.selected_table.as_deref().unwrap_or(""))
                        .and_then(|tbl| tbl.columns.get(filter.column_idx))
                        .map_or("?", |c| c.name.as_str());

                    cmds.push(RenderCommand::FillRect {
                        x: x + 8.0,
                        y: ny - 2.0,
                        width: width - 16.0,
                        height: 18.0,
                        color: SURFACE0,
                        corner_radii: CornerRadii::all(3.0),
                    });
                    cmds.push(RenderCommand::Text {
                        x: x + 12.0,
                        y: ny,
                        text: format!("{col_name} {} {}", filter.op.label(), filter.value_str),
                        color: TEAL,
                        font_size: 10.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(width - 40.0),
                    });
                    // Remove button
                    cmds.push(RenderCommand::Text {
                        x: x + width - 20.0,
                        y: ny,
                        text: "x".to_owned(),
                        color: RED,
                        font_size: 10.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(10.0),
                    });
                    let _ = fi; // suppress unused
                    ny += 20.0;
                }
            }
        }
    }

    fn render_data_grid(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        // Grid background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        let tab = match self.active_db_tab() {
            Some(t) => t,
            None => return,
        };

        let (col_names, all_rows) = match tab.current_table_data() {
            Some(data) => data,
            None => {
                cmds.push(RenderCommand::Text {
                    x: x + 20.0,
                    y: y + 30.0,
                    text: "No table selected".to_owned(),
                    color: OVERLAY0,
                    font_size: 13.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 40.0),
                });
                return;
            }
        };

        let total_rows = all_rows.len();
        let start = tab.page.saturating_mul(PAGE_SIZE);
        let end = (start.saturating_add(PAGE_SIZE)).min(total_rows);
        let page_rows = if start < all_rows.len() {
            &all_rows[start..end]
        } else {
            &[]
        };

        let col_count = col_names.len();
        let col_width = if col_count > 0 {
            (width / col_count as f32).max(DEFAULT_COL_WIDTH).min(width)
        } else {
            DEFAULT_COL_WIDTH
        };

        // Clip to grid area
        cmds.push(RenderCommand::PushClip {
            x,
            y,
            width,
            height,
        });

        // Header row
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: col_width * col_count as f32,
            height: HEADER_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        for (ci, col_name) in col_names.iter().enumerate() {
            let cx = x + ci as f32 * col_width;

            // Sort indicator
            let sort_indicator = tab.sort_state.as_ref().and_then(|s| {
                if s.column_idx == ci {
                    Some(match s.direction {
                        SortDir::Ascending => " ^",
                        SortDir::Descending => " v",
                    })
                } else {
                    None
                }
            });

            let header_text = format!("{col_name}{}", sort_indicator.unwrap_or(""));

            cmds.push(RenderCommand::Text {
                x: cx + CELL_PADDING,
                y: y + 7.0,
                text: header_text,
                color: LAVENDER,
                font_size: 11.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(col_width - CELL_PADDING * 2.0),
            });

            // Column separator
            if ci > 0 {
                cmds.push(RenderCommand::Line {
                    x1: cx,
                    y1: y,
                    x2: cx,
                    y2: y + height,
                    color: SURFACE1,
                    width: 1.0,
                });
            }
        }

        // Header bottom border
        cmds.push(RenderCommand::Line {
            x1: x,
            y1: y + HEADER_HEIGHT,
            x2: x + col_width * col_count as f32,
            y2: y + HEADER_HEIGHT,
            color: SURFACE1,
            width: 1.0,
        });

        // Data rows
        let mut ry = y + HEADER_HEIGHT;
        for (ri, row) in page_rows.iter().enumerate() {
            if ry > y + height {
                break;
            }

            // Alternating row colors
            let row_bg = if ri % 2 == 0 { BASE } else { SURFACE0 };
            cmds.push(RenderCommand::FillRect {
                x,
                y: ry,
                width: col_width * col_count as f32,
                height: ROW_HEIGHT,
                color: row_bg,
                corner_radii: CornerRadii::ZERO,
            });

            for (ci, cell) in row.iter().enumerate() {
                let cx = x + ci as f32 * col_width;
                let display = cell.display();
                let color = match cell {
                    CellValue::Null => OVERLAY0,
                    CellValue::Integer(_) => BLUE,
                    CellValue::Real(_) => PEACH,
                    CellValue::Text(_) => TEXT,
                    CellValue::Blob(_) => MAUVE,
                };

                cmds.push(RenderCommand::Text {
                    x: cx + CELL_PADDING,
                    y: ry + 6.0,
                    text: display,
                    color,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(col_width - CELL_PADDING * 2.0),
                });
            }

            // Row separator
            cmds.push(RenderCommand::Line {
                x1: x,
                y1: ry + ROW_HEIGHT,
                x2: x + col_width * col_count as f32,
                y2: ry + ROW_HEIGHT,
                color: SURFACE0,
                width: 1.0,
            });

            ry += ROW_HEIGHT;
        }

        cmds.push(RenderCommand::PopClip);

        // Pagination bar
        let page_bar_y = y + height - 22.0;
        cmds.push(RenderCommand::FillRect {
            x,
            y: page_bar_y,
            width,
            height: 22.0,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        let total_pages = if total_rows == 0 { 1 } else { total_rows.saturating_sub(1) / PAGE_SIZE + 1 };
        let page_text = format!(
            "Page {} of {} ({} rows)",
            tab.page.saturating_add(1),
            total_pages,
            total_rows
        );
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: page_bar_y + 5.0,
            text: page_text,
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 200.0),
        });

        // Prev / Next buttons
        let nav_x = x + width - 120.0;
        for (bi, label) in ["< Prev", "Next >"].iter().enumerate() {
            let bx = nav_x + bi as f32 * 60.0;
            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: page_bar_y + 2.0,
                width: 54.0,
                height: 18.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 6.0,
                y: page_bar_y + 5.0,
                text: (*label).to_owned(),
                color: SUBTEXT1,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(48.0),
            });
        }
    }

    fn render_bottom_panels(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        // Panel background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Top border
        cmds.push(RenderCommand::Line {
            x1: x,
            y1: y,
            x2: x + width,
            y2: y,
            color: SURFACE1,
            width: 1.0,
        });

        // Panel tabs
        let mut tx = x + 4.0;
        for panel in BottomPanel::all() {
            let is_active = *panel == self.bottom_panel;
            let label = panel.label();
            let tw = label.len() as f32 * 7.0 + 16.0;

            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: y + 2.0,
                width: tw,
                height: 22.0,
                color: if is_active { SURFACE0 } else { MANTLE },
                corner_radii: CornerRadii {
                    top_left: 3.0,
                    top_right: 3.0,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
            });
            cmds.push(RenderCommand::Text {
                x: tx + 8.0,
                y: y + 6.0,
                text: label.to_owned(),
                color: if is_active { TEXT } else { SUBTEXT0 },
                font_size: 10.0,
                font_weight: if is_active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(tw - 16.0),
            });
            tx += tw + 2.0;
        }

        let content_y = y + 26.0;
        let content_height = height - 26.0;

        // Clip panel content
        cmds.push(RenderCommand::PushClip {
            x,
            y: content_y,
            width,
            height: content_height,
        });

        match self.bottom_panel {
            BottomPanel::SqlEditor => self.render_sql_editor(cmds, x, content_y, width, content_height),
            BottomPanel::Results => self.render_results(cmds, x, content_y, width, content_height),
            BottomPanel::Schema => self.render_schema(cmds, x, content_y, width, content_height),
            BottomPanel::Diagram => self.render_diagram(cmds, x, content_y, width, content_height),
        }

        cmds.push(RenderCommand::PopClip);
    }

    fn render_sql_editor(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32, _height: f32) {
        // Editor background
        cmds.push(RenderCommand::FillRect {
            x: x + 8.0,
            y: y + 4.0,
            width: width - 16.0,
            height: 80.0,
            color: CRUST,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: x + 8.0,
            y: y + 4.0,
            width: width - 16.0,
            height: 80.0,
            color: SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // SQL text with keyword highlighting (simplified: render full text, then overlay)
        let sql = &self.sql_input;
        if sql.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: y + 12.0,
                text: "Enter SQL query...".to_owned(),
                color: OVERLAY0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 40.0),
            });
        } else {
            // Render tokens with syntax coloring
            let tokens = tokenize_sql(sql);
            let mut tx_pos = x + 16.0;
            let ty = y + 12.0;
            let max_w = width - 40.0;

            for token in &tokens {
                let (text, color, weight) = match token {
                    SqlToken::Keyword(k) => (k.clone(), MAUVE, FontWeightHint::Bold),
                    SqlToken::Identifier(id) => (id.clone(), TEXT, FontWeightHint::Regular),
                    SqlToken::StringLiteral(s) => (format!("'{s}'"), GREEN, FontWeightHint::Regular),
                    SqlToken::NumberLiteral(n) => (n.clone(), PEACH, FontWeightHint::Regular),
                    SqlToken::Operator(op) => (op.clone(), RED, FontWeightHint::Regular),
                    SqlToken::Comma => (",".to_owned(), TEXT, FontWeightHint::Regular),
                    SqlToken::Semicolon => (";".to_owned(), TEXT, FontWeightHint::Regular),
                    SqlToken::LeftParen => ("(".to_owned(), YELLOW, FontWeightHint::Regular),
                    SqlToken::RightParen => (")".to_owned(), YELLOW, FontWeightHint::Regular),
                    SqlToken::Star => ("*".to_owned(), PEACH, FontWeightHint::Bold),
                    SqlToken::Dot => (".".to_owned(), TEXT, FontWeightHint::Regular),
                    SqlToken::Whitespace => (" ".to_owned(), TEXT, FontWeightHint::Regular),
                };

                let char_w = 7.2;
                let text_w = text.len() as f32 * char_w;

                if tx_pos + text_w < x + max_w {
                    cmds.push(RenderCommand::Text {
                        x: tx_pos,
                        y: ty,
                        text,
                        color,
                        font_size: 12.0,
                        font_weight: weight,
                        max_width: Some(text_w + 4.0),
                    });
                }
                tx_pos += text_w;
            }
        }

        // History section
        let history_y = y + 92.0;
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: history_y,
            text: format!("HISTORY ({} queries)", self.history.len()),
            color: OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 24.0),
        });

        let mut hy = history_y + 16.0;
        for entry in self.history.iter().rev().take(5) {
            let star = if entry.favorite { "[*] " } else { "" };
            let status_color = if entry.success { GREEN } else { RED };

            cmds.push(RenderCommand::FillRect {
                x: x + 8.0,
                y: hy - 1.0,
                width: width - 16.0,
                height: 16.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(2.0),
            });

            // Status dot
            cmds.push(RenderCommand::FillRect {
                x: x + 12.0,
                y: hy + 4.0,
                width: 6.0,
                height: 6.0,
                color: status_color,
                corner_radii: CornerRadii::all(3.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 22.0,
                y: hy + 1.0,
                text: format!("{star}{}", truncate_str(&entry.sql, 80)),
                color: SUBTEXT0,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 40.0),
            });
            hy += 18.0;
        }
    }

    fn render_results(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32, height: f32) {
        match &self.query_result {
            None => {
                cmds.push(RenderCommand::Text {
                    x: x + 16.0,
                    y: y + 8.0,
                    text: "No query results. Execute a query first.".to_owned(),
                    color: OVERLAY0,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 32.0),
                });
            }
            Some(result) => {
                // Message
                let msg_color = if result.is_error { RED } else { GREEN };
                cmds.push(RenderCommand::Text {
                    x: x + 12.0,
                    y: y + 4.0,
                    text: result.message.clone(),
                    color: msg_color,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(width - 24.0),
                });

                // Result table
                if !result.columns.is_empty() {
                    let col_count = result.columns.len();
                    let col_w = (width / col_count as f32).max(100.0).min(width);

                    // Column headers
                    let header_y = y + 22.0;
                    cmds.push(RenderCommand::FillRect {
                        x,
                        y: header_y,
                        width,
                        height: 20.0,
                        color: SURFACE0,
                        corner_radii: CornerRadii::ZERO,
                    });

                    for (ci, col_name) in result.columns.iter().enumerate() {
                        cmds.push(RenderCommand::Text {
                            x: x + ci as f32 * col_w + 6.0,
                            y: header_y + 4.0,
                            text: col_name.clone(),
                            color: LAVENDER,
                            font_size: 10.0,
                            font_weight: FontWeightHint::Bold,
                            max_width: Some(col_w - 12.0),
                        });
                    }

                    // Data rows
                    let mut ry = header_y + 22.0;
                    for row in result.rows.iter().take(20) {
                        if ry > y + height {
                            break;
                        }
                        for (ci, cell) in row.iter().enumerate() {
                            let color = match cell {
                                CellValue::Null => OVERLAY0,
                                CellValue::Integer(_) => BLUE,
                                CellValue::Real(_) => PEACH,
                                CellValue::Text(_) => TEXT,
                                CellValue::Blob(_) => MAUVE,
                            };
                            cmds.push(RenderCommand::Text {
                                x: x + ci as f32 * col_w + 6.0,
                                y: ry,
                                text: truncate_str(&cell.display(), 30).to_owned(),
                                color,
                                font_size: 10.0,
                                font_weight: FontWeightHint::Regular,
                                max_width: Some(col_w - 12.0),
                            });
                        }
                        ry += 16.0;
                    }
                }
            }
        }
    }

    fn render_schema(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32, height: f32) {
        let tab = match self.active_db_tab() {
            Some(t) => t,
            None => return,
        };

        let table_name = match &tab.selected_table {
            Some(n) => n,
            None => {
                cmds.push(RenderCommand::Text {
                    x: x + 16.0,
                    y: y + 8.0,
                    text: "Select a table to view its schema.".to_owned(),
                    color: OVERLAY0,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 32.0),
                });
                return;
            }
        };

        let table = match tab.db.find_table(table_name) {
            Some(t) => t,
            None => return,
        };

        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: y + 4.0,
            text: format!("SCHEMA: {table_name}"),
            color: BLUE,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 24.0),
        });

        // Column headers
        let headers = ["Column", "Type", "Constraints"];
        let col_widths = [180.0, 100.0, 200.0];
        let mut cy = y + 24.0;

        cmds.push(RenderCommand::FillRect {
            x: x + 8.0,
            y: cy,
            width: width - 16.0,
            height: 18.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(2.0),
        });

        let mut hx = x + 12.0;
        for (hi, header) in headers.iter().enumerate() {
            cmds.push(RenderCommand::Text {
                x: hx,
                y: cy + 3.0,
                text: (*header).to_owned(),
                color: LAVENDER,
                font_size: 10.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(col_widths.get(hi).copied().unwrap_or(100.0)),
            });
            hx += col_widths.get(hi).copied().unwrap_or(100.0);
        }

        cy += 22.0;

        for col in &table.columns {
            if cy > y + height {
                break;
            }

            let type_color = col.data_type.color();
            let constraints = col.constraints.describe();

            let mut rx = x + 12.0;

            cmds.push(RenderCommand::Text {
                x: rx,
                y: cy,
                text: col.name.clone(),
                color: TEXT,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(180.0),
            });
            rx += 180.0;

            cmds.push(RenderCommand::Text {
                x: rx,
                y: cy,
                text: col.data_type.label().to_owned(),
                color: type_color,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
            });
            rx += 100.0;

            if !constraints.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: rx,
                    y: cy,
                    text: constraints,
                    color: YELLOW,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(200.0),
                });
            }

            cy += 18.0;
        }

        // Foreign keys
        let fks: Vec<&ForeignKey> = tab.db.foreign_keys.iter()
            .filter(|fk| fk.from_table.to_uppercase() == table_name.to_uppercase())
            .collect();

        if !fks.is_empty() {
            cy += 8.0;
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: cy,
                text: "FOREIGN KEYS".to_owned(),
                color: PEACH,
                font_size: 10.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 24.0),
            });
            cy += 16.0;

            for fk in fks {
                cmds.push(RenderCommand::Text {
                    x: x + 16.0,
                    y: cy,
                    text: format!(
                        "{}.{} -> {}.{}",
                        fk.from_table, fk.from_column, fk.to_table, fk.to_column
                    ),
                    color: TEAL,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 32.0),
                });
                cy += 16.0;
            }
        }
    }

    fn render_diagram(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32, _height: f32) {
        let tab = match self.active_db_tab() {
            Some(t) => t,
            None => return,
        };

        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: y + 4.0,
            text: "SCHEMA DIAGRAM".to_owned(),
            color: BLUE,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 24.0),
        });

        let table_count = tab.db.tables.len();
        if table_count == 0 {
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: y + 28.0,
                text: "No tables in database.".to_owned(),
                color: OVERLAY0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 32.0),
            });
            return;
        }

        // Simple layout: tables as boxes in a row
        let box_width: f32 = 160.0;
        let box_spacing: f32 = 40.0;
        let start_x = x + 20.0;
        let start_y = y + 28.0;

        let mut table_positions: Vec<(f32, f32, String)> = Vec::new();

        for (ti, table) in tab.db.tables.iter().enumerate() {
            let tx = start_x + ti as f32 * (box_width + box_spacing);
            let ty = start_y;
            let box_height = 22.0 + table.columns.len() as f32 * 14.0;

            table_positions.push((tx, ty, table.name.clone()));

            // Table box
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: ty,
                width: box_width,
                height: box_height,
                color: SURFACE0,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
            cmds.push(RenderCommand::StrokeRect {
                x: tx,
                y: ty,
                width: box_width,
                height: box_height,
                color: BLUE,
                line_width: 1.0,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });

            // Table name header
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: ty,
                width: box_width,
                height: 20.0,
                color: BLUE,
                corner_radii: CornerRadii {
                    top_left: CORNER_RADIUS,
                    top_right: CORNER_RADIUS,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
            });
            cmds.push(RenderCommand::Text {
                x: tx + 6.0,
                y: ty + 4.0,
                text: table.name.clone(),
                color: CRUST,
                font_size: 10.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(box_width - 12.0),
            });

            // Columns
            let mut cy = ty + 24.0;
            for col in &table.columns {
                let pk_marker = if col.constraints.primary_key { "PK " } else { "" };
                cmds.push(RenderCommand::Text {
                    x: tx + 6.0,
                    y: cy,
                    text: format!("{pk_marker}{}: {}", col.name, col.data_type.label()),
                    color: SUBTEXT1,
                    font_size: 9.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(box_width - 12.0),
                });
                cy += 14.0;
            }
        }

        // Draw FK relationship lines
        for fk in &tab.db.foreign_keys {
            let from_pos = table_positions.iter().find(|(_, _, n)| n.to_uppercase() == fk.from_table.to_uppercase());
            let to_pos = table_positions.iter().find(|(_, _, n)| n.to_uppercase() == fk.to_table.to_uppercase());

            if let (Some((fx, fy, _)), Some((tox, toy, _))) = (from_pos, to_pos) {
                // Draw a line from the right side of from_table to the left side of to_table
                let from_x = fx + box_width;
                let from_y = fy + 10.0;
                let to_x = *tox;
                let to_y = toy + 10.0;

                cmds.push(RenderCommand::Line {
                    x1: from_x,
                    y1: from_y,
                    x2: to_x,
                    y2: to_y,
                    color: PEACH,
                    width: 1.5,
                });

                // FK label
                let mid_x = (from_x + to_x) / 2.0;
                let mid_y = (from_y + to_y) / 2.0 - 8.0;
                cmds.push(RenderCommand::Text {
                    x: mid_x,
                    y: mid_y,
                    text: format!("{} -> {}", fk.from_column, fk.to_column),
                    color: PEACH,
                    font_size: 8.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(120.0),
                });
            }
        }
    }

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: STATUS_BAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let tab = self.active_db_tab();

        // Database name
        let db_name = tab.map_or("No database", |t| t.db.name.as_str());
        cmds.push(RenderCommand::Text {
            x: x + 10.0,
            y: y + 5.0,
            text: format!("DB: {db_name}"),
            color: BLUE,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });

        // Table info
        if let Some(t) = tab {
            if let Some(ref table_name) = t.selected_table {
                if let Some(table) = t.db.find_table(table_name) {
                    cmds.push(RenderCommand::Text {
                        x: x + 200.0,
                        y: y + 5.0,
                        text: format!(
                            "Table: {} ({} cols, {} rows)",
                            table_name,
                            table.col_count(),
                            table.row_count()
                        ),
                        color: SUBTEXT0,
                        font_size: 10.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(300.0),
                    });
                }
            }
        }

        // History count
        cmds.push(RenderCommand::Text {
            x: x + width - 150.0,
            y: y + 5.0,
            text: format!("Queries: {}", self.history.len()),
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(140.0),
        });
    }
}

// ============================================================================
// Export format enum
// ============================================================================

/// Supported export formats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExportFormat {
    Csv,
    Json,
    SqlInserts,
}

// ============================================================================
// Helper functions
// ============================================================================

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_owned()
    } else {
        let end = s.char_indices()
            .nth(max_len.saturating_sub(3))
            .map_or(s.len(), |(i, _)| i);
        format!("{}...", &s[..end])
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let app = DbViewerApp::new();
    let cmds = app.render(1200.0, 800.0);
    // In the real OS, these commands would be submitted to the compositor.
    // For now, just verify it produces output.
    assert!(!cmds.is_empty(), "Render should produce commands");
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Data type tests ---

    #[test]
    fn test_data_type_from_str_loose_integer() {
        assert_eq!(DataType::from_str_loose("INTEGER"), DataType::Integer);
        assert_eq!(DataType::from_str_loose("INT"), DataType::Integer);
        assert_eq!(DataType::from_str_loose("BIGINT"), DataType::Integer);
    }

    #[test]
    fn test_data_type_from_str_loose_real() {
        assert_eq!(DataType::from_str_loose("REAL"), DataType::Real);
        assert_eq!(DataType::from_str_loose("FLOAT"), DataType::Real);
        assert_eq!(DataType::from_str_loose("DOUBLE"), DataType::Real);
    }

    #[test]
    fn test_data_type_from_str_loose_text() {
        assert_eq!(DataType::from_str_loose("TEXT"), DataType::Text);
        assert_eq!(DataType::from_str_loose("VARCHAR"), DataType::Text);
        assert_eq!(DataType::from_str_loose("unknown_type"), DataType::Text);
    }

    #[test]
    fn test_data_type_from_str_loose_blob() {
        assert_eq!(DataType::from_str_loose("BLOB"), DataType::Blob);
    }

    #[test]
    fn test_data_type_label() {
        assert_eq!(DataType::Integer.label(), "INTEGER");
        assert_eq!(DataType::Real.label(), "REAL");
        assert_eq!(DataType::Text.label(), "TEXT");
        assert_eq!(DataType::Blob.label(), "BLOB");
        assert_eq!(DataType::Null.label(), "NULL");
    }

    // --- Cell value tests ---

    #[test]
    fn test_cell_value_display_integer() {
        assert_eq!(CellValue::Integer(42).display(), "42");
    }

    #[test]
    fn test_cell_value_display_real() {
        assert_eq!(CellValue::Real(3.14).display(), "3.140000");
    }

    #[test]
    fn test_cell_value_display_text() {
        assert_eq!(CellValue::Text("hello".to_owned()).display(), "hello");
    }

    #[test]
    fn test_cell_value_display_blob() {
        assert_eq!(CellValue::Blob(vec![1, 2, 3]).display(), "<BLOB 3 bytes>");
    }

    #[test]
    fn test_cell_value_display_null() {
        assert_eq!(CellValue::Null.display(), "NULL");
    }

    #[test]
    fn test_cell_value_parse_as_integer() {
        assert_eq!(CellValue::parse_as("42", &DataType::Integer), CellValue::Integer(42));
        assert_eq!(CellValue::parse_as("null", &DataType::Integer), CellValue::Null);
    }

    #[test]
    fn test_cell_value_parse_as_real() {
        assert_eq!(CellValue::parse_as("3.14", &DataType::Real), CellValue::Real(3.14));
    }

    #[test]
    fn test_cell_value_parse_as_text() {
        assert_eq!(CellValue::parse_as("hello", &DataType::Text), CellValue::Text("hello".to_owned()));
    }

    // --- Sort key tests ---

    #[test]
    fn test_sort_key_null_ordering() {
        assert!(CellValue::Null.as_sort_key() < CellValue::Integer(0).as_sort_key());
    }

    #[test]
    fn test_sort_key_integer_ordering() {
        assert!(CellValue::Integer(1).as_sort_key() < CellValue::Integer(2).as_sort_key());
    }

    #[test]
    fn test_sort_key_text_ordering() {
        assert!(CellValue::Text("a".to_owned()).as_sort_key() < CellValue::Text("b".to_owned()).as_sort_key());
    }

    // --- Table tests ---

    #[test]
    fn test_table_insert_row() {
        let mut table = Table::new("test", vec![
            ColumnDef::new("id", DataType::Integer),
            ColumnDef::new("name", DataType::Text),
        ]);
        let result = table.insert_row(vec![CellValue::Integer(1), CellValue::Text("Alice".to_owned())]);
        assert!(result.is_ok());
        assert_eq!(table.row_count(), 1);
    }

    #[test]
    fn test_table_insert_row_column_mismatch() {
        let mut table = Table::new("test", vec![ColumnDef::new("id", DataType::Integer)]);
        let result = table.insert_row(vec![CellValue::Integer(1), CellValue::Text("extra".to_owned())]);
        assert!(result.is_err());
    }

    #[test]
    fn test_table_not_null_constraint() {
        let mut table = Table::new("test", vec![
            ColumnDef::new("name", DataType::Text).with_not_null(),
        ]);
        let result = table.insert_row(vec![CellValue::Null]);
        assert!(result.is_err());
    }

    #[test]
    fn test_table_unique_constraint() {
        let mut table = Table::new("test", vec![
            ColumnDef::new("email", DataType::Text).with_unique(),
        ]);
        assert!(table.insert_row(vec![CellValue::Text("a@b.com".to_owned())]).is_ok());
        assert!(table.insert_row(vec![CellValue::Text("a@b.com".to_owned())]).is_err());
    }

    #[test]
    fn test_table_auto_increment() {
        let mut table = Table::new("test", vec![
            ColumnDef::new("id", DataType::Integer).with_primary_key().with_auto_increment(),
            ColumnDef::new("name", DataType::Text),
        ]);
        let _ = table.insert_row(vec![CellValue::Null, CellValue::Text("A".to_owned())]);
        let _ = table.insert_row(vec![CellValue::Null, CellValue::Text("B".to_owned())]);
        assert_eq!(table.rows.get(0).and_then(|r| r.first()), Some(&CellValue::Integer(1)));
        assert_eq!(table.rows.get(1).and_then(|r| r.first()), Some(&CellValue::Integer(2)));
    }

    #[test]
    fn test_table_column_index() {
        let table = Table::new("test", vec![
            ColumnDef::new("id", DataType::Integer),
            ColumnDef::new("name", DataType::Text),
        ]);
        assert_eq!(table.column_index("id"), Some(0));
        assert_eq!(table.column_index("NAME"), Some(1)); // case insensitive
        assert_eq!(table.column_index("missing"), None);
    }

    #[test]
    fn test_table_delete_where() {
        let mut table = Table::new("test", vec![ColumnDef::new("v", DataType::Integer)]);
        let _ = table.insert_row(vec![CellValue::Integer(1)]);
        let _ = table.insert_row(vec![CellValue::Integer(2)]);
        let _ = table.insert_row(vec![CellValue::Integer(3)]);
        let deleted = table.delete_where(0, &FilterOp::Equal, &CellValue::Integer(2));
        assert_eq!(deleted, 1);
        assert_eq!(table.row_count(), 2);
    }

    #[test]
    fn test_table_update_where() {
        let mut table = Table::new("test", vec![ColumnDef::new("v", DataType::Integer)]);
        let _ = table.insert_row(vec![CellValue::Integer(1)]);
        let _ = table.insert_row(vec![CellValue::Integer(2)]);
        let updated = table.update_where(0, &CellValue::Integer(99), 0, &FilterOp::Equal, &CellValue::Integer(1));
        assert_eq!(updated, 1);
        assert_eq!(table.rows.get(0).and_then(|r| r.first()), Some(&CellValue::Integer(99)));
    }

    // --- Database tests ---

    #[test]
    fn test_database_create_table() {
        let mut db = Database::new("test.db");
        let table = Table::new("t1", vec![ColumnDef::new("id", DataType::Integer)]);
        assert!(db.create_table(table).is_ok());
        assert_eq!(db.table_names().len(), 1);
    }

    #[test]
    fn test_database_create_duplicate_table() {
        let mut db = Database::new("test.db");
        let _ = db.create_table(Table::new("t1", vec![]));
        assert!(db.create_table(Table::new("t1", vec![])).is_err());
    }

    #[test]
    fn test_database_drop_table() {
        let mut db = Database::new("test.db");
        let _ = db.create_table(Table::new("t1", vec![]));
        assert!(db.drop_table("t1").is_ok());
        assert!(db.table_names().is_empty());
    }

    #[test]
    fn test_database_drop_nonexistent() {
        let mut db = Database::new("test.db");
        assert!(db.drop_table("missing").is_err());
    }

    #[test]
    fn test_database_sample() {
        let db = Database::sample();
        assert_eq!(db.tables.len(), 3);
        assert!(db.find_table("users").is_some());
        assert!(db.find_table("products").is_some());
        assert!(db.find_table("orders").is_some());
        assert!(!db.foreign_keys.is_empty());
    }

    // --- Filter tests ---

    #[test]
    fn test_filter_equal() {
        assert!(matches_filter(&CellValue::Integer(5), &FilterOp::Equal, &CellValue::Integer(5)));
        assert!(!matches_filter(&CellValue::Integer(5), &FilterOp::Equal, &CellValue::Integer(6)));
    }

    #[test]
    fn test_filter_not_equal() {
        assert!(matches_filter(&CellValue::Integer(5), &FilterOp::NotEqual, &CellValue::Integer(6)));
    }

    #[test]
    fn test_filter_less_than() {
        assert!(matches_filter(&CellValue::Integer(3), &FilterOp::LessThan, &CellValue::Integer(5)));
        assert!(!matches_filter(&CellValue::Integer(5), &FilterOp::LessThan, &CellValue::Integer(3)));
    }

    #[test]
    fn test_filter_greater_than() {
        assert!(matches_filter(&CellValue::Integer(5), &FilterOp::GreaterThan, &CellValue::Integer(3)));
    }

    #[test]
    fn test_filter_is_null() {
        assert!(matches_filter(&CellValue::Null, &FilterOp::IsNull, &CellValue::Null));
        assert!(!matches_filter(&CellValue::Integer(1), &FilterOp::IsNull, &CellValue::Null));
    }

    #[test]
    fn test_filter_is_not_null() {
        assert!(matches_filter(&CellValue::Integer(1), &FilterOp::IsNotNull, &CellValue::Null));
        assert!(!matches_filter(&CellValue::Null, &FilterOp::IsNotNull, &CellValue::Null));
    }

    #[test]
    fn test_filter_like() {
        let cell = CellValue::Text("Hello World".to_owned());
        assert!(matches_filter(&cell, &FilterOp::Like, &CellValue::Text("%world".to_owned())));
        assert!(matches_filter(&cell, &FilterOp::Like, &CellValue::Text("hello%".to_owned())));
        assert!(matches_filter(&cell, &FilterOp::Like, &CellValue::Text("%lo w%".to_owned())));
        assert!(!matches_filter(&cell, &FilterOp::Like, &CellValue::Text("xyz%".to_owned())));
    }

    #[test]
    fn test_like_underscore_wildcard() {
        assert!(simple_like_match("abc", "a_c"));
        assert!(!simple_like_match("ac", "a_c"));
    }

    // --- SQL Tokenizer tests ---

    #[test]
    fn test_tokenize_select() {
        let tokens = tokenize_sql("SELECT * FROM users");
        let non_ws: Vec<_> = tokens.into_iter().filter(|t| *t != SqlToken::Whitespace).collect();
        assert_eq!(non_ws.len(), 4);
        assert_eq!(non_ws[0], SqlToken::Keyword("SELECT".to_owned()));
        assert_eq!(non_ws[1], SqlToken::Star);
        assert_eq!(non_ws[2], SqlToken::Keyword("FROM".to_owned()));
        assert_eq!(non_ws[3], SqlToken::Identifier("users".to_owned()));
    }

    #[test]
    fn test_tokenize_string_literal() {
        let tokens = tokenize_sql("'hello world'");
        let non_ws: Vec<_> = tokens.into_iter().filter(|t| *t != SqlToken::Whitespace).collect();
        assert_eq!(non_ws.len(), 1);
        assert_eq!(non_ws[0], SqlToken::StringLiteral("hello world".to_owned()));
    }

    #[test]
    fn test_tokenize_number() {
        let tokens = tokenize_sql("42 3.14");
        let non_ws: Vec<_> = tokens.into_iter().filter(|t| *t != SqlToken::Whitespace).collect();
        assert_eq!(non_ws.len(), 2);
        assert_eq!(non_ws[0], SqlToken::NumberLiteral("42".to_owned()));
        assert_eq!(non_ws[1], SqlToken::NumberLiteral("3.14".to_owned()));
    }

    #[test]
    fn test_tokenize_operators() {
        let tokens = tokenize_sql("= != < > <= >=");
        let non_ws: Vec<_> = tokens.into_iter().filter(|t| *t != SqlToken::Whitespace).collect();
        assert_eq!(non_ws.len(), 6);
    }

    // --- SQL Parser tests ---

    #[test]
    fn test_parse_select_all() {
        let stmt = parse_sql("SELECT * FROM users").unwrap();
        match stmt {
            SqlStatement::Select { columns, table, .. } => {
                assert_eq!(table, "users");
                assert!(matches!(columns[0], SelectColumn::AllColumns));
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_columns() {
        let stmt = parse_sql("SELECT name, age FROM users").unwrap();
        match stmt {
            SqlStatement::Select { columns, .. } => {
                assert_eq!(columns.len(), 2);
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_where() {
        let stmt = parse_sql("SELECT * FROM users WHERE age > 30").unwrap();
        match stmt {
            SqlStatement::Select { where_clause, .. } => {
                let wc = where_clause.unwrap();
                assert_eq!(wc.column, "age");
                assert_eq!(wc.op, FilterOp::GreaterThan);
                assert_eq!(wc.value, "30");
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_order_by() {
        let stmt = parse_sql("SELECT * FROM users ORDER BY name DESC").unwrap();
        match stmt {
            SqlStatement::Select { order_by, .. } => {
                let (col, dir) = order_by.unwrap();
                assert_eq!(col, "name");
                assert_eq!(dir, SortDir::Descending);
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_limit() {
        let stmt = parse_sql("SELECT * FROM users LIMIT 10").unwrap();
        match stmt {
            SqlStatement::Select { limit, .. } => {
                assert_eq!(limit, Some(10));
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_aggregate() {
        let stmt = parse_sql("SELECT COUNT(*) FROM users").unwrap();
        match stmt {
            SqlStatement::Select { columns, .. } => {
                assert!(matches!(columns[0], SelectColumn::Aggregate { func: AggFunc::Count, .. }));
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_insert() {
        let stmt = parse_sql("INSERT INTO users (name) VALUES ('Alice')").unwrap();
        match stmt {
            SqlStatement::Insert { table, columns, values } => {
                assert_eq!(table, "users");
                assert_eq!(columns, vec!["name"]);
                assert_eq!(values.len(), 1);
                assert_eq!(values[0][0], "Alice");
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_update() {
        let stmt = parse_sql("UPDATE users SET name = 'Bob' WHERE id = 1").unwrap();
        match stmt {
            SqlStatement::Update { table, set_clauses, where_clause } => {
                assert_eq!(table, "users");
                assert_eq!(set_clauses.len(), 1);
                assert!(where_clause.is_some());
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_delete() {
        let stmt = parse_sql("DELETE FROM users WHERE id = 1").unwrap();
        match stmt {
            SqlStatement::Delete { table, where_clause } => {
                assert_eq!(table, "users");
                assert!(where_clause.is_some());
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_create_table() {
        let stmt = parse_sql("CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT NOT NULL)").unwrap();
        match stmt {
            SqlStatement::CreateTable { name, columns, if_not_exists } => {
                assert_eq!(name, "test");
                assert_eq!(columns.len(), 2);
                assert!(columns[0].primary_key);
                assert!(columns[1].not_null);
                assert!(!if_not_exists);
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_drop_table() {
        let stmt = parse_sql("DROP TABLE IF EXISTS test").unwrap();
        match stmt {
            SqlStatement::DropTable { name, if_exists } => {
                assert_eq!(name, "test");
                assert!(if_exists);
            }
            _ => panic!("Expected DROP TABLE"),
        }
    }

    #[test]
    fn test_parse_empty_query() {
        assert!(parse_sql("").is_err());
    }

    // --- SQL Execution tests ---

    #[test]
    fn test_execute_select_all() {
        let mut db = Database::sample();
        let stmt = parse_sql("SELECT * FROM users").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        assert_eq!(result.columns.len(), 5);
        assert_eq!(result.rows.len(), 10);
    }

    #[test]
    fn test_execute_select_with_where() {
        let mut db = Database::sample();
        let stmt = parse_sql("SELECT * FROM users WHERE age > 30").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        assert!(result.rows.len() < 10);
        // All returned rows should have age > 30
        for row in &result.rows {
            if let Some(CellValue::Integer(age)) = row.get(3) {
                assert!(*age > 30);
            }
        }
    }

    #[test]
    fn test_execute_select_order_by() {
        let mut db = Database::sample();
        let stmt = parse_sql("SELECT * FROM users ORDER BY age ASC").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        let ages: Vec<i64> = result.rows.iter().filter_map(|r| {
            if let Some(CellValue::Integer(v)) = r.get(3) { Some(*v) } else { None }
        }).collect();
        for w in ages.windows(2) {
            assert!(w[0] <= w[1], "Should be sorted ascending");
        }
    }

    #[test]
    fn test_execute_select_limit() {
        let mut db = Database::sample();
        let stmt = parse_sql("SELECT * FROM users LIMIT 3").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert_eq!(result.rows.len(), 3);
    }

    #[test]
    fn test_execute_count() {
        let mut db = Database::sample();
        let stmt = parse_sql("SELECT COUNT(*) FROM users").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], CellValue::Integer(10));
    }

    #[test]
    fn test_execute_sum() {
        let mut db = Database::sample();
        let stmt = parse_sql("SELECT SUM(age) FROM users").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        if let CellValue::Real(sum) = &result.rows[0][0] {
            assert!(*sum > 0.0);
        }
    }

    #[test]
    fn test_execute_avg() {
        let mut db = Database::sample();
        let stmt = parse_sql("SELECT AVG(score) FROM users").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
    }

    #[test]
    fn test_execute_min_max() {
        let mut db = Database::sample();
        let min_stmt = parse_sql("SELECT MIN(age) FROM users").unwrap();
        let min_result = execute_sql(&mut db, &min_stmt);
        let max_stmt = parse_sql("SELECT MAX(age) FROM users").unwrap();
        let max_result = execute_sql(&mut db, &max_stmt);
        assert!(!min_result.is_error);
        assert!(!max_result.is_error);
    }

    #[test]
    fn test_execute_insert() {
        let mut db = Database::sample();
        let stmt = parse_sql("INSERT INTO users (name, email, age) VALUES ('Zoe', 'zoe@example.com', 26)").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        assert_eq!(result.affected_rows, 1);
        assert_eq!(db.find_table("users").unwrap().row_count(), 11);
    }

    #[test]
    fn test_execute_update() {
        let mut db = Database::sample();
        let stmt = parse_sql("UPDATE users SET age = 99 WHERE name = 'Alice'").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        assert_eq!(result.affected_rows, 1);
    }

    #[test]
    fn test_execute_delete() {
        let mut db = Database::sample();
        let stmt = parse_sql("DELETE FROM users WHERE name = 'Alice'").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        assert_eq!(result.affected_rows, 1);
        assert_eq!(db.find_table("users").unwrap().row_count(), 9);
    }

    #[test]
    fn test_execute_create_table() {
        let mut db = Database::new("test.db");
        let stmt = parse_sql("CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        assert!(db.find_table("test").is_some());
    }

    #[test]
    fn test_execute_drop_table() {
        let mut db = Database::sample();
        let stmt = parse_sql("DROP TABLE users").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        assert!(db.find_table("users").is_none());
    }

    #[test]
    fn test_execute_select_nonexistent_table() {
        let mut db = Database::sample();
        let stmt = parse_sql("SELECT * FROM nonexistent").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(result.is_error);
    }

    #[test]
    fn test_execute_group_by() {
        let mut db = Database::sample();
        let stmt = parse_sql("SELECT category, COUNT(*) FROM products GROUP BY category").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        assert!(result.rows.len() >= 2); // At least Electronics and Furniture
    }

    // --- Export tests ---

    #[test]
    fn test_export_csv() {
        let mut table = Table::new("test", vec![
            ColumnDef::new("id", DataType::Integer),
            ColumnDef::new("name", DataType::Text),
        ]);
        let _ = table.insert_row(vec![CellValue::Integer(1), CellValue::Text("Alice".to_owned())]);
        let csv = export_csv(&table);
        assert!(csv.contains("id,name"));
        assert!(csv.contains("1,\"Alice\""));
    }

    #[test]
    fn test_export_json() {
        let mut table = Table::new("test", vec![
            ColumnDef::new("id", DataType::Integer),
            ColumnDef::new("name", DataType::Text),
        ]);
        let _ = table.insert_row(vec![CellValue::Integer(1), CellValue::Text("Alice".to_owned())]);
        let json = export_json(&table);
        assert!(json.contains("\"id\": 1"));
        assert!(json.contains("\"name\": \"Alice\""));
    }

    #[test]
    fn test_export_sql_inserts() {
        let mut table = Table::new("test", vec![
            ColumnDef::new("id", DataType::Integer),
            ColumnDef::new("name", DataType::Text),
        ]);
        let _ = table.insert_row(vec![CellValue::Integer(1), CellValue::Text("Alice".to_owned())]);
        let sql = export_sql_inserts(&table);
        assert!(sql.contains("INSERT INTO test"));
        assert!(sql.contains("1, 'Alice'"));
    }

    // --- Import tests ---

    #[test]
    fn test_import_csv_basic() {
        let csv = "name,age\nAlice,30\nBob,25";
        let table = import_csv("imported", csv).unwrap();
        assert_eq!(table.name, "imported");
        assert_eq!(table.col_count(), 2);
        assert_eq!(table.row_count(), 2);
    }

    #[test]
    fn test_import_csv_type_inference() {
        let csv = "id,score\n1,95.5\n2,82.3";
        let table = import_csv("data", csv).unwrap();
        // id column should be inferred as Integer
        assert_eq!(table.columns[0].data_type, DataType::Integer);
        // score column should be inferred as Real
        assert_eq!(table.columns[1].data_type, DataType::Real);
    }

    #[test]
    fn test_import_csv_quoted() {
        let csv = "name,bio\n\"Alice\",\"She said \"\"hello\"\"\"";
        let table = import_csv("quoted", csv).unwrap();
        assert_eq!(table.row_count(), 1);
    }

    #[test]
    fn test_import_csv_empty() {
        assert!(import_csv("empty", "").is_err());
    }

    // --- Constraint describe tests ---

    #[test]
    fn test_constraint_describe_pk() {
        let c = ColumnConstraints { primary_key: true, ..Default::default() };
        assert!(c.describe().contains("PK"));
    }

    #[test]
    fn test_constraint_describe_not_null() {
        let c = ColumnConstraints { not_null: true, ..Default::default() };
        assert!(c.describe().contains("NN"));
    }

    #[test]
    fn test_constraint_describe_unique() {
        let c = ColumnConstraints { unique: true, ..Default::default() };
        assert!(c.describe().contains("UQ"));
    }

    // --- App tests ---

    #[test]
    fn test_app_new() {
        let app = DbViewerApp::new();
        assert_eq!(app.tabs.len(), 1);
        assert!(app.active_db_tab().is_some());
    }

    #[test]
    fn test_app_execute_query() {
        let mut app = DbViewerApp::new();
        app.sql_input = "SELECT * FROM users".to_owned();
        app.execute_query();
        assert!(app.query_result.is_some());
        let result = app.query_result.as_ref().unwrap();
        assert!(!result.is_error);
        assert_eq!(result.rows.len(), 10);
    }

    #[test]
    fn test_app_execute_bad_query() {
        let mut app = DbViewerApp::new();
        app.sql_input = "INVALID SQL".to_owned();
        app.execute_query();
        assert!(app.query_result.as_ref().unwrap().is_error);
    }

    #[test]
    fn test_app_execute_empty_query() {
        let mut app = DbViewerApp::new();
        app.sql_input.clear();
        app.execute_query();
        assert!(app.query_result.as_ref().unwrap().is_error);
    }

    #[test]
    fn test_app_add_tab() {
        let mut app = DbViewerApp::new();
        app.add_tab("new.db");
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active_tab, 1);
    }

    #[test]
    fn test_app_close_tab() {
        let mut app = DbViewerApp::new();
        app.add_tab("second.db");
        app.close_tab(0);
        assert_eq!(app.tabs.len(), 1);
    }

    #[test]
    fn test_app_close_last_tab() {
        let mut app = DbViewerApp::new();
        app.close_tab(0);
        assert_eq!(app.tabs.len(), 1); // Should not close last tab
    }

    #[test]
    fn test_app_select_table() {
        let mut app = DbViewerApp::new();
        app.select_table("products");
        assert_eq!(app.active_db_tab().unwrap().selected_table.as_deref(), Some("products"));
    }

    #[test]
    fn test_app_toggle_sort() {
        let mut app = DbViewerApp::new();
        app.toggle_sort(0);
        let sort = app.active_db_tab().unwrap().sort_state.as_ref().unwrap();
        assert_eq!(sort.column_idx, 0);
        assert_eq!(sort.direction, SortDir::Ascending);

        app.toggle_sort(0);
        let sort = app.active_db_tab().unwrap().sort_state.as_ref().unwrap();
        assert_eq!(sort.direction, SortDir::Descending);
    }

    #[test]
    fn test_app_pagination() {
        let mut app = DbViewerApp::new();
        assert_eq!(app.active_db_tab().unwrap().page, 0);
        app.next_page(); // Only 10 rows with PAGE_SIZE=50, no change
        app.prev_page();
        assert_eq!(app.active_db_tab().unwrap().page, 0);
    }

    #[test]
    fn test_app_add_filter() {
        let mut app = DbViewerApp::new();
        app.filter_column_idx = 3; // age
        app.filter_op_idx = 0; // Equal
        app.filter_value = "30".to_owned();
        app.add_filter();
        assert_eq!(app.active_db_tab().unwrap().filters.len(), 1);
    }

    #[test]
    fn test_app_remove_filter() {
        let mut app = DbViewerApp::new();
        app.filter_value = "test".to_owned();
        app.add_filter();
        app.remove_filter(0);
        assert!(app.active_db_tab().unwrap().filters.is_empty());
    }

    #[test]
    fn test_app_delete_row() {
        let mut app = DbViewerApp::new();
        let initial_count = app.active_db_tab().unwrap().db.find_table("users").unwrap().row_count();
        app.delete_row(0);
        let new_count = app.active_db_tab().unwrap().db.find_table("users").unwrap().row_count();
        assert_eq!(new_count, initial_count - 1);
    }

    #[test]
    fn test_app_export_csv() {
        let app = DbViewerApp::new();
        let csv = app.export_current_table(ExportFormat::Csv);
        assert!(csv.is_some());
        assert!(csv.unwrap().contains("id,name,email,age,score"));
    }

    #[test]
    fn test_app_export_json() {
        let app = DbViewerApp::new();
        let json = app.export_current_table(ExportFormat::Json);
        assert!(json.is_some());
        assert!(json.unwrap().contains("\"name\""));
    }

    #[test]
    fn test_app_export_sql() {
        let app = DbViewerApp::new();
        let sql = app.export_current_table(ExportFormat::SqlInserts);
        assert!(sql.is_some());
        assert!(sql.unwrap().contains("INSERT INTO users"));
    }

    #[test]
    fn test_app_import_csv() {
        let mut app = DbViewerApp::new();
        let csv = "city,pop\nNY,8000000\nLA,4000000";
        assert!(app.import_csv_data("cities", csv).is_ok());
        assert!(app.active_db_tab().unwrap().db.find_table("cities").is_some());
    }

    #[test]
    fn test_app_toggle_favorite() {
        let mut app = DbViewerApp::new();
        app.sql_input = "SELECT 1".to_owned();
        app.execute_query();
        assert!(!app.history[0].favorite);
        app.toggle_favorite(0);
        assert!(app.history[0].favorite);
    }

    #[test]
    fn test_app_history() {
        let mut app = DbViewerApp::new();
        app.sql_input = "SELECT * FROM users".to_owned();
        app.execute_query();
        app.sql_input = "SELECT * FROM products".to_owned();
        app.execute_query();
        assert_eq!(app.history.len(), 2);
    }

    #[test]
    fn test_app_render() {
        let app = DbViewerApp::new();
        let cmds = app.render(1200.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_results_panel() {
        let mut app = DbViewerApp::new();
        app.sql_input = "SELECT * FROM users".to_owned();
        app.execute_query();
        let cmds = app.render(1200.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_current_table_data_with_filter() {
        let mut app = DbViewerApp::new();
        app.filter_column_idx = 3; // age
        app.filter_op_idx = 4; // GreaterOrEqual
        app.filter_value = "35".to_owned();
        app.add_filter();
        let (_, rows) = app.active_db_tab().unwrap().current_table_data().unwrap();
        for row in &rows {
            if let Some(CellValue::Integer(age)) = row.get(3) {
                assert!(*age >= 35);
            }
        }
    }

    #[test]
    fn test_current_table_data_with_sort() {
        let mut app = DbViewerApp::new();
        app.toggle_sort(3); // Sort by age ascending
        let (_, rows) = app.active_db_tab().unwrap().current_table_data().unwrap();
        let ages: Vec<i64> = rows.iter().filter_map(|r| {
            if let Some(CellValue::Integer(v)) = r.get(3) { Some(*v) } else { None }
        }).collect();
        for w in ages.windows(2) {
            assert!(w[0] <= w[1]);
        }
    }

    // --- Tree node tests ---

    #[test]
    fn test_build_tree_nodes() {
        let db = Database::sample();
        let nodes = build_tree_nodes(&db);
        assert!(!nodes.is_empty());
        // Should have headers for tables, indexes, views, triggers
        assert!(nodes.iter().any(|n| n.kind == TreeNodeKind::TablesHeader));
        assert!(nodes.iter().any(|n| n.kind == TreeNodeKind::IndexesHeader));
        assert!(nodes.iter().any(|n| n.kind == TreeNodeKind::ViewsHeader));
        assert!(nodes.iter().any(|n| n.kind == TreeNodeKind::TriggersHeader));
    }

    // --- Truncate helper test ---

    #[test]
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_str_long() {
        let result = truncate_str("hello world this is long", 10);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 13); // 10 chars + "..."
    }

    // --- Filter op tests ---

    #[test]
    fn test_filter_op_labels() {
        assert_eq!(FilterOp::Equal.label(), "=");
        assert_eq!(FilterOp::NotEqual.label(), "!=");
        assert_eq!(FilterOp::Like.label(), "LIKE");
        assert_eq!(FilterOp::IsNull.label(), "IS NULL");
    }

    #[test]
    fn test_filter_op_all() {
        assert_eq!(FilterOp::all().len(), 9);
    }

    // --- Aggregate function tests ---

    #[test]
    fn test_agg_func_from_keyword() {
        assert_eq!(AggFunc::from_keyword("COUNT"), Some(AggFunc::Count));
        assert_eq!(AggFunc::from_keyword("sum"), Some(AggFunc::Sum));
        assert_eq!(AggFunc::from_keyword("avg"), Some(AggFunc::Avg));
        assert_eq!(AggFunc::from_keyword("MIN"), Some(AggFunc::Min));
        assert_eq!(AggFunc::from_keyword("MAX"), Some(AggFunc::Max));
        assert_eq!(AggFunc::from_keyword("INVALID"), None);
    }

    #[test]
    fn test_agg_func_labels() {
        assert_eq!(AggFunc::Count.label(), "COUNT");
        assert_eq!(AggFunc::Sum.label(), "SUM");
    }

    // --- Column def builder tests ---

    #[test]
    fn test_column_def_builder() {
        let col = ColumnDef::new("id", DataType::Integer)
            .with_primary_key()
            .with_auto_increment();
        assert!(col.constraints.primary_key);
        assert!(col.constraints.auto_increment);
        assert!(col.constraints.not_null); // PK implies NN
    }

    #[test]
    fn test_column_def_default() {
        let col = ColumnDef::new("score", DataType::Real).with_default("0.0");
        assert_eq!(col.constraints.default_value.as_deref(), Some("0.0"));
    }

    // --- Bottom panel tests ---

    #[test]
    fn test_bottom_panel_labels() {
        assert_eq!(BottomPanel::SqlEditor.label(), "SQL Editor");
        assert_eq!(BottomPanel::Results.label(), "Results");
        assert_eq!(BottomPanel::Schema.label(), "Schema");
        assert_eq!(BottomPanel::Diagram.label(), "Diagram");
    }

    #[test]
    fn test_bottom_panel_all() {
        assert_eq!(BottomPanel::all().len(), 4);
    }
}
