use chrono::{Local, NaiveDate};
use eframe::egui;
use eframe::egui::{Color32, RichText};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const APP_NAME: &str = "Atlas Wallet";
const DATA_FILE_NAME: &str = "atlas-wallet-data.json";
const LEGACY_APP_NAME: &str = "TallyNest";
const LEGACY_DATA_FILE_NAME: &str = "tallynest-data.json";
const AIRWALLET_LEGACY_APP_NAME: &str = "AirWallet";
const AIRWALLET_LEGACY_DATA_FILE_NAME: &str = "airwallet-data.json";
const CHILDREN: [&str; 2] = ["Child 1", "Child 2"];
const DEFAULT_PARENT_PIN: &str = "1234";
const PIN_LENGTH: usize = 4;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1080.0, 720.0])
            .with_min_inner_size([820.0, 560.0])
            .with_title(APP_NAME)
            .with_app_id("com.atlaswallet.app")
            .with_icon(app_icon()),
        ..Default::default()
    };

    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(AtlasWalletApp::new(cc)))),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LedgerSort {
    NewestFirst,
    OldestFirst,
}

impl LedgerSort {
    fn toggle(&mut self) {
        *self = match self {
            Self::NewestFirst => Self::OldestFirst,
            Self::OldestFirst => Self::NewestFirst,
        };
    }
}

struct AtlasWalletApp {
    data: AppData,
    selected_wallet: usize,
    ledger_sort: LedgerSort,
    draft: EntryDraft,
    starting_balance_input: String,
    child_name_input: String,
    new_child_name_input: String,
    pin_digits: [String; PIN_LENGTH],
    pending_pin_focus: Option<usize>,
    new_pin_input: String,
    parent_unlocked: bool,
    status: String,
    data_path: PathBuf,
}

