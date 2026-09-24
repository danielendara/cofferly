use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

pub const DEFAULT_PARENT_PIN: &str = "1234";
pub const DEFAULT_CHILD_NAMES: [&str; 2] = ["Child 1", "Child 2"];
pub const MAX_CHILD_NAME_CHARS: usize = 40;
pub const MAX_ABSOLUTE_CENTS: i64 = 99_999_999_999;
pub const MAX_DESCRIPTION_CHARS: usize = 100;
pub const STARTING_BALANCE_DESCRIPTION: &str = "Starting balance";
pub const WEEKLY_ALLOWANCE_DESCRIPTION: &str = "Weekly allowance";
/// Missed weeks posted at one unlock; older missed weeks are skipped (and reported).
pub const MAX_ALLOWANCE_CATCH_UP_WEEKS: i64 = 8;

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
    /// Optional auto-posted weekly allowance (#179). `None` = off. Defaulted so
    /// older vaults and backups load unchanged, and omitted when off so they
    /// also serialize unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weekly_allowance: Option<WeeklyAllowance>,
}

/// A wallet's weekly allowance. Dates are calendar dates (`NaiveDate` from the
/// local clock), so DST shifts can't move a posting day.
///
/// Rules (#179):
/// - The weekday is the day it was turned on (`enabled_on`). That day itself
///   does **not** post: the parent turning it on has usually just handled this
///   week, so the first automatic entry is one week later.
/// - `last_posted` is the latest weekday already accounted for: `enabled_on`
///   until the first post, then the date of the newest posted entry. It only
///   moves forward and is saved in the same vault write as the entries it
///   covers, so re-unlocking, relaunching, or restoring a backup never posts a
///   week twice.
/// - Changing the amount keeps `enabled_on` and `last_posted`; only future weeks
///   use the new amount. Turning it off drops the whole setting; turning it
///   back on starts fresh from that day (the weeks it was off never post).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeeklyAllowance {
    pub amount_cents: i64,
    pub enabled_on: NaiveDate,
    pub last_posted: NaiveDate,
}

impl WeeklyAllowance {
    pub fn starting(amount_cents: i64, today: NaiveDate) -> Self {
        Self {
            amount_cents,
            enabled_on: today,
            last_posted: today,
        }
    }

    pub fn weekday_name(&self) -> String {
        self.enabled_on.format("%A").to_string()
    }
}

/// What one catch-up pass did for a wallet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AllowancePosting {
    /// Entries added.
    pub posted: usize,
    /// Missed weeks older than the 8-week catch-up window, never posted.
    pub skipped_weeks: usize,
    /// Posting stopped because the next entry would leave the supported
    /// balance range (`MAX_ABSOLUTE_CENTS`). Retried at the next unlock.
    pub stopped_at_limit: bool,
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

    /// Posts one "Weekly allowance" deposit per missed weekday after
    /// `last_posted`, up to and including `today` -- never a future date. At
    /// most the newest `MAX_ALLOWANCE_CATCH_UP_WEEKS` are posted; older missed
    /// weeks are skipped and counted. A clock set before `last_posted` posts
    /// nothing and leaves `last_posted` alone. Stops (without advancing past
    /// the last posted week) if an entry would overflow the balance range.
    pub fn post_due_allowance(&mut self, today: NaiveDate) -> AllowancePosting {
        let mut result = AllowancePosting::default();
        let Some(mut allowance) = self.weekly_allowance else {
            return result;
        };
        if allowance.amount_cents <= 0 || !valid_cents(allowance.amount_cents) {
            return result;
        }

        let days_since = (today - allowance.last_posted).num_days();
        let weeks_due = days_since.div_euclid(7);
        if weeks_due <= 0 {
            return result;
        }

        let first_week = (weeks_due - MAX_ALLOWANCE_CATCH_UP_WEEKS).max(0) + 1;
        let skipped_weeks = (first_week - 1) as usize;
        let mut last_posted = allowance.last_posted;
        for week in first_week..=weeks_due {
            let date = allowance.last_posted + chrono::Duration::weeks(week);
            self.entries.push(Entry {
                date,
                description: WEEKLY_ALLOWANCE_DESCRIPTION.to_owned(),
                amount_cents: allowance.amount_cents,
            });
            if !self.balances_are_valid() {
                self.entries.pop();
                result.stopped_at_limit = true;
                break;
            }
            result.posted += 1;
            last_posted = date;
        }

        // Skipped weeks are only written off once the window after them posted
        // (or at least started to); otherwise the next unlock re-evaluates them.
        if result.posted > 0 {
            result.skipped_weeks = skipped_weeks;
            allowance.last_posted = last_posted;
            self.weekly_allowance = Some(allowance);
        }
        result
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

/// Filtered ledger rows plus match count and net for the ledger table chrome.
#[derive(Debug)]
pub struct LedgerFilterSummary<'a> {
    pub rows: Vec<&'a OwnedLedgerRow>,
    pub matching_entry_count: usize,
    pub matching_net_cents: i64,
}

