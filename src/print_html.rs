use std::fs;
use std::path::PathBuf;

use crate::data::{ledger_filter_summary, LedgerSort, Wallet};
use crate::money::format_money;

/// Write a printable HTML ledger to `path` (typically under the OS temp directory).
pub fn write_printable_ledger(path: &PathBuf, wallets: &[Wallet]) -> Result<PathBuf, String> {
    write_printable_ledger_filtered(path, wallets, "")
}

/// Like [`write_printable_ledger`], but a non-empty `description_filter` keeps
/// only the rows [`ledger_filter_summary`] would show (including the start
/// row). Order stays oldest-first. Running balances are the full ledger's.
pub fn write_printable_ledger_filtered(
    path: &PathBuf,
    wallets: &[Wallet],
    description_filter: &str,
) -> Result<PathBuf, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let mut body = String::new();
    for wallet in wallets {
        let header = format!(
            "<section><h1>{}</h1><p class=\"balance\">Current balance: {}</p>",
            escape_html(&wallet.child_name),
            format_money(wallet.current_balance_cents())
        );
        body.push_str(&header);

        body.push_str(
            "<table><thead><tr><th>Date</th><th>Description</th><th>Amount</th><th>Balance</th></tr></thead><tbody>",
        );

        let owned_rows = wallet.ledger_rows_sorted_owned(LedgerSort::OldestFirst);
        let summary = ledger_filter_summary(&owned_rows, description_filter);
        for ledger_row in summary.rows {
            let row = format!(
                "<tr><td>{}</td><td>{}</td><td class=\"{}\">{}</td><td>{}</td></tr>",
                ledger_row.date.label(),
                escape_html(&ledger_row.description),
                if ledger_row.amount_cents < 0 {
                    "minus"
                } else {
                    "plus"
                },
                format_money(ledger_row.amount_cents),
                format_money(ledger_row.balance_cents)
            );
            body.push_str(&row);
        }

        body.push_str("</tbody></table></section>");
    }

    let html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Cofferly Ledger</title>
<style>
body {{ font-family: "Segoe UI", Arial, sans-serif; color: #22333b; margin: 36px; }}
section {{ break-after: page; margin-bottom: 40px; }}
h1 {{ font-size: 34px; margin: 0 0 6px; }}
.balance {{ font-size: 20px; font-weight: 700; margin: 0 0 18px; }}
table {{ width: 100%; border-collapse: collapse; font-size: 14px; }}
th, td {{ border: 1px solid #9aa7ad; padding: 8px 10px; text-align: left; }}
th {{ background: #e4f3ef; }}
td:last-child, th:last-child, td:nth-child(3), th:nth-child(3) {{ text-align: right; }}
.plus {{ color: #18794e; font-weight: 700; }}
.minus {{ color: #ae373f; font-weight: 700; }}
@media print {{ body {{ margin: 0.45in; }} button {{ display: none; }} section:last-child {{ break-after: auto; }} }}
</style>
</head>
<body>
<button onclick="window.print()">Print</button>
{body}
<script>setTimeout(() => window.print(), 350);</script>
</body>
</html>"#
    );

    fs::write(path, html).map_err(|err| err.to_string())?;
    Ok(path.clone())
}

pub fn ledger_file_stem(child_name: &str) -> String {
    let mut stem = String::new();
    let mut last_was_separator = false;

    for character in child_name.trim().chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            stem.push(character);
            last_was_separator = false;
        } else if !last_was_separator && !stem.is_empty() {
            stem.push('-');
            last_was_separator = true;
        }
    }

    while stem.ends_with('-') {
        stem.pop();
    }

    if stem.is_empty() {
        "wallet".to_owned()
    } else {
        stem.chars().take(48).collect()
    }
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Entry;
    use chrono::NaiveDate;
    use tempfile::tempdir;

    #[test]
    fn creates_safe_ledger_file_stems() {
        assert_eq!(ledger_file_stem("A/B: Kid?"), "a-b-kid");
        assert_eq!(ledger_file_stem("   "), "wallet");
        assert_eq!(ledger_file_stem("Jane & Sam"), "jane-sam");
    }

    #[test]
    fn escapes_printable_html() {
        assert_eq!(
            escape_html("Game & Book <gift>"),
            "Game &amp; Book &lt;gift&gt;"
        );
    }

    #[test]
    fn writes_complete_printable_ledger_with_escaped_family_data() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("ledger.html");
        let wallets = vec![Wallet {
            child_name: "Alex & Sam".to_owned(),
            starting_balance_cents: 2_000,
            entries: vec![Entry {
                date: NaiveDate::from_ymd_opt(2026, 7, 11).unwrap(),
                description: "Book <sale>".to_owned(),
                amount_cents: -750,
            }],
        }];

        let written = write_printable_ledger(&path, &wallets).unwrap();
        let html = std::fs::read_to_string(&written).unwrap();

        assert_eq!(written, path);
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("Alex &amp; Sam"));
        assert!(html.contains("Book &lt;sale&gt;"));
        assert!(html.contains("-$7.50"));
        assert!(html.contains("$12.50"));
        assert!(html.contains("window.print()"));
    }

    #[test]
    fn filtered_printable_ledger_keeps_start_row_and_full_running_balance() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ledger.html");
        let wallets = sample_wallets();

        let written = write_printable_ledger_filtered(&path, &wallets[..1], "snack").unwrap();
        let html = std::fs::read_to_string(&written).unwrap();

        assert!(html.contains("Starting balance"));
        assert!(html.contains("Snack early"));
        assert!(html.contains("Snack late"));
        assert!(!html.contains("Weekly allowance"));
        // Full-ledger balance after the later snack, not a balance recomputed
        // from the filtered rows alone ($10.00 - $2.00 - $1.00 = $7.00).
        assert!(html.contains("Snack late</td><td class=\"minus\">-$1.00</td><td>$17.00</td>"));
        assert!(!html.contains("$7.00"));
        assert!(html.find("Snack early").unwrap() < html.find("Snack late").unwrap());
        // Other wallets are only included when the caller passes them.
        assert!(!html.contains("Child 2"));

        let all = dir.path().join("all.html");
        let all_html =
            std::fs::read_to_string(write_printable_ledger(&all, &wallets).unwrap()).unwrap();
        assert!(all_html.contains("Weekly allowance"));
        assert!(all_html.contains("Child 2"));
        assert!(all_html.contains("Bus fare"));
    }

    fn sample_wallets() -> Vec<Wallet> {
        vec![
            Wallet {
                child_name: "Child 1".to_owned(),
                starting_balance_cents: 1_000,
                entries: vec![
                    Entry {
                        date: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
                        description: "Snack early".to_owned(),
                        amount_cents: -200,
                    },
                    Entry {
                        date: NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
                        description: "Weekly allowance".to_owned(),
                        amount_cents: 1_000,
                    },
                    Entry {
                        date: NaiveDate::from_ymd_opt(2026, 6, 10).unwrap(),
                        description: "Snack late".to_owned(),
                        amount_cents: -100,
                    },
                ],
            },
            Wallet {
                child_name: "Child 2".to_owned(),
                starting_balance_cents: 0,
                entries: vec![Entry {
                    date: NaiveDate::from_ymd_opt(2026, 6, 2).unwrap(),
                    description: "Bus fare".to_owned(),
                    amount_cents: -300,
                }],
            },
        ]
    }
}