impl AtlasWalletApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_style(&cc.egui_ctx);

        let data_path = data_path();
        let data = load_app_data_with_legacy(&data_path, &legacy_data_path())
            .unwrap_or_else(default_app_data);

        Self {
            data,
            selected_wallet: 0,
            ledger_sort: LedgerSort::NewestFirst,
            draft: EntryDraft {
                description: String::new(),
                amount: String::new(),
                kind: EntryKind::Deduction,
            },
            starting_balance_input: String::new(),
            child_name_input: String::new(),
            new_child_name_input: String::new(),
            pin_digits: Default::default(),
            pending_pin_focus: Some(0),
            new_pin_input: String::new(),
            parent_unlocked: false,
            status: "Enter the parent PIN to unlock Atlas Wallet.".to_owned(),
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
        if self.entered_parent_pin() == self.data.parent_pin {
            self.parent_unlocked = true;
            self.clear_pin_digits();
            self.status = "Parent mode unlocked.".to_owned();
        } else {
            self.clear_pin_digits();
            self.status = "Wrong PIN. Try again.".to_owned();
        }
    }

    fn lock_parent(&mut self) {
        self.parent_unlocked = false;
        self.clear_pin_digits();
        self.status = "Locked. Enter the parent PIN to make changes.".to_owned();
    }

    fn entered_parent_pin(&self) -> String {
        self.pin_digits.concat()
    }

    fn clear_pin_digits(&mut self) {
        for digit in &mut self.pin_digits {
            digit.clear();
        }
        self.pending_pin_focus = Some(0);
    }

    fn parent_pin_complete(&self) -> bool {
        self.pin_digits.iter().all(|digit| digit.len() == 1)
    }

    fn normalize_pin_digit_input(&mut self, index: usize) {
        let digits: Vec<char> = self.pin_digits[index]
            .chars()
            .filter(|character| character.is_ascii_digit())
            .collect();

        if digits.is_empty() {
            self.pin_digits[index].clear();
            self.pending_pin_focus = Some(index);
            return;
        }

        if digits.len() == 1 {
            self.pin_digits[index] = digits[0].to_string();
            if index + 1 < PIN_LENGTH {
                self.pending_pin_focus = Some(index + 1);
            }
            return;
        }

        let mut last_filled = index;
        for (offset, digit) in digits.into_iter().enumerate() {
            let target = index + offset;
            if target >= PIN_LENGTH {
                break;
            }

            self.pin_digits[target] = digit.to_string();
            last_filled = target;
        }

        self.pending_pin_focus = Some((last_filled + 1).min(PIN_LENGTH - 1));
    }

    fn update_pin(&mut self) {
        if !valid_pin(&self.new_pin_input) {
            self.status = "Choose exactly 4 digits for the parent PIN.".to_owned();
            return;
        }

        self.data.parent_pin = self.new_pin_input.clone();
        self.new_pin_input.clear();
        self.save_with_success("Parent PIN updated.");
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

        let action = match self.draft.kind {
            EntryKind::Deposit => "Added",
            EntryKind::Deduction => "Deducted",
        };
        let signed_amount = match self.draft.kind {
            EntryKind::Deposit => amount,
            EntryKind::Deduction => -amount,
        };
        let wallet_name = self.selected_wallet().child_name.clone();
        let status = format!(
            "{action} {} for {}: {description}.",
            format_money(amount),
            wallet_name
        );

        self.selected_wallet_mut().entries.push(Entry {
            date: Local::now().date_naive(),
            description,
            amount_cents: signed_amount,
        });

        self.draft.description.clear();
        self.draft.amount.clear();
        self.save_with_success(status);
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

        let wallet_name = self.selected_wallet().child_name.clone();
        self.selected_wallet_mut().starting_balance_cents = balance;
        self.starting_balance_input.clear();
        self.save_with_success(format!(
            "Updated {} starting balance to {}.",
            wallet_name,
            format_money(balance)
        ));
    }

    fn rename_selected_child(&mut self) {
        if !self.parent_unlocked {
            self.status = "Unlock parent mode before renaming wallets.".to_owned();
            return;
        }

        let name = self.child_name_input.trim().to_owned();
        if !valid_child_name(&name) {
            self.status = "Use a child name between 1 and 40 characters.".to_owned();
            return;
        }

        let old_name = self.selected_wallet().child_name.clone();
        self.selected_wallet_mut().child_name = name.clone();
        self.child_name_input.clear();
        self.save_with_success(format!("Renamed {old_name} to {name}."));
    }

    fn add_child_wallet(&mut self) {
        if !self.parent_unlocked {
            self.status = "Unlock parent mode before adding wallets.".to_owned();
            return;
        }

        let name = self.new_child_name_input.trim().to_owned();
        if !valid_child_name(&name) {
            self.status = "Use a child name between 1 and 40 characters.".to_owned();
            return;
        }

        self.data.wallets.push(Wallet {
            child_name: name.clone(),
            starting_balance_cents: 0,
            entries: Vec::new(),
        });
        self.selected_wallet = self.data.wallets.len() - 1;
        self.new_child_name_input.clear();
        self.save_with_success(format!("Added wallet for {name}."));
    }

    fn remove_latest_entry(&mut self) {
        if !self.parent_unlocked {
            self.status = "Unlock parent mode before removing entries.".to_owned();
            return;
        }

        let wallet_name = self.selected_wallet().child_name.clone();
        if let Some(entry) = self.selected_wallet_mut().entries.pop() {
            self.save_with_success(format!(
                "Removed latest entry from {}: {} {}.",
                wallet_name,
                format_money(entry.amount_cents),
                entry.description
            ));
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
            "atlas-wallet-ledgers.html".to_owned()
        } else {
            format!(
                "atlas-wallet-{}-ledger.html",
                ledger_file_stem(&self.selected_wallet().child_name)
            )
        };

        self.data_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(file_name)
    }

    fn save_with_success(&mut self, success_status: impl Into<String>) {
        match save_app_data(&self.data_path, &self.data) {
            Ok(()) => self.status = success_status.into(),
            Err(err) => self.status = format!("Could not save: {err}"),
        }
    }
}

