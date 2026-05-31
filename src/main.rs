use chrono::{Local, NaiveDate};
use eframe::egui;
use eframe::egui::{Color32, RichText};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const APP_NAME: &str = "AirWallet";
const CHILDREN: [&str; 2] = ["Child 1", "Child 2"];
const DEFAULT_PARENT_PIN: &str = "1234";

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1080.0, 720.0])
            .with_min_inner_size([820.0, 560.0])
            .with_title(APP_NAME)
            .with_app_id("com.airwallet.app")
            .with_icon(app_icon()),
        ..Default::default()
    };

    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| Box::new(AirWalletApp::new(cc))),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppData {
    parent_pin: String,
    wallets: Vec<Wallet>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Wallet {
    child_name: String,
    starting_balance_cents: i64,
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    date: NaiveDate,
    description: String,
    amount_cents: i64,
}

#[derive(Debug, Clone)]
struct EntryDraft {
    description: String,
    amount: String,
    kind: EntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    Deposit,
    Deduction,
}

struct AirWalletApp {
    data: AppData,
    selected_wallet: usize,
    draft: EntryDraft,
    starting_balance_input: String,
    pin_input: String,
    new_pin_input: String,
    parent_unlocked: bool,
    status: String,
    data_path: PathBuf,
}

impl AirWalletApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_style(&cc.egui_ctx);

        let data_path = data_path();
        let data = load_app_data(&data_path).unwrap_or_else(default_app_data);

        Self {
            data,
            selected_wallet: 0,
            draft: EntryDraft {
                description: String::new(),
                amount: String::new(),
                kind: EntryKind::Deduction,
            },
            starting_balance_input: String::new(),
            pin_input: String::new(),
            new_pin_input: String::new(),
            parent_unlocked: false,
            status: "Enter the parent PIN to unlock AirWallet.".to_owned(),
            data_path,
        }
    }

    fn selected_wallet(&self) -> &Wallet {
        &self.data.wallets[self.selected_wallet]
    }

    fn selected_wallet_mut(&mut self) -> &mut Wallet {
        &mut self.data.wallets[self.selected_wallet]
    }

    fn unlock_parent(&mut self) {
        if self.pin_input == self.data.parent_pin {
            self.parent_unlocked = true;
            self.pin_input.clear();
            self.status = "Parent mode unlocked.".to_owned();
        } else {
            self.pin_input.clear();
            self.status = "Wrong PIN. Try again.".to_owned();
        }
    }

    fn lock_parent(&mut self) {
        self.parent_unlocked = false;
        self.pin_input.clear();
        self.status = "Locked. Enter the parent PIN to make changes.".to_owned();
    }

    fn update_pin(&mut self) {
        if !valid_pin(&self.new_pin_input) {
            self.status = "Choose exactly 4 digits for the parent PIN.".to_owned();
            return;
        }

        self.data.parent_pin = self.new_pin_input.clone();
        self.new_pin_input.clear();
        self.save();
        self.status = "Parent PIN updated.".to_owned();
    }

    fn add_entry(&mut self) {
        if !self.parent_unlocked {
            self.status = "Unlock parent mode before adding entries.".to_owned();
            return;
        }

        let amount = match parse_dollars_to_cents(&self.draft.amount) {
            Ok(amount) if amount > 0 => amount,
            _ => {
                self.status = "Enter a valid amount, like 10 or 10.50.".to_owned();
                return;
            }
        };

        let description = self.draft.description.trim().to_owned();
        if description.is_empty() {
            self.status = "Add a short description first.".to_owned();
            return;
        }

        let signed_amount = match self.draft.kind {
            EntryKind::Deposit => amount,
            EntryKind::Deduction => -amount,
        };

        self.selected_wallet_mut().entries.push(Entry {
            date: Local::now().date_naive(),
            description,
            amount_cents: signed_amount,
        });

        self.draft.description.clear();
        self.draft.amount.clear();
        self.save();
    }

    fn quick_entry(&mut self, description: &str, amount_cents: i64, kind: EntryKind) {
        self.draft.description = description.to_owned();
        self.draft.amount = format_money_input(amount_cents);
        self.draft.kind = kind;
    }

    fn update_starting_balance(&mut self) {
        if !self.parent_unlocked {
            self.status = "Unlock parent mode before changing balances.".to_owned();
            return;
        }

        let balance = match parse_dollars_to_cents(&self.starting_balance_input) {
            Ok(balance) => balance,
            Err(_) => {
                self.status = "Enter a valid starting balance, like 90 or 90.00.".to_owned();
                return;
            }
        };

        self.selected_wallet_mut().starting_balance_cents = balance;
        self.starting_balance_input.clear();
        self.save();
    }

    fn remove_latest_entry(&mut self) {
        if !self.parent_unlocked {
            self.status = "Unlock parent mode before removing entries.".to_owned();
            return;
        }

        if self.selected_wallet_mut().entries.pop().is_some() {
            self.save();
        } else {
            self.status = "There are no entries to remove.".to_owned();
        }
    }

    fn print_selected_wallet(&mut self) {
        match write_printable_ledger(&self.print_path(false), &[self.selected_wallet().clone()]) {
            Ok(path) => self.open_printable_file(path),
            Err(err) => self.status = format!("Could not create printable ledger: {err}"),
        }
    }

    fn print_all_wallets(&mut self) {
        match write_printable_ledger(&self.print_path(true), &self.data.wallets) {
            Ok(path) => self.open_printable_file(path),
            Err(err) => self.status = format!("Could not create printable ledger: {err}"),
        }
    }

    fn open_printable_file(&mut self, path: PathBuf) {
        match opener::open(&path) {
            Ok(()) => self.status = format!("Opened printable ledger: {}", path.display()),
            Err(err) => {
                self.status = format!(
                    "Printable ledger saved to {}, but could not open it: {err}",
                    path.display()
                );
            }
        }
    }

    fn print_path(&self, all_wallets: bool) -> PathBuf {
        let file_name = if all_wallets {
            "airwallet-ledgers.html".to_owned()
        } else {
            format!(
                "airwallet-{}-ledger.html",
                self.selected_wallet().child_name.to_lowercase()
            )
        };

        self.data_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(file_name)
    }

    fn save(&mut self) {
        match save_app_data(&self.data_path, &self.data) {
            Ok(()) => self.status = format!("Saved to {}", self.data_path.display()),
            Err(err) => self.status = format!("Could not save: {err}"),
        }
    }
}