/// Narrows already-sorted ledger rows for the table's local description filter
/// (issue #139) in one pass. Purely display-level: never touches
/// `Wallet::entries`, preserves input order, and only omits non-matching
/// entries.
///
/// Case-insensitive substring match on `description`. An empty or
/// whitespace-only query keeps every row. The starting-balance row is always
/// kept (even when nothing else matches) so a filtered table is never
/// confused with having no ledger at all.
///
/// `matching_entry_count` and `matching_net_cents` count only real entries,
/// not the start row.
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
            weekly_allowance: None,
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
                weekly_allowance: None,
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
                weekly_allowance: None,
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
            weekly_allowance: None,
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
            weekly_allowance: None,
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
            weekly_allowance: None,
        }
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
            Case {
                query: "   ",
                descriptions: &[
                    "Starting balance",
                    "Weekly allowance",
                    "Snack",
                    "Birthday gift",
                ],
                matching_entry_count: 3,
                matching_net_cents: 2_800,
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
            weekly_allowance: None,
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
            weekly_allowance: None,
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
            weekly_allowance: None,
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
            weekly_allowance: None,
        };

        let latest = wallet.latest_deposit().unwrap();
        assert_eq!(latest.description, "Bonus chore");
        assert_eq!(latest.amount_cents, 300);
    }

    mod weekly_allowance {
        use super::*;
        use chrono::Datelike;

        fn date(y: i32, m: u32, d: u32) -> NaiveDate {
            NaiveDate::from_ymd_opt(y, m, d).unwrap()
        }

        fn wallet_with(allowance: Option<WeeklyAllowance>) -> Wallet {
            Wallet {
                child_name: "Ada".to_owned(),
                starting_balance_cents: 0,
                entries: Vec::new(),
                weekly_allowance: allowance,
            }
        }

        fn posted_dates(wallet: &Wallet) -> Vec<NaiveDate> {
            wallet
                .entries
                .iter()
                .filter(|entry| entry.description == WEEKLY_ALLOWANCE_DESCRIPTION)
                .map(|entry| entry.date)
                .collect()
        }

        // Tuesday 2026-09-01.
        const AMOUNT: i64 = 500;
        fn enabled() -> WeeklyAllowance {
            WeeklyAllowance::starting(AMOUNT, date(2026, 9, 1))
        }

        #[test]
        fn off_is_a_no_op() {
            let mut wallet = wallet_with(None);
            assert_eq!(
                wallet.post_due_allowance(date(2027, 1, 1)),
                AllowancePosting::default()
            );
            assert!(wallet.entries.is_empty());
            assert!(wallet.weekly_allowance.is_none());
        }

        #[test]
        fn the_enable_day_and_the_rest_of_its_week_post_nothing() {
            let mut wallet = wallet_with(Some(enabled()));
            for day in 1..=7 {
                assert_eq!(wallet.post_due_allowance(date(2026, 9, day)).posted, 0);
            }
            assert!(wallet.entries.is_empty());
            assert_eq!(
                wallet.weekly_allowance.unwrap().last_posted,
                date(2026, 9, 1)
            );
            assert_eq!(enabled().weekday_name(), "Tuesday");
        }

        #[test]
        fn one_missed_week_posts_one_entry_on_the_weekday() {
            let mut wallet = wallet_with(Some(enabled()));
            let result = wallet.post_due_allowance(date(2026, 9, 8));

            assert_eq!(result.posted, 1);
            assert_eq!(result.skipped_weeks, 0);
            assert_eq!(
                wallet.entries,
                vec![Entry {
                    date: date(2026, 9, 8),
                    description: WEEKLY_ALLOWANCE_DESCRIPTION.to_owned(),
                    amount_cents: AMOUNT,
                }]
            );
            assert_eq!(
                wallet.weekly_allowance.unwrap().last_posted,
                date(2026, 9, 8)
            );
            assert_eq!(wallet.current_balance_cents(), AMOUNT);
        }

        #[test]
        fn multiple_missed_weeks_post_each_weekday_and_never_a_future_date() {
            let mut wallet = wallet_with(Some(enabled()));
            // Monday, three Tuesdays later plus six days: the 4th Tuesday is tomorrow.
            let result = wallet.post_due_allowance(date(2026, 9, 28));

            assert_eq!(result.posted, 3);
            assert_eq!(
                posted_dates(&wallet),
                vec![date(2026, 9, 8), date(2026, 9, 15), date(2026, 9, 22)]
            );
            assert_eq!(
                wallet.weekly_allowance.unwrap().last_posted,
                date(2026, 9, 22)
            );
        }

        #[test]
        fn catch_up_is_capped_at_eight_weeks_and_reports_skipped_weeks() {
            let mut wallet = wallet_with(Some(enabled()));
            // 11 Tuesdays after 2026-09-01 is 2026-11-17.
            let result = wallet.post_due_allowance(date(2026, 11, 17));

            assert_eq!(result.posted, 8);
            assert_eq!(result.skipped_weeks, 3);
            let dates = posted_dates(&wallet);
            assert_eq!(dates.first(), Some(&date(2026, 9, 29)));
            assert_eq!(dates.last(), Some(&date(2026, 11, 17)));
            assert_eq!(
                wallet.weekly_allowance.unwrap().last_posted,
                date(2026, 11, 17)
            );

            // The skipped weeks are written off, not retried.
            assert_eq!(
                wallet.post_due_allowance(date(2026, 11, 17)),
                AllowancePosting::default()
            );
        }

        #[test]
        fn posting_is_idempotent_across_repeat_calls_and_a_save_reload() {
            let mut wallet = wallet_with(Some(enabled()));
            assert_eq!(wallet.post_due_allowance(date(2026, 9, 16)).posted, 2);
            assert_eq!(wallet.post_due_allowance(date(2026, 9, 16)).posted, 0);

            let reloaded: Wallet =
                serde_json::from_str(&serde_json::to_string(&wallet).unwrap()).unwrap();
            let mut reloaded = reloaded;
            assert_eq!(reloaded.post_due_allowance(date(2026, 9, 21)).posted, 0);
            assert_eq!(reloaded.entries.len(), 2);
            assert_eq!(reloaded.post_due_allowance(date(2026, 9, 22)).posted, 1);
            assert_eq!(reloaded.entries.len(), 3);
        }

        #[test]
        fn a_clock_set_before_last_posted_posts_nothing_and_never_moves_it_back() {
            let mut allowance = enabled();
            allowance.last_posted = date(2026, 9, 22);
            let mut wallet = wallet_with(Some(allowance));

            for today in [date(2026, 9, 21), date(2026, 9, 1), date(2025, 1, 1)] {
                assert_eq!(
                    wallet.post_due_allowance(today),
                    AllowancePosting::default()
                );
            }
            assert!(wallet.entries.is_empty());
            assert_eq!(wallet.weekly_allowance, Some(allowance));
        }

        #[test]
        fn dst_and_year_boundaries_keep_the_same_weekday() {
            // US DST starts Sun 2027-03-14 and ends Sun 2026-11-01; dates are
            // calendar dates, so neither shift moves the posting day.
            let mut spring = wallet_with(Some(WeeklyAllowance::starting(AMOUNT, date(2027, 3, 7))));
            spring.post_due_allowance(date(2027, 3, 21));
            assert_eq!(
                posted_dates(&spring),
                vec![date(2027, 3, 14), date(2027, 3, 21)]
            );

            let mut fall = wallet_with(Some(WeeklyAllowance::starting(AMOUNT, date(2026, 10, 25))));
            fall.post_due_allowance(date(2026, 11, 8));
            assert_eq!(
                posted_dates(&fall),
                vec![date(2026, 11, 1), date(2026, 11, 8)]
            );

            let mut new_year =
                wallet_with(Some(WeeklyAllowance::starting(AMOUNT, date(2026, 12, 22))));
            new_year.post_due_allowance(date(2027, 1, 6));
            assert_eq!(
                posted_dates(&new_year),
                vec![date(2026, 12, 29), date(2027, 1, 5)]
            );
            assert!(posted_dates(&new_year)
                .iter()
                .all(|d| d.weekday() == chrono::Weekday::Tue));
        }

        #[test]
        fn stops_before_exceeding_max_absolute_cents_and_retries_later() {
            let mut wallet = wallet_with(Some(enabled()));
            wallet.starting_balance_cents = MAX_ABSOLUTE_CENTS - AMOUNT - 1;

            let result = wallet.post_due_allowance(date(2026, 9, 22));
            assert_eq!(result.posted, 1);
            assert!(result.stopped_at_limit);
            assert!(wallet.balances_are_valid());
            assert_eq!(
                wallet.weekly_allowance.unwrap().last_posted,
                date(2026, 9, 8)
            );

            let again = wallet.post_due_allowance(date(2026, 9, 22));
            assert_eq!(again.posted, 0);
            assert!(again.stopped_at_limit);
            assert_eq!(
                wallet.weekly_allowance.unwrap().last_posted,
                date(2026, 9, 8)
            );
            assert_eq!(wallet.entries.len(), 1);
        }

        #[test]
        fn a_non_positive_or_out_of_range_amount_never_posts() {
            for amount_cents in [0, -500, MAX_ABSOLUTE_CENTS + 1] {
                let mut wallet = wallet_with(Some(WeeklyAllowance::starting(
                    amount_cents,
                    date(2026, 9, 1),
                )));
                assert_eq!(wallet.post_due_allowance(date(2026, 9, 29)).posted, 0);
                assert!(wallet.entries.is_empty());
            }
        }

        #[test]
        fn vaults_without_the_field_load_and_save_unchanged() {
            let legacy = r#"{"wallets":[{"child_name":"Ada","starting_balance_cents":100,"entries":[{"date":"2026-09-01","description":"Weekly allowance","amount_cents":500}]}]}"#;
            let data: AppData = serde_json::from_str(legacy).unwrap();
            assert!(data.wallets[0].weekly_allowance.is_none());
            assert_eq!(serde_json::to_string(&data).unwrap(), legacy);

            let mut on = data.clone();
            on.wallets[0].weekly_allowance = Some(enabled());
            let json = serde_json::to_string(&on).unwrap();
            assert!(json.contains(r#""weekly_allowance":{"amount_cents":500,"enabled_on":"2026-09-01","last_posted":"2026-09-01"}"#));
            let back: AppData = serde_json::from_str(&json).unwrap();
            assert_eq!(back.wallets[0].weekly_allowance, Some(enabled()));
        }
    }
}