impl eframe::App for AtlasWalletApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !self.parent_unlocked {
            self.lock_screen(ui);
            return;
        }

        egui::Panel::top("header").show_inside(ui, |ui| {
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

        egui::Panel::left("wallet_picker")
            .resizable(false)
            .min_size(210.0)
            .show_inside(ui, |ui| {
                ui.add_space(10.0);
                ui.label(RichText::new("Wallets").strong().size(18.0));
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
                        .add_sized([180.0, 66.0], egui::Button::selectable(selected, label))
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

        egui::CentralPanel::default().show_inside(ui, |ui| {
            self.wallet_header(ui);
            ui.add_space(14.0);
            self.quick_actions(ui);
            ui.add_space(10.0);
            self.wallet_settings(ui);
            ui.add_space(10.0);
            self.balance_tools(ui);
            ui.add_space(10.0);
            self.entry_form(ui);
            ui.add_space(18.0);
            self.ledger_table(ui);
        });
    }
}

impl AtlasWalletApp {
    fn lock_screen(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.label(RichText::new(APP_NAME).size(42.0).strong());
                ui.label(RichText::new("Parent PIN required").size(22.0));
                ui.add_space(14.0);
                ui.label("Balances and ledgers stay private until a parent unlocks the app.");
                ui.add_space(24.0);

                ui.label(RichText::new("Enter 4 digit parent PIN").small().strong());
                ui.add_space(8.0);

                if let Some(index) = self.pending_pin_focus.take() {
                    ui.memory_mut(|memory| memory.request_focus(pin_digit_id(index)));
                }

                let enter_pressed = ui.input(|input| input.key_pressed(egui::Key::Enter));

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    let pin_entry_width = PIN_LENGTH as f32 * 64.0 + (PIN_LENGTH - 1) as f32 * 12.0;
                    ui.add_space(((ui.available_width() - pin_entry_width) / 2.0).max(0.0));

                    for index in 0..PIN_LENGTH {
                        let response = ui.add_sized(
                            [64.0, 68.0],
                            egui::TextEdit::singleline(&mut self.pin_digits[index])
                                .id(pin_digit_id(index))
                                .password(true)
                                .font(egui::TextStyle::Heading)
                                .horizontal_align(egui::Align::Center)
                                .vertical_align(egui::Align::Center)
                                .char_limit(PIN_LENGTH)
                                .desired_width(64.0),
                        );

                        if response.changed() {
                            self.normalize_pin_digit_input(index);
                            ui.ctx().request_repaint();
                        }

                        if response.has_focus()
                            && self.pin_digits[index].is_empty()
                            && ui.input(|input| input.key_pressed(egui::Key::Backspace))
                            && index > 0
                        {
                            self.pending_pin_focus = Some(index - 1);
                            ui.ctx().request_repaint();
                        }
                    }
                });

                if self.parent_pin_complete() && enter_pressed {
                    self.unlock_parent();
                }

                if ui
                    .add_sized([180.0, 38.0], egui::Button::new("Unlock"))
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

    fn wallet_settings(&mut self, ui: &mut egui::Ui) {
        let selected_child_name = self.selected_wallet().child_name.clone();

        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Child names").strong());
                ui.label("Rename selected");
                ui.add_sized(
                    [170.0, 24.0],
                    egui::TextEdit::singleline(&mut self.child_name_input)
                        .hint_text(selected_child_name),
                );
                if ui.button("Rename").clicked() {
                    self.rename_selected_child();
                }

                ui.separator();

                ui.label("Add wallet");
                ui.add_sized(
                    [170.0, 24.0],
                    egui::TextEdit::singleline(&mut self.new_child_name_input)
                        .hint_text("Child name"),
                );
                if ui.button("Add child").clicked() {
                    self.add_child_wallet();
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
        let ledger_sort = self.ledger_sort;
        let wallet = self.selected_wallet();
        let rows = wallet.rows_with_balance_sorted(ledger_sort);
        let mut toggle_sort = false;

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
                    let label = match ledger_sort {
                        LedgerSort::NewestFirst => "Date v",
                        LedgerSort::OldestFirst => "Date ^",
                    };
                    let tooltip = match ledger_sort {
                        LedgerSort::NewestFirst => "Newest entries first. Click for oldest first.",
                        LedgerSort::OldestFirst => "Oldest entries first. Click for newest first.",
                    };
                    if ui
                        .small_button(RichText::new(label).strong())
                        .on_hover_text(tooltip)
                        .clicked()
                    {
                        toggle_sort = true;
                    }
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

        if toggle_sort {
            self.ledger_sort.toggle();
        }
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

    fn rows_with_balance_sorted(&self, sort: LedgerSort) -> Vec<(&Entry, i64)> {
        let mut rows: Vec<_> = self.rows_with_balance().into_iter().enumerate().collect();

        rows.sort_by(
            |(left_index, (left_entry, _)), (right_index, (right_entry, _))| {
                let chronological = left_entry
                    .date
                    .cmp(&right_entry.date)
                    .then_with(|| left_index.cmp(right_index));

                match sort {
                    LedgerSort::NewestFirst => chronological.reverse(),
                    LedgerSort::OldestFirst => chronological,
                }
            },
        );

        rows.into_iter().map(|(_, row)| row).collect()
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

    base.join(APP_NAME).join(DATA_FILE_NAME)
}

fn legacy_data_path() -> PathBuf {
    let base = dirs::data_local_dir()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    base.join(LEGACY_APP_NAME).join(LEGACY_DATA_FILE_NAME)
}

fn load_app_data_with_legacy(path: &PathBuf, legacy_path: &PathBuf) -> Option<AppData> {
    if path.exists() {
        return load_app_data(path);
    }

    if let Some(data) = load_app_data(legacy_path) {
        let _ = save_app_data(path, &data);
        return Some(data);
    }

    // Ultimate fallback for installs that still have the original AirWallet data
    let airwallet_base = dirs::data_local_dir()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let airwallet_path = airwallet_base
        .join(AIRWALLET_LEGACY_APP_NAME)
        .join(AIRWALLET_LEGACY_DATA_FILE_NAME);
    if let Some(data) = load_app_data(&airwallet_path) {
        let _ = save_app_data(path, &data);
        return Some(data);
    }

    None
}

fn load_app_data(path: &PathBuf) -> Option<AppData> {
    let contents = fs::read_to_string(path).ok()?;

    if let Ok(data) = serde_json::from_str::<AppData>(&contents) {
        return normalize_app_data(data);
    }

    let wallets = serde_json::from_str::<Vec<Wallet>>(&contents).ok()?;
    normalize_app_data(AppData {
        parent_pin: DEFAULT_PARENT_PIN.to_owned(),
        wallets,
    })
}

fn normalize_app_data(mut data: AppData) -> Option<AppData> {
    if data.wallets.is_empty() {
        return None;
    }

    if !valid_pin(&data.parent_pin) {
        data.parent_pin = DEFAULT_PARENT_PIN.to_owned();
    }

    Some(data)
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

fn parse_dollars_to_cents(input: &str) -> Result<i64, String> {
    let trimmed = input.trim();
    let trimmed = trimmed.strip_prefix('$').unwrap_or(trimmed).trim();
    if trimmed.is_empty() {
        return Err("Enter a dollar amount.".to_owned());
    }

    let (negative, amount) = match trimmed.strip_prefix('-') {
        Some(amount) => (true, amount),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };

    if amount.is_empty() {
        return Err("Enter a dollar amount.".to_owned());
    }

    let (dollars, cents) = match amount.split_once('.') {
        Some((dollars, cents)) => {
            if cents.contains('.') {
                return Err("Use only one decimal point.".to_owned());
            }
            (dollars, cents)
        }
        None => (amount, "0"),
    };

    if dollars.is_empty() && cents.is_empty() {
        return Err("Enter a dollar amount.".to_owned());
    }

    if !dollars.chars().all(|character| character.is_ascii_digit())
        || !cents.chars().all(|character| character.is_ascii_digit())
    {
        return Err("Use digits and at most one decimal point.".to_owned());
    }

    let dollars = if dollars.is_empty() {
        0
    } else {
        dollars.parse::<i64>().map_err(|err| err.to_string())?
    };
    let cents = match cents.len() {
        0 => 0,
        1 => cents.parse::<i64>().map_err(|err| err.to_string())? * 10,
        2 => cents.parse::<i64>().map_err(|err| err.to_string())?,
        _ => return Err("Use at most two decimal places.".to_owned()),
    };

    let amount = dollars
        .checked_mul(100)
        .and_then(|dollars| dollars.checked_add(cents))
        .ok_or_else(|| "Amount is too large.".to_owned())?;

    if negative {
        amount
            .checked_neg()
            .ok_or_else(|| "Amount is too large.".to_owned())
    } else {
        Ok(amount)
    }
}

fn valid_pin(pin: &str) -> bool {
    pin.len() == 4 && pin.chars().all(|character| character.is_ascii_digit())
}

fn pin_digit_id(index: usize) -> egui::Id {
    egui::Id::new(("parent_pin_digit", index))
}

fn valid_child_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 40
}

fn ledger_file_stem(child_name: &str) -> String {
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
    let mut style = (*ctx.global_style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(36, 87, 122);
    style.visuals.selection.bg_fill = Color32::from_rgb(36, 87, 122);
    ctx.set_global_style(style);
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
    fn parses_flexible_money_inputs() {
        assert_eq!(parse_dollars_to_cents("10.").unwrap(), 1000);
        assert_eq!(parse_dollars_to_cents(".50").unwrap(), 50);
        assert_eq!(parse_dollars_to_cents("$ 10").unwrap(), 1000);
        assert_eq!(parse_dollars_to_cents("-10.50").unwrap(), -1050);
        assert_eq!(parse_dollars_to_cents("-.50").unwrap(), -50);
    }

    #[test]
    fn rejects_invalid_money_inputs() {
        assert!(parse_dollars_to_cents("").is_err());
        assert!(parse_dollars_to_cents("$").is_err());
        assert!(parse_dollars_to_cents("10.999").is_err());
        assert!(parse_dollars_to_cents("10.1.1").is_err());
        assert!(parse_dollars_to_cents("ten").is_err());
    }

    #[test]
    fn rejects_money_inputs_that_overflow_cents() {
        assert!(parse_dollars_to_cents("92233720368547758.08").is_err());
    }

    #[test]
    fn validates_four_digit_pin() {
        assert!(valid_pin("1234"));
        assert!(!valid_pin("123"));
        assert!(!valid_pin("12a4"));
    }

    #[test]
    fn validates_child_names() {
        assert!(valid_child_name("Child 1"));
        assert!(!valid_child_name(""));
        assert!(!valid_child_name("   "));
        assert!(!valid_child_name(
            "This name is too long for the Atlas Wallet sidebar"
        ));
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
    fn imports_legacy_data_when_new_data_does_not_exist() {
        let test_dir = std::env::temp_dir().join(format!(
            "atlas-wallet-migration-test-{}",
            std::process::id()
        ));
        let new_path = test_dir.join(APP_NAME).join(DATA_FILE_NAME);
        let legacy_path = test_dir.join(LEGACY_APP_NAME).join(LEGACY_DATA_FILE_NAME);
        let data = default_app_data();

        save_app_data(&legacy_path, &data).unwrap();

        let loaded = load_app_data_with_legacy(&new_path, &legacy_path).unwrap();

        assert_eq!(loaded.wallets.len(), data.wallets.len());
        assert!(new_path.exists());

        fs::remove_dir_all(test_dir).unwrap();
    }

    #[test]
    fn does_not_replace_invalid_new_data_with_legacy_data() {
        let test_dir = std::env::temp_dir().join(format!(
            "atlas-wallet-invalid-data-test-{}",
            std::process::id()
        ));
        let new_path = test_dir.join(APP_NAME).join(DATA_FILE_NAME);
        let legacy_path = test_dir.join(LEGACY_APP_NAME).join(LEGACY_DATA_FILE_NAME);

        fs::create_dir_all(new_path.parent().unwrap()).unwrap();
        fs::write(&new_path, "invalid data").unwrap();
        save_app_data(&legacy_path, &default_app_data()).unwrap();

        assert!(load_app_data_with_legacy(&new_path, &legacy_path).is_none());
        assert_eq!(fs::read_to_string(&new_path).unwrap(), "invalid data");

        fs::remove_dir_all(test_dir).unwrap();
    }

    #[test]
    fn creates_safe_ledger_file_stems() {
        assert_eq!(ledger_file_stem("A/B: Kid?"), "a-b-kid");
        assert_eq!(ledger_file_stem("   "), "wallet");
        assert_eq!(ledger_file_stem("Jane & Sam"), "jane-sam");
    }

    #[test]
    fn formats_money() {
        assert_eq!(format_money(19400), "$194.00");
        assert_eq!(format_money(-500), "-$5.00");
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

        let rows = wallet.rows_with_balance_sorted(LedgerSort::NewestFirst);
        let descriptions: Vec<_> = rows
            .iter()
            .map(|(entry, _)| entry.description.as_str())
            .collect();
        let balances: Vec<_> = rows.iter().map(|(_, balance)| *balance).collect();

        assert_eq!(descriptions, ["Latest", "Second", "First"]);
        assert_eq!(balances, [1400, 1300, 1500]);
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

        let rows = wallet.rows_with_balance_sorted(LedgerSort::OldestFirst);
        let descriptions: Vec<_> = rows
            .iter()
            .map(|(entry, _)| entry.description.as_str())
            .collect();

        assert_eq!(descriptions, ["First", "Second"]);
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