impl eframe::App for AirWalletApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.parent_unlocked {
            self.lock_screen(ctx);
            return;
        }

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new(APP_NAME).size(30.0));
                ui.separator();
                ui.label("Parent mode");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Lock").clicked() {
                        self.lock_parent();
                    }
                });
            });
            ui.add_space(8.0);
        });

        egui::SidePanel::left("wallet_picker")
            .resizable(false)
            .min_width(210.0)
            .show(ctx, |ui| {
                ui.add_space(10.0);
                ui.label(RichText::new("Twin Wallets").strong().size(18.0));
                ui.add_space(8.0);

                for index in 0..self.data.wallets.len() {
                    let wallet = &self.data.wallets[index];
                    let selected = self.selected_wallet == index;
                    let label = format!(
                        "{}\n{}",
                        wallet.child_name,
                        format_money(wallet.current_balance_cents())
                    );

                    if ui
                        .add_sized([180.0, 66.0], egui::SelectableLabel::new(selected, label))
                        .clicked()
                    {
                        self.selected_wallet = index;
                    }
                }

                ui.add_space(12.0);
                if ui.button("Print this ledger").clicked() {
                    self.print_selected_wallet();
                }
                if ui.button("Print both ledgers").clicked() {
                    self.print_all_wallets();
                }

                ui.separator();
                ui.label(RichText::new("Change PIN").strong());
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_pin_input)
                        .password(true)
                        .desired_width(110.0)
                        .hint_text("4 digits"),
                );
                if ui.button("Save PIN").clicked() {
                    self.update_pin();
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        RichText::new(&self.status)
                            .small()
                            .color(Color32::DARK_GRAY),
                    );
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.wallet_header(ui);
            ui.add_space(14.0);
            self.quick_actions(ui);
            ui.add_space(10.0);
            self.balance_tools(ui);
            ui.add_space(10.0);
            self.entry_form(ui);
            ui.add_space(18.0);
            self.ledger_table(ui);
        });
    }
}

