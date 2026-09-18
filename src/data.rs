use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

pub const DEFAULT_PARENT_PIN: &str = "1234";
pub const DEFAULT_CHILD_NAMES: [&str; 2] = ["Child 1", "Child 2"];
pub const MAX_CHILD_NAME_CHARS: usize = 40;
pub const MAX_ABSOLUTE_CENTS: i64 = 99_999_999_999;
pub const MAX_DESCRIPTION_CHARS: usize = 100;
pub const STARTING_BALANCE_DESCRIPTION: &str = "Starting balance";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppData {
    /// Only populated when deserializing a legacy v2 PIN envelope. New story
    /// payloads never serialize an authentication secret into the ledger.
    #[serde(default, skip_serializing)]
    pub parent_pin: String,
    pub wallets: Vec<Wallet>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    pub child_name: String,
    pub starting_balance_cents: i64,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub date: NaiveDate,
    pub description: String,
    pub amount_cents: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EntryKind {
    Deposit,
    #[default]
    Deduction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerSort {
    NewestFirst,
    OldestFirst,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerRowDate {
    Start,
    Entry(NaiveDate),
}

impl LedgerRowDate {
    pub fn label(self) -> String {
        match self {
            Self::Start => "Start".to_owned(),
            Self::Entry(date) => format_ledger_date(date),
        }
    }
}

pub fn format_ledger_date(date: NaiveDate) -> String {
    date.format("%m/%d/%Y").to_string()
}

pub fn parse_ledger_date(input: &str) -> Result<NaiveDate, String> {
    let trimmed = input.trim();
    NaiveDate::parse_from_str(trimmed, "%m/%d/%Y")
        .or_else(|_| NaiveDate::parse_from_str(trimmed, "%Y-%m-%d"))
        .map_err(|_| {
            "Enter a date as MM/DD/YYYY (08/21/2026) or ISO %Y-%m-%d (2026-08-21).".to_owned()
        })
}

#[derive(Debug, Clone, Copy)]
pub struct LedgerRow<'a> {
    pub date: LedgerRowDate,
    pub description: &'a str,
    pub amount_cents: i64,
    pub balance_cents: i64,
    /// Index into `Wallet::entries`, or `None` for the synthetic starting-balance
    /// row. Rows are sorted and filtered for display, so this is what lets the UI
    /// point back at the entry a row came from (e.g. to correct it).
    pub entry_index: Option<usize>,
}

/// Owned ledger row for UI caching (avoids rebuilding borrows every frame).
#[derive(Debug, Clone)]
pub struct OwnedLedgerRow {
    pub date: LedgerRowDate,
    pub description: String,
    pub amount_cents: i64,
    pub balance_cents: i64,
    /// See `LedgerRow::entry_index`.
    pub entry_index: Option<usize>,
}

impl OwnedLedgerRow {
    pub fn from_borrowed(row: LedgerRow<'_>) -> Self {
        Self {
            date: row.date,
            description: row.description.to_owned(),
            amount_cents: row.amount_cents,
            balance_cents: row.balance_cents,
            entry_index: row.entry_index,
        }
    }
}

impl LedgerSort {
    pub fn toggle(&mut self) {
        *self = match self {
            Self::NewestFirst => Self::OldestFirst,
            Self::OldestFirst => Self::NewestFirst,
        };
    }
}

impl Wallet {
    pub fn current_balance_cents(&self) -> i64 {
        self.entries
            .iter()
            .fold(self.starting_balance_cents, |balance, entry| {
                clamp_cents(balance.saturating_add(entry.amount_cents))
            })
    }

    pub fn ledger_rows(&self) -> Vec<LedgerRow<'_>> {
        let mut balance = self.starting_balance_cents;
        let mut rows = Vec::with_capacity(self.entries.len() + 1);

        rows.push(LedgerRow {
            date: LedgerRowDate::Start,
            description: STARTING_BALANCE_DESCRIPTION,
            amount_cents: self.starting_balance_cents,
            balance_cents: self.starting_balance_cents,
            entry_index: None,
        });

        for (index, entry) in self.entries.iter().enumerate() {
            balance = clamp_cents(balance.saturating_add(entry.amount_cents));
            rows.push(LedgerRow {
                date: LedgerRowDate::Entry(entry.date),
                description: &entry.description,
                amount_cents: entry.amount_cents,
                balance_cents: balance,
                entry_index: Some(index),
            });
        }

        rows
    }

    pub fn balances_are_valid(&self) -> bool {
        self.checked_running_balances().is_some()
    }

    fn checked_running_balances(&self) -> Option<Vec<i64>> {
        if !valid_cents(self.starting_balance_cents)
            || self
                .entries
                .iter()
                .any(|entry| !valid_cents(entry.amount_cents))
        {
            return None;
        }

        let mut balance = self.starting_balance_cents;
        let mut balances = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            balance = balance.checked_add(entry.amount_cents)?;
            if !valid_cents(balance) {
                return None;
            }
            balances.push(balance);
        }

        Some(balances)
    }

    pub fn ledger_rows_sorted(&self, sort: LedgerSort) -> Vec<LedgerRow<'_>> {
        let mut rows: Vec<_> = self.ledger_rows().into_iter().enumerate().collect();

        rows.sort_by(|(left_index, left_row), (right_index, right_row)| {
            let chronological = compare_ledger_row_dates(left_row.date, right_row.date)
                .then_with(|| left_index.cmp(right_index));

            match sort {
                LedgerSort::NewestFirst => chronological.reverse(),
                LedgerSort::OldestFirst => chronological,
            }
        });

        rows.into_iter().map(|(_, row)| row).collect()
    }

    pub fn ledger_rows_sorted_owned(&self, sort: LedgerSort) -> Vec<OwnedLedgerRow> {
        self.ledger_rows_sorted(sort)
            .into_iter()
            .map(OwnedLedgerRow::from_borrowed)
            .collect()
    }

    /// The most recently recorded deposit, for "Repeat last deposit" prefill.
    ///
    /// Entries don't carry their own `EntryKind` tag; a deposit is any entry
    /// with a positive `amount_cents` (deductions are stored negative -- see
    /// `add_entry`'s `signed_amount`). Newest is chosen by date, tie-broken by
    /// list index (the entry added later wins), matching the ordering
    /// `ledger_rows_sorted` uses for `LedgerSort::NewestFirst`.
    pub fn latest_deposit(&self) -> Option<&Entry> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.amount_cents > 0)
            .max_by_key(|(index, entry)| (entry.date, *index))
            .map(|(_, entry)| entry)
    }
}

