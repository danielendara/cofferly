use std::fs;
use std::path::PathBuf;

use crate::data::Wallet;
use crate::money::format_money;

pub fn write_printable_ledger(path: &PathBuf, wallets: &[Wallet]) -> Result<PathBuf, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let mut body = String::new();
    for wallet in wallets {
        body.push_str(&format!(
            "<section><h1>{}</h1><p class=\"balance\">Current balance: {}</p>",
            escape_html(&wallet.child_name),
            format_money(wallet.current_balance_cents())
        ));
        body.push_str(
            "<table><thead><tr><th>Date</th><th>Description</th><th>Amount</th><th>Balance</th></tr></thead><tbody>",
        );
        body.push_str(&format!(
            "<tr><td>Start</td><td>Starting balance</td><td>{}</td><td>{}</td></tr>",
            format_money(wallet.starting_balance_cents),
            format_money(wallet.starting_balance_cents)
        ));

        for (entry, balance) in wallet.rows_with_balance() {
            body.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td class=\"{}\">{}</td><td>{}</td></tr>",
                entry.date.format("%m/%d/%Y"),
                escape_html(&entry.description),
                if entry.amount_cents < 0 {
                    "minus"
                } else {
                    "plus"
                },
                format_money(entry.amount_cents),
                format_money(balance)
            ));
        }

        body.push_str("</tbody></table></section>");
    }

    let html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Atlas Wallet Ledger</title>
<style>
body {{ font-family: "Segoe UI", Arial, sans-serif; color: #1d2528; margin: 36px; }}
section {{ break-after: page; margin-bottom: 40px; }}
h1 {{ font-size: 34px; margin: 0 0 6px; }}
.balance {{ font-size: 20px; font-weight: 700; margin: 0 0 18px; }}
table {{ width: 100%; border-collapse: collapse; font-size: 14px; }}
th, td {{ border: 1px solid #9aa7ad; padding: 8px 10px; text-align: left; }}
th {{ background: #e9f1f4; }}
td:last-child, th:last-child, td:nth-child(3), th:nth-child(3) {{ text-align: right; }}
.plus {{ color: #1d7656; font-weight: 700; }}
.minus {{ color: #b03030; font-weight: 700; }}
@media print {{ body {{ margin: 0.45in; }} button {{ display: none; }} section:last-child {{ break-after: auto; }} }}
</style>
</head>
<body>
<button onclick="window.print()">Print</button>
{}
<script>setTimeout(() => window.print(), 350);</script>
</body>
</html>"#,
        body
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
}