impl AirWalletApp {
    fn lock_screen(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.label(RichText::new(APP_NAME).size(42.0).strong());
                ui.label(RichText::new("Parent PIN required").size(22.0));
                ui.add_space(14.0);
                ui.label("Balances and ledgers stay private until a parent unlocks the app.");
                ui.add_space(20.0);

                let response = ui.add_sized(
                    [180.0, 36.0],
                    egui::TextEdit::singleline(&mut self.pin_input)
                        .password(true)
                        .hint_text("4 digit PIN"),
                );

                if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    self.unlock_parent();
                }

                if ui
                    .add_sized([180.0, 36.0], egui::Button::new("Unlock"))
                    .clicked()
                {
                    self.unlock_parent();
                }

                ui.add_space(14.0);
                ui.label(RichText::new(&self.status).color(Color32::DARK_GRAY));
                ui.add_space(8.0);
                ui.label(RichText::new("First run default PIN: 1234").small());
            });
        });
    }

    fn wallet_header(&self, ui: &mut egui::Ui) {
        let wallet = self.selected_wallet();

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(&wallet.child_name).size(36.0).strong());
                ui.label("Running balance");
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format_money(wallet.current_balance_cents()))
                        .size(48.0)
                        .strong()
                        .color(balance_color(wallet.current_balance_cents())),
                );
            });
        });
    }

    fn quick_actions(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Quick add").strong());

                if ui.button("+$5").clicked() {
                    self.quick_entry("Allowance", 500, EntryKind::Deposit);
                }
                if ui.button("+$10").clicked() {
                    self.quick_entry("Allowance", 1000, EntryKind::Deposit);
                }
                if ui.button("+$20").clicked() {
                    self.quick_entry("Gift", 2000, EntryKind::Deposit);
                }
                if ui.button("+$50").clicked() {
                    self.quick_entry("Gift", 5000, EntryKind::Deposit);
                }

                ui.separator();

                if ui.button("-$5").clicked() {
                    self.quick_entry("Game purchase", 500, EntryKind::Deduction);
                }
                if ui.button("-$10").clicked() {
                    self.quick_entry("Purchase", 1000, EntryKind::Deduction);
                }
                if ui.button("-$15").clicked() {
                    self.quick_entry("Purchase", 1500, EntryKind::Deduction);
                }
            });
        });
    }

    fn balance_tools(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Starting balance").strong());
                ui.add_sized(
                    [110.0, 24.0],
                    egui::TextEdit::singleline(&mut self.starting_balance_input).hint_text("90.00"),
                );

                if ui.button("Update").clicked() {
                    self.update_starting_balance();
                }

                ui.separator();

                if ui.button("Remove latest entry").clicked() {
                    self.remove_latest_entry();
                }
            });
        });
    }

    fn entry_form(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("New entry").strong());

                ui.selectable_value(&mut self.draft.kind, EntryKind::Deposit, "Deposit");
                ui.selectable_value(&mut self.draft.kind, EntryKind::Deduction, "Deduction");

                ui.label("Description");
                ui.add_sized(
                    [280.0, 24.0],
                    egui::TextEdit::singleline(&mut self.draft.description)
                        .hint_text("Game, birthday, allowance"),
                );

                ui.label("Amount");
                ui.add_sized(
                    [110.0, 24.0],
                    egui::TextEdit::singleline(&mut self.draft.amount).hint_text("10.00"),
                );

                if ui.button("Add entry").clicked() {
                    self.add_entry();
                }
            });
        });
    }

    fn ledger_table(&mut self, ui: &mut egui::Ui) {
        let wallet = self.selected_wallet();
        let rows = wallet.rows_with_balance();

        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::initial(100.0).at_least(90.0))
            .column(egui_extras::Column::remainder().at_least(240.0))
            .column(egui_extras::Column::initial(120.0).at_least(90.0))
            .column(egui_extras::Column::initial(140.0).at_least(100.0))
            .header(30.0, |mut header| {
                header.col(|ui| {
                    ui.strong("Date");
                });
                header.col(|ui| {
                    ui.strong("Description");
                });
                header.col(|ui| {
                    ui.strong("Amount");
                });
                header.col(|ui| {
                    ui.strong("Balance");
                });
            })
            .body(|mut body| {
                body.row(30.0, |mut row| {
                    row.col(|ui| {
                        ui.label("Start");
                    });
                    row.col(|ui| {
                        ui.label("Starting balance");
                    });
                    row.col(|ui| {
                        ui.label(format_money(wallet.starting_balance_cents));
                    });
                    row.col(|ui| {
                        ui.strong(format_money(wallet.starting_balance_cents));
                    });
                });

                for (entry, balance) in rows {
                    body.row(30.0, |mut row| {
                        row.col(|ui| {
                            ui.label(entry.date.format("%m/%d/%Y").to_string());
                        });
                        row.col(|ui| {
                            ui.label(&entry.description);
                        });
                        row.col(|ui| {
                            ui.label(
                                RichText::new(format_money(entry.amount_cents))
                                    .color(amount_color(entry.amount_cents)),
                            );
                        });
                        row.col(|ui| {
                            ui.strong(format_money(balance));
                        });
                    });
                }
            });
    }
}