/// Narrows already-sorted ledger rows to those matching a case-insensitive
/// description substring search, for the ledger table's local filter
/// (issue #139). Purely a display-level view over `rows`: it never touches
/// `Wallet::entries` and preserves the input order (i.e. whatever sort was
/// already applied), it only omits non-matching entries.
///
/// The starting-balance row is exempt from the filter and always kept,
/// per the issue's stated preference ("always show start row") -- it isn't
/// a real transaction a parent would be searching for, and hiding it would
/// make an empty-looking table ambiguous with the "no matches" case.
///
/// An empty (or whitespace-only) query matches every row.
#[cfg(test)]
pub fn filter_ledger_rows<'a>(rows: &'a [OwnedLedgerRow], query: &str) -> Vec<&'a OwnedLedgerRow> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return rows.iter().collect();
    }

    rows.iter()
        .filter(|row| {
            matches!(row.date, LedgerRowDate::Start)
                || row.description.to_lowercase().contains(&query)
        })
        .collect()
}

/// Filtered ledger rows plus the match count and net used by the table chrome.
///
/// `rows` follows [`filter_ledger_rows`] (starting-balance row always kept).
/// `matching_entry_count` and `matching_net_cents` exclude that start row.
#[derive(Debug)]
pub struct LedgerFilterSummary<'a> {
    pub rows: Vec<&'a OwnedLedgerRow>,
    pub matching_entry_count: usize,
    pub matching_net_cents: i64,
}

