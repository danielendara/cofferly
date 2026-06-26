use chrono::Local;
use eframe::egui;
use eframe::egui::{Color32, RichText};
use std::path::PathBuf;

mod data;
mod io;
mod money;
mod print_html;
mod theme;

pub const APP_NAME: &str = "Cofferly";
pub const DATA_FILE_NAME: &str = "data.json";
pub const ATLAS_LEGACY_APP_NAME: &str = "Atlas Wallet";
pub const ATLAS_LEGACY_DATA_FILE_NAME: &str = "atlas-wallet-data.json";
pub const LEGACY_APP_NAME: &str = "TallyNest";
pub const LEGACY_DATA_FILE_NAME: &str = "tallynest-data.json";
pub const AIRWALLET_LEGACY_APP_NAME: &str = "AirWallet";
pub const AIRWALLET_LEGACY_DATA_FILE_NAME: &str = "airwallet-data.json";
const PIN_LENGTH: usize = 4;

use data::{
    default_app_data, valid_cents, valid_child_name, valid_pin, AppData, Entry, EntryKind,
    LedgerSort, Wallet,
};
use io::{data_path, load_app_data_with_legacy, save_app_data};
use money::{format_money, format_money_input, parse_dollars_to_cents};
use print_html::{ledger_file_stem, write_printable_ledger};
use theme::{amount_color, app_icon, balance_color, configure_style};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1080.0, 720.0])
            .with_min_inner_size([820.0, 560.0])
            .with_title(APP_NAME)
            .with_app_id("com.cofferly.app")
            .with_icon(app_icon()),
        ..Default::default()
    };

    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(CofferlyApp::new(cc)))),
    )
}

#[derive(Debug, Clone)]
struct EntryDraft {
    description: String,
    amount: String,
    kind: EntryKind,
}

struct CofferlyApp {
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
    save_enabled: bool,
    status: String,
    data_path: PathBuf,
}