impl Wallet {
    fn current_balance_cents(&self) -> i64 {
        self.starting_balance_cents
            + self
                .entries
                .iter()
                .map(|entry| entry.amount_cents)
                .sum::<i64>()
    }

    fn rows_with_balance(&self) -> Vec<(&Entry, i64)> {
        let mut balance = self.starting_balance_cents;
        self.entries
            .iter()
            .map(|entry| {
                balance += entry.amount_cents;
                (entry, balance)
            })
            .collect()
    }
}

fn default_app_data() -> AppData {
    AppData {
        parent_pin: DEFAULT_PARENT_PIN.to_owned(),
        wallets: default_wallets(),
    }
}

fn default_wallets() -> Vec<Wallet> {
    CHILDREN
        .iter()
        .map(|name| Wallet {
            child_name: (*name).to_owned(),
            starting_balance_cents: 0,
            entries: Vec::new(),
        })
        .collect()
}

fn data_path() -> PathBuf {
    let base = dirs::data_local_dir()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    base.join(APP_NAME).join("airwallet-data.json")
}

fn load_app_data(path: &PathBuf) -> Option<AppData> {
    let contents = fs::read_to_string(path).ok()?;

    if let Ok(data) = serde_json::from_str::<AppData>(&contents) {
        return Some(data);
    }

    let wallets = serde_json::from_str::<Vec<Wallet>>(&contents).ok()?;
    Some(AppData {
        parent_pin: DEFAULT_PARENT_PIN.to_owned(),
        wallets,
    })
}

fn save_app_data(path: &PathBuf, data: &AppData) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let contents = serde_json::to_string_pretty(data).map_err(|err| err.to_string())?;
    fs::write(path, contents).map_err(|err| err.to_string())
}