/// Walks `rows` once: same filter as [`filter_ledger_rows`], plus the entry
/// count and signed net the ledger table shows beside Clear.
pub fn ledger_filter_summary<'a>(
    rows: &'a [OwnedLedgerRow],
    query: &str,
) -> LedgerFilterSummary<'a> {
    let query = query.trim().to_lowercase();
    let keep_all = query.is_empty();
    let mut filtered = Vec::new();
    let mut matching_entry_count = 0;
    let mut matching_net_cents = 0;

    for row in rows {
        let is_start = matches!(row.date, LedgerRowDate::Start);
        if keep_all || is_start || row.description.to_lowercase().contains(&query) {
            filtered.push(row);
            if !is_start {
                matching_entry_count += 1;
                matching_net_cents += row.amount_cents;
            }
        }
    }

    LedgerFilterSummary {
        rows: filtered,
        matching_entry_count,
        matching_net_cents,
    }
}

fn compare_ledger_row_dates(left: LedgerRowDate, right: LedgerRowDate) -> Ordering {
    match (left, right) {
        (LedgerRowDate::Start, LedgerRowDate::Start) => Ordering::Equal,
        (LedgerRowDate::Start, LedgerRowDate::Entry(_)) => Ordering::Less,
        (LedgerRowDate::Entry(_), LedgerRowDate::Start) => Ordering::Greater,
        (LedgerRowDate::Entry(left_date), LedgerRowDate::Entry(right_date)) => {
            left_date.cmp(&right_date)
        }
    }
}

pub fn default_app_data() -> AppData {
    AppData {
        parent_pin: DEFAULT_PARENT_PIN.to_owned(),
        wallets: default_wallets(),
    }
}

pub fn default_wallets() -> Vec<Wallet> {
    DEFAULT_CHILD_NAMES
        .iter()
        .map(|name| Wallet {
            child_name: (*name).to_owned(),
            starting_balance_cents: 0,
            entries: Vec::new(),
        })
        .collect()
}

pub fn normalize_app_data(mut data: AppData) -> Option<AppData> {
    if data.wallets.is_empty() {
        return None;
    }

    if data
        .wallets
        .iter()
        .any(|wallet| !wallet.balances_are_valid())
    {
        return None;
    }

    if !valid_pin(&data.parent_pin) {
        data.parent_pin = DEFAULT_PARENT_PIN.to_owned();
    }

    Some(data)
}

pub fn valid_pin(pin: &str) -> bool {
    pin.len() == 4 && pin.chars().all(|character| character.is_ascii_digit())
}

pub fn valid_child_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= MAX_CHILD_NAME_CHARS
}

pub fn valid_cents(cents: i64) -> bool {
    cents.unsigned_abs() <= MAX_ABSOLUTE_CENTS as u64
}

pub fn valid_description(desc: &str) -> bool {
    let trimmed = desc.trim();
    !trimmed.is_empty() && trimmed.chars().count() <= MAX_DESCRIPTION_CHARS
}

pub fn description_char_count(desc: &str) -> usize {
    desc.chars().count()
}

pub fn description_length_label(desc: &str) -> String {
    format!("{}/{}", description_char_count(desc), MAX_DESCRIPTION_CHARS)
}

pub fn description_length_accessible_name(desc: &str) -> String {
    format!(
        "What was it for? {} of {} characters",
        description_char_count(desc),
        MAX_DESCRIPTION_CHARS
    )
}

