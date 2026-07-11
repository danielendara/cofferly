use std::fs;
use std::path::PathBuf;

use crate::data::{LedgerSort, Wallet};
use crate::money::format_money;

pub fn write_printable_ledger(path: &PathBuf, wallets: &[Wallet]) -> Result<PathBuf, String> {
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

        for ledger_row in wallet.ledger_rows_sorted(LedgerSort::OldestFirst) {
            let row = format!(
                "<tr><td>{}</td><td>{}</td><td class=\"{}\">{}</td><td>{}</td></tr>",
                ledger_row.date.label(),
                escape_html(ledger_row.description),
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
}