impl CofferlyApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_style(&cc.egui_ctx);

        let data_path = data_path();
        let (data, save_enabled, status) = match load_app_data_with_legacy(&data_path) {
            Ok(Some(data)) => (
                data,
                true,
                "Enter the parent PIN to unlock Cofferly.".to_string(),
            ),
            Ok(None) => (
                default_app_data(),
                true,
                "Enter the parent PIN to unlock Cofferly.".to_string(),
            ),
            Err(err) => (
                default_app_data(),
                false,
                format!("Could not load saved data: {err}. Changes are disabled."),
            ),
        };

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
            save_enabled,
            status,
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
            self.status = "Parent mode unlocked.".to_string();
        } else {
            self.clear_pin_digits();
            self.status = "Wrong PIN. Try again.".to_string();
        }
    }

    fn lock_parent(&mut self) {
        self.parent_unlocked = false;
        self.clear_pin_digits();
        self.status = "Locked. Enter the parent PIN to make changes.".to_string();
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
            .filter(char::is_ascii_digit)
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
        if !self.can_change("Unlock parent mode before changing the PIN.") {
            return;
        }

        if !valid_pin(&self.new_pin_input) {
            self.status = "Choose exactly 4 digits for the parent PIN.".to_string();
            return;
        }

        self.data.parent_pin = std::mem::take(&mut self.new_pin_input);
        self.save_with_success("Parent PIN updated.");
    }

    fn add_entry(&mut self) {
        if !self.can_change("Unlock parent mode before adding entries.") {
            return;
        }

        let amount = match parse_dollars_to_cents(&self.draft.amount) {
            Ok(amount) if amount > 0 => amount,
            _ => {
                self.status = "Enter a valid amount, like 10 or 10.50.".to_string();
                return;
            }
        };
        if !valid_cents(amount) {
            self.status = "Enter a smaller amount.".to_string();
            return;
        }

        let description = self.draft.description.trim().to_owned();
        if description.is_empty() {
            self.status = "Add a short description first.".to_string();
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
        let mut updated_wallet = self.selected_wallet().clone();
        updated_wallet.entries.push(Entry {
            date: Local::now().date_naive(),
            description: description.clone(),
            amount_cents: signed_amount,
        });
        if !updated_wallet.balances_are_valid() {
            self.status =
                "That entry would put the wallet outside Cofferly's supported range.".to_string();
            return;
        }

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
        self.draft.description = description.to_string();
        self.draft.amount = format_money_input(amount_cents);
        self.draft.kind = kind;
    }

    fn update_starting_balance(&mut self) {
        if !self.can_change("Unlock parent mode before changing balances.") {
            return;
        }

        let Ok(balance) = parse_dollars_to_cents(&self.starting_balance_input) else {
            self.status = "Enter a valid starting balance, like 90 or 90.00.".to_string();
            return;
        };
        if !valid_cents(balance) {
            self.status = "Enter a smaller starting balance.".to_string();
            return;
        }
        let mut updated_wallet = self.selected_wallet().clone();
        updated_wallet.starting_balance_cents = balance;
        if !updated_wallet.balances_are_valid() {
            self.status =
                "That starting balance would put the wallet outside Cofferly's supported range."
                    .to_string();
            return;
        }

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
        if !self.can_change("Unlock parent mode before renaming wallets.") {
            return;
        }

        let name = self.child_name_input.trim().to_owned();
        if !valid_child_name(&name) {
            self.status = "Use a child name between 1 and 40 characters.".to_string();
            return;
        }

        let old_name = std::mem::take(&mut self.selected_wallet_mut().child_name);
        self.selected_wallet_mut().child_name = name;
        self.child_name_input.clear();
        self.save_with_success(format!(
            "Renamed {old_name} to {}.",
            self.selected_wallet().child_name
        ));
    }

    fn add_child_wallet(&mut self) {
        if !self.can_change("Unlock parent mode before adding wallets.") {
            return;
        }

        let name = self.new_child_name_input.trim().to_owned();
        if !valid_child_name(&name) {
            self.status = "Use a child name between 1 and 40 characters.".to_string();
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
        if !self.can_change("Unlock parent mode before removing entries.") {
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
            self.status = "There are no entries to remove.".to_string();
        }
    }

    fn print_selected_wallet(&mut self) {
        if !self.save_enabled {
            self.status = "Saved data could not be loaded, so printing is disabled.".to_string();
            return;
        }

        match write_printable_ledger(&self.print_path(false), &[self.selected_wallet().clone()]) {
            Ok(path) => self.open_printable_file(&path),
            Err(err) => self.status = format!("Could not create printable ledger: {err}"),
        }
    }

    fn print_all_wallets(&mut self) {
        if !self.save_enabled {
            self.status = "Saved data could not be loaded, so printing is disabled.".to_string();
            return;
        }

        match write_printable_ledger(&self.print_path(true), &self.data.wallets) {
            Ok(path) => self.open_printable_file(&path),
            Err(err) => self.status = format!("Could not create printable ledger: {err}"),
        }
    }

    fn open_printable_file(&mut self, path: &PathBuf) {
        match opener::open(path) {
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
            "cofferly-ledgers.html".to_owned()
        } else {
            format!(
                "cofferly-{}-ledger.html",
                ledger_file_stem(&self.selected_wallet().child_name)
            )
        };

        self.data_path
            .parent()
            .map_or_else(|| PathBuf::from("."), PathBuf::from)
            .join(file_name)
    }

    fn save_with_success(&mut self, success_status: impl Into<String>) {
        if !self.save_enabled {
            self.status = "Saved data could not be loaded, so changes are disabled.".to_string();
            return;
        }

        match save_app_data(&self.data_path, &self.data) {
            Ok(()) => self.status = success_status.into(),
            Err(err) => self.status = format!("Could not save: {err}"),
        }
    }

    fn can_change(&mut self, locked_status: &str) -> bool {
        if !self.save_enabled {
            self.status = "Saved data could not be loaded, so changes are disabled.".to_string();
            return false;
        }

        if !self.parent_unlocked {
            self.status = locked_status.to_string();
            return false;
        }

        true
    }
}

impl eframe::App for CofferlyApp {
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

impl CofferlyApp {
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
                    let pin_entry_width = 4.0 * 64.0 + 3.0 * 12.0;
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

fn pin_digit_id(index: usize) -> egui::Id {
    egui::Id::new(("parent_pin_digit", index))
}