fn clamp_cents(cents: i64) -> i64 {
    cents.clamp(-MAX_ABSOLUTE_CENTS, MAX_ABSOLUTE_CENTS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_child_names() {
        assert!(valid_child_name("Child 1"));
        assert!(!valid_child_name(""));
        assert!(!valid_child_name("   "));
        assert!(!valid_child_name(
            "This name is too long for the Cofferly sidebar"
        ));
    }

    #[test]
    fn validates_description_and_amount_boundaries() {
        assert!(valid_description("Allowance"));
        assert!(!valid_description("   "));
        assert!(valid_description(&"a".repeat(MAX_DESCRIPTION_CHARS)));
        assert!(!valid_description(&"a".repeat(MAX_DESCRIPTION_CHARS + 1)));
        assert_eq!(description_length_label(""), "0/100");
        assert_eq!(description_length_label("Allowance"), "9/100");
        assert_eq!(
            description_length_label(&"a".repeat(MAX_DESCRIPTION_CHARS)),
            "100/100"
        );
        assert_eq!(
            description_length_accessible_name("Snack"),
            "What was it for? 5 of 100 characters"
        );

        assert!(valid_cents(MAX_ABSOLUTE_CENTS));
        assert!(valid_cents(-MAX_ABSOLUTE_CENTS));
        assert!(!valid_cents(MAX_ABSOLUTE_CENTS + 1));
        assert!(!valid_cents(-(MAX_ABSOLUTE_CENTS + 1)));
    }

    #[test]
    fn ledger_sort_toggle_and_date_labels_are_predictable() {
        let mut sort = LedgerSort::NewestFirst;
        sort.toggle();
        assert_eq!(sort, LedgerSort::OldestFirst);
        sort.toggle();
        assert_eq!(sort, LedgerSort::NewestFirst);

        assert_eq!(LedgerRowDate::Start.label(), "Start");
        assert_eq!(
            LedgerRowDate::Entry(NaiveDate::from_ymd_opt(2026, 7, 11).unwrap()).label(),
            "07/11/2026"
        );
        assert_eq!(
            parse_ledger_date("07/11/2026").unwrap(),
            NaiveDate::from_ymd_opt(2026, 7, 11).unwrap()
        );
        assert_eq!(
            parse_ledger_date("2026-07-11").unwrap(),
            NaiveDate::from_ymd_opt(2026, 7, 11).unwrap()
        );
        assert_eq!(
            parse_ledger_date("not a date").unwrap_err(),
            "Enter a date as MM/DD/YYYY (08/21/2026) or ISO %Y-%m-%d (2026-08-21)."
        );
    }

    #[test]
    fn rejects_empty_loaded_wallets() {
        let data = AppData {
            parent_pin: "1234".to_owned(),
            wallets: Vec::new(),
        };

        assert!(normalize_app_data(data).is_none());
    }

    #[test]
    fn resets_invalid_loaded_pin() {
        let data = AppData {
            parent_pin: "nope".to_owned(),
            wallets: default_wallets(),
        };

        assert_eq!(
            normalize_app_data(data).unwrap().parent_pin,
            DEFAULT_PARENT_PIN
        );
    }

    #[test]
    fn rejects_loaded_wallets_with_out_of_range_amounts() {
        let data = AppData {
            parent_pin: "1234".to_owned(),
            wallets: vec![Wallet {
                child_name: "Child 1".to_owned(),
                starting_balance_cents: MAX_ABSOLUTE_CENTS + 1,
                entries: Vec::new(),
            }],
        };

        assert!(normalize_app_data(data).is_none());
    }

    #[test]
    fn rejects_loaded_wallets_with_overflowing_running_balances() {
        let data = AppData {
            parent_pin: "1234".to_owned(),
            wallets: vec![Wallet {
                child_name: "Child 1".to_owned(),
                starting_balance_cents: MAX_ABSOLUTE_CENTS,
                entries: vec![Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 10).unwrap(),
                    description: "Too much".to_owned(),
                    amount_cents: 1,
                }],
            }],
        };

        assert!(normalize_app_data(data).is_none());
    }

    #[test]
    fn sorts_ledger_rows_newest_first_with_historical_balances() {
        let wallet = Wallet {
            child_name: "Child 1".to_owned(),
            starting_balance_cents: 1000,
            entries: vec![
                Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
                    description: "First".to_owned(),
                    amount_cents: 500,
                },
                Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 9).unwrap(),
                    description: "Second".to_owned(),
                    amount_cents: -200,
                },
                Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 9).unwrap(),
                    description: "Latest".to_owned(),
                    amount_cents: 100,
                },
            ],
        };

        let rows = wallet.ledger_rows_sorted(LedgerSort::NewestFirst);
        let descriptions: Vec<_> = rows.iter().map(|row| row.description).collect();
        let balances: Vec<_> = rows.iter().map(|row| row.balance_cents).collect();

        assert_eq!(
            descriptions,
            ["Latest", "Second", "First", "Starting balance"]
        );
        assert_eq!(balances, [1400, 1300, 1500, 1000]);
    }

    #[test]
    fn sorts_ledger_rows_oldest_first() {
        let wallet = Wallet {
            child_name: "Child 1".to_owned(),
            starting_balance_cents: 0,
            entries: vec![
                Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
                    description: "First".to_owned(),
                    amount_cents: 100,
                },
                Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 9).unwrap(),
                    description: "Second".to_owned(),
                    amount_cents: 100,
                },
            ],
        };

        let rows = wallet.ledger_rows_sorted(LedgerSort::OldestFirst);
        let descriptions: Vec<_> = rows.iter().map(|row| row.description).collect();

        assert_eq!(descriptions, ["Starting balance", "First", "Second"]);
    }

    fn filter_test_wallet() -> Wallet {
        Wallet {
            child_name: "Child 1".to_owned(),
            starting_balance_cents: 1000,
            entries: vec![
                Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
                    description: "Weekly allowance".to_owned(),
                    amount_cents: 500,
                },
                Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 9).unwrap(),
                    description: "Snack".to_owned(),
                    amount_cents: -200,
                },
                Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 10).unwrap(),
                    description: "Birthday gift".to_owned(),
                    amount_cents: 2500,
                },
            ],
        }
    }

    #[test]
    fn filter_ledger_rows_with_empty_query_returns_every_row_unchanged() {
        let wallet = filter_test_wallet();
        let rows = wallet.ledger_rows_sorted_owned(LedgerSort::OldestFirst);

        let filtered = filter_ledger_rows(&rows, "");
        let descriptions: Vec<_> = filtered
            .iter()
            .map(|row| row.description.as_str())
            .collect();

        assert_eq!(
            descriptions,
            [
                "Starting balance",
                "Weekly allowance",
                "Snack",
                "Birthday gift"
            ]
        );
    }

    #[test]
    fn filter_ledger_rows_narrows_to_case_insensitive_substring_matches() {
        let wallet = filter_test_wallet();
        let rows = wallet.ledger_rows_sorted_owned(LedgerSort::OldestFirst);

        let filtered = filter_ledger_rows(&rows, "ALLOW");
        let descriptions: Vec<_> = filtered
            .iter()
            .map(|row| row.description.as_str())
            .collect();

        // The starting-balance row is always kept alongside whatever entries match.
        assert_eq!(descriptions, ["Starting balance", "Weekly allowance"]);
    }

    #[test]
    fn filter_ledger_rows_always_keeps_the_starting_balance_row() {
        let wallet = filter_test_wallet();
        let rows = wallet.ledger_rows_sorted_owned(LedgerSort::OldestFirst);

        // A query that matches zero entries still keeps the start row.
        let filtered = filter_ledger_rows(&rows, "nonexistent description");

        assert_eq!(filtered.len(), 1);
        assert!(matches!(filtered[0].date, LedgerRowDate::Start));
    }

    #[test]
    fn filter_ledger_rows_treats_whitespace_only_query_as_empty() {
        let wallet = filter_test_wallet();
        let rows = wallet.ledger_rows_sorted_owned(LedgerSort::OldestFirst);

        let filtered = filter_ledger_rows(&rows, "   ");

        assert_eq!(filtered.len(), rows.len());
    }

    #[test]
    fn ledger_filter_summary_is_a_single_pass_over_filter_count_and_net() {
        let wallet = filter_test_wallet();
        let rows = wallet.ledger_rows_sorted_owned(LedgerSort::OldestFirst);

        struct Case {
            query: &'static str,
            descriptions: &'static [&'static str],
            matching_entry_count: usize,
            matching_net_cents: i64,
        }

        let cases = [
            Case {
                query: "",
                descriptions: &[
                    "Starting balance",
                    "Weekly allowance",
                    "Snack",
                    "Birthday gift",
                ],
                matching_entry_count: 3,
                matching_net_cents: 2_800,
            },
            Case {
                query: "ALLOW",
                descriptions: &["Starting balance", "Weekly allowance"],
                matching_entry_count: 1,
                matching_net_cents: 500,
            },
            Case {
                query: "snack",
                descriptions: &["Starting balance", "Snack"],
                matching_entry_count: 1,
                matching_net_cents: -200,
            },
            Case {
                query: "nonexistent",
                descriptions: &["Starting balance"],
                matching_entry_count: 0,
                matching_net_cents: 0,
            },
        ];

        for case in cases {
            let summary = ledger_filter_summary(&rows, case.query);
            let descriptions: Vec<_> = summary
                .rows
                .iter()
                .map(|row| row.description.as_str())
                .collect();

            assert_eq!(descriptions, case.descriptions, "rows for {:?}", case.query);
            assert_eq!(
                summary.matching_entry_count, case.matching_entry_count,
                "count for {:?}",
                case.query
            );
            assert_eq!(
                summary.matching_net_cents, case.matching_net_cents,
                "net for {:?}",
                case.query
            );
            assert!(
                matches!(summary.rows[0].date, LedgerRowDate::Start),
                "start row missing for {:?}",
                case.query
            );
        }
    }

    #[test]
    fn latest_deposit_is_none_without_any_entries() {
        let wallet = Wallet {
            child_name: "Child 1".to_owned(),
            starting_balance_cents: 500,
            entries: Vec::new(),
        };

        assert!(wallet.latest_deposit().is_none());
    }

    #[test]
    fn latest_deposit_is_none_with_only_deductions() {
        let wallet = Wallet {
            child_name: "Child 1".to_owned(),
            starting_balance_cents: 500,
            entries: vec![Entry {
                date: NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
                description: "Snack".to_owned(),
                amount_cents: -200,
            }],
        };

        assert!(wallet.latest_deposit().is_none());
    }

    #[test]
    fn latest_deposit_picks_newest_by_date_ignoring_deductions() {
        let wallet = Wallet {
            child_name: "Child 1".to_owned(),
            starting_balance_cents: 0,
            entries: vec![
                Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
                    description: "Allowance".to_owned(),
                    amount_cents: 1000,
                },
                Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 9).unwrap(),
                    description: "Snack".to_owned(),
                    amount_cents: -200,
                },
                Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 10).unwrap(),
                    description: "Birthday gift".to_owned(),
                    amount_cents: 2500,
                },
            ],
        };

        let latest = wallet.latest_deposit().unwrap();
        assert_eq!(latest.description, "Birthday gift");
        assert_eq!(latest.amount_cents, 2500);
    }

    #[test]
    fn latest_deposit_breaks_same_day_ties_by_list_index() {
        let wallet = Wallet {
            child_name: "Child 1".to_owned(),
            starting_balance_cents: 0,
            entries: vec![
                Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
                    description: "Allowance".to_owned(),
                    amount_cents: 1000,
                },
                Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
                    description: "Bonus chore".to_owned(),
                    amount_cents: 300,
                },
            ],
        };

        let latest = wallet.latest_deposit().unwrap();
        assert_eq!(latest.description, "Bonus chore");
        assert_eq!(latest.amount_cents, 300);
    }
}