fn write_printable_ledger(path: &PathBuf, wallets: &[Wallet]) -> Result<PathBuf, String> {
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
<title>AirWallet Ledger</title>
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

fn parse_dollars_to_cents(input: &str) -> Result<i64, String> {
    let trimmed = input.trim().trim_start_matches('$');
    let (dollars, cents) = match trimmed.split_once('.') {
        Some((dollars, cents)) => (dollars, cents),
        None => (trimmed, "0"),
    };

    let dollars = dollars.parse::<i64>().map_err(|err| err.to_string())?;
    let cents = match cents.len() {
        0 => 0,
        1 => cents.parse::<i64>().map_err(|err| err.to_string())? * 10,
        2 => cents.parse::<i64>().map_err(|err| err.to_string())?,
        _ => return Err("Use at most two decimal places.".to_owned()),
    };

    Ok(dollars * 100 + cents)
}

fn valid_pin(pin: &str) -> bool {
    pin.len() == 4 && pin.chars().all(|character| character.is_ascii_digit())
}

fn format_money(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let absolute = cents.abs();
    format!("{sign}${}.{:02}", absolute / 100, absolute % 100)
}

fn format_money_input(cents: i64) -> String {
    let absolute = cents.abs();
    format!("{}.{:02}", absolute / 100, absolute % 100)
}

fn balance_color(cents: i64) -> Color32 {
    if cents < 0 {
        Color32::from_rgb(176, 48, 48)
    } else {
        Color32::from_rgb(30, 110, 80)
    }
}

fn amount_color(cents: i64) -> Color32 {
    if cents < 0 {
        Color32::from_rgb(176, 48, 48)
    } else {
        Color32::from_rgb(30, 110, 80)
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

fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(36, 87, 122);
    style.visuals.selection.bg_fill = Color32::from_rgb(36, 87, 122);
    ctx.set_style(style);
}

fn app_icon() -> egui::IconData {
    const SIZE: u32 = 64;
    let mut rgba = vec![0_u8; (SIZE * SIZE * 4) as usize];

    for y in 0..SIZE {
        for x in 0..SIZE {
            let index = ((y * SIZE + x) * 4) as usize;
            let inside = rounded_rect(x, y, 5, 5, 54, 54, 12);

            if inside {
                rgba[index] = 31;
                rgba[index + 1] = 126;
                rgba[index + 2] = 108;
                rgba[index + 3] = 255;
            }

            if rounded_rect(x, y, 14, 18, 44, 30, 7) {
                rgba[index] = 246;
                rgba[index + 1] = 250;
                rgba[index + 2] = 248;
                rgba[index + 3] = 255;
            }

            if rounded_rect(x, y, 16, 25, 40, 24, 6) {
                rgba[index] = 227;
                rgba[index + 1] = 241;
                rgba[index + 2] = 236;
                rgba[index + 3] = 255;
            }

            if rounded_rect(x, y, 42, 30, 8, 8, 4) {
                rgba[index] = 31;
                rgba[index + 1] = 126;
                rgba[index + 2] = 108;
                rgba[index + 3] = 255;
            }

            if (20..=44).contains(&x) && (12..=16).contains(&y) {
                rgba[index] = 255;
                rgba[index + 1] = 214;
                rgba[index + 2] = 102;
                rgba[index + 3] = 255;
            }
        }
    }

    egui::IconData {
        rgba,
        width: SIZE,
        height: SIZE,
    }
}

fn rounded_rect(x: u32, y: u32, left: u32, top: u32, width: u32, height: u32, radius: u32) -> bool {
    if x < left || y < top || x >= left + width || y >= top + height {
        return false;
    }

    let right = left + width - 1;
    let bottom = top + height - 1;
    let cx = if x < left + radius {
        left + radius
    } else if x > right - radius {
        right - radius
    } else {
        x
    };
    let cy = if y < top + radius {
        top + radius
    } else if y > bottom - radius {
        bottom - radius
    } else {
        y
    };

    let dx = x as i64 - cx as i64;
    let dy = y as i64 - cy as i64;
    dx * dx + dy * dy <= (radius as i64) * (radius as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_whole_dollars() {
        assert_eq!(parse_dollars_to_cents("10").unwrap(), 1000);
    }

    #[test]
    fn parses_dollars_and_cents() {
        assert_eq!(parse_dollars_to_cents("$10.50").unwrap(), 1050);
    }

    #[test]
    fn validates_four_digit_pin() {
        assert!(valid_pin("1234"));
        assert!(!valid_pin("123"));
        assert!(!valid_pin("12a4"));
    }

    #[test]
    fn formats_money() {
        assert_eq!(format_money(19400), "$194.00");
        assert_eq!(format_money(-500), "-$5.00");
    }

    #[test]
    fn escapes_printable_html() {
        assert_eq!(
            escape_html("Game & Book <gift>"),
            "Game &amp; Book &lt;gift&gt;"
        );
    }

    #[test]
    fn app_icon_is_valid_rgba_square() {
        let icon = app_icon();
        assert_eq!(icon.width, 64);
        assert_eq!(icon.height, 64);
        assert_eq!(icon.rgba.len(), 64 * 64 * 4);
    }
}
