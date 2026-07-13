use chrono::Local;
use eframe::egui;
use eframe::egui::Color32;
use std::path::PathBuf;

mod crypto;
mod data;
mod io;
mod money;
mod print_html;
mod theme;
mod views;

pub const APP_NAME: &str = "Cofferly";
pub const DATA_FILE_NAME: &str = "data.json";
pub const ATLAS_LEGACY_APP_NAME: &str = "Atlas Wallet";
pub const ATLAS_LEGACY_DATA_FILE_NAME: &str = "atlas-wallet-data.json";
pub const LEGACY_APP_NAME: &str = "TallyNest";
pub const LEGACY_DATA_FILE_NAME: &str = "tallynest-data.json";
pub const AIRWALLET_LEGACY_APP_NAME: &str = "AirWallet";
pub const AIRWALLET_LEGACY_DATA_FILE_NAME: &str = "airwallet-data.json";
const PIN_LENGTH: usize = 4;
const LOCK_SCREEN_IMAGE_BYTES: &[u8] = include_bytes!("../assets/cofferly-lock.jpg");

use data::{
    default_app_data, valid_cents, valid_child_name, valid_description, valid_pin, AppData, Entry,
    EntryKind, LedgerSort, Wallet,
};
use io::{data_path, load_app_data_with_legacy, save_encrypted};
use money::{format_money, format_money_input, parse_dollars_to_cents};
use print_html::{ledger_file_stem, write_printable_ledger};
use theme::{app_icon, balance_color, configure_style};

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

/// The entry most recently removed from a wallet, held briefly so the user can
/// undo the deletion. Cleared by any new mutation.
#[derive(Debug, Clone)]
struct RemovableEntry {
    wallet_index: usize,
    entry: Entry,
}

struct CofferlyApp {
    data: AppData,
    raw_bytes: Option<Vec<u8>>,
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
    lock_screen_image: Option<egui::TextureHandle>,
    lock_screen_bg: egui::Color32,
    show_settings: bool,
    confirm_delete_wallet: bool,
    undo: Option<RemovableEntry>,
}

impl CofferlyApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_style(&cc.egui_ctx);

        let data_path = data_path();
        let (raw_bytes, raw_load_error) = match io::load_raw(&data_path) {
            Ok(raw_bytes) => (raw_bytes, None),
            Err(err) => (None, Some(err)),
        };

        // Try to load as plain JSON for backward compat / first run.
        // If the file is encrypted, we'll decrypt it on successful PIN entry.
        let (data, save_enabled, status) = if let Some(err) = raw_load_error {
            (
                default_app_data(),
                false,
                format!("Could not read saved data: {err}. Changes are disabled."),
            )
        } else if let Some(bytes) = &raw_bytes {
            if crypto::is_encrypted(bytes) {
                // Encrypted file — we will decrypt after PIN entry.
                // Use defaults until unlocked.
                (
                    default_app_data(),
                    true,
                    "Enter the parent PIN to unlock Cofferly.".to_string(),
                )
            } else {
                match load_app_data_with_legacy(&data_path) {
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
                }
            }
        } else {
            (
                default_app_data(),
                true,
                "Enter the parent PIN to unlock Cofferly.".to_string(),
            )
        };

        let (lock_screen_image, lock_screen_bg) = load_lock_screen_image(&cc.egui_ctx);

        Self {
            data,
            raw_bytes,
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
            lock_screen_image,
            lock_screen_bg,
            show_settings: false,
            confirm_delete_wallet: false,
            undo: None,
        }
    }

    fn selected_wallet(&self) -> &Wallet {
        &self.data.wallets[self.selected_wallet]
    }

    fn selected_wallet_mut(&mut self) -> &mut Wallet {
        &mut self.data.wallets[self.selected_wallet]
    }

    fn unlock_parent(&mut self) {
        let entered = self.entered_parent_pin();

        // Try encrypted path first
        if let Some(raw) = &self.raw_bytes {
            if crypto::is_encrypted(raw) {
                if let Ok(plain) = crypto::decrypt(raw, &entered) {
                    if let Ok(loaded) = serde_json::from_slice::<AppData>(&plain) {
                        if let Some(normalized) = data::normalize_app_data(loaded) {
                            // Successful decrypt with this PIN proves it was correct.
                            self.data = normalized;
                            self.parent_unlocked = true;
                            self.clear_pin_digits();
                            self.status = "Parent mode unlocked.".to_string();
                            return;
                        }
                    }
                }
                self.clear_pin_digits();
                self.status = "Wrong PIN or data has been tampered with.".to_string();
                return;
            }
        }

        // Legacy plain JSON path (or first run after migration).
        // The migration re-encrypts the file below, so the plaintext bytes do not
        // stay on disk in the legacy location.
        if entered == self.data.parent_pin {
            self.parent_unlocked = true;
            self.clear_pin_digits();

            // Auto-migrate plain data file to encrypted format immediately.
            // This is important when copying an old data file to another computer.
            if let Some(raw) = &self.raw_bytes {
                if !crypto::is_encrypted(raw) {
                    let data = self.data.clone();
                    match self.save_encrypted_data_and_refresh(&data, &entered) {
                        Ok(()) => {
                            self.status =
                                "Parent mode unlocked (data file migrated to encrypted format)."
                                    .to_string();
                        }
                        Err(e) => {
                            self.status = format!(
                                "Parent mode unlocked, but could not encrypt data file: {e}"
                            );
                        }
                    }
                    return;
                }
            }

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
        // A PIN change is unrelated to a pending removal; drop the undo window.
        self.undo = None;
        self.confirm_delete_wallet = false;

        if !valid_pin(&self.new_pin_input) {
            self.status = "Choose exactly 4 digits for the parent PIN.".to_string();
            return;
        }

        let mut updated_data = self.data.clone();
        updated_data.parent_pin = self.new_pin_input.clone();
        let new_pin = updated_data.parent_pin.clone();

        match self.save_encrypted_data_and_refresh(&updated_data, &new_pin) {
            Ok(()) => {
                self.data = updated_data;
                self.new_pin_input.clear();
                self.status = "Parent PIN updated.".to_string();
            }
            Err(err) => self.status = format!("Could not save: {err}"),
        }
    }

    fn add_entry(&mut self) {
        if !self.can_change("Unlock parent mode before adding entries.") {
            return;
        }
        // Adding an entry invalidates any pending undo from a prior removal.
        self.undo = None;
        self.confirm_delete_wallet = false;

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
        if !valid_description(&self.draft.description) {
            self.status = "Add a description (1-100 characters).".to_string();
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

        // Push first, then validate. If the resulting wallet is out of range,
        // pop the entry so nothing changes. This avoids cloning the whole wallet
        // and constructing the Entry twice.
        self.selected_wallet_mut().entries.push(Entry {
            date: Local::now().date_naive(),
            description: description.clone(),
            amount_cents: signed_amount,
        });
        if !self.selected_wallet().balances_are_valid() {
            self.selected_wallet_mut().entries.pop();
            self.status =
                "That entry would put the wallet outside Cofferly's supported range.".to_string();
            return;
        }

        let status = format!(
            "{action} {} for {}: {description}.",
            format_money(amount),
            wallet_name
        );

        self.draft.description.clear();
        self.draft.amount.clear();
        self.save_with_success(status);
    }

    fn update_starting_balance(&mut self) {
        if !self.can_change("Unlock parent mode before changing balances.") {
            return;
        }
        self.undo = None;
        self.confirm_delete_wallet = false;

        let Ok(balance) = parse_dollars_to_cents(&self.starting_balance_input) else {
            self.status = "Enter a valid starting balance, like 90 or 90.00.".to_string();
            return;
        };
        if !valid_cents(balance) {
            self.status = "Enter a smaller starting balance.".to_string();
            return;
        }

        let wallet = self.selected_wallet_mut();
        let previous_balance = wallet.starting_balance_cents;
        wallet.starting_balance_cents = balance;
        if !wallet.balances_are_valid() {
            // Roll back so nothing changes.
            wallet.starting_balance_cents = previous_balance;
            self.status =
                "That starting balance would put the wallet outside Cofferly's supported range."
                    .to_string();
            return;
        }

        let wallet_name = self.selected_wallet().child_name.clone();
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
        self.undo = None;
        self.confirm_delete_wallet = false;

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
        self.undo = None;
        self.confirm_delete_wallet = false;

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
        self.confirm_delete_wallet = false;

        let wallet_name = self.selected_wallet().child_name.clone();
        if let Some(entry) = self.selected_wallet_mut().entries.pop() {
            // Hold the removed entry so the user can undo before any other change.
            self.undo = Some(RemovableEntry {
                wallet_index: self.selected_wallet,
                entry: entry.clone(),
            });
            self.save_with_success(format!(
                "Removed latest entry from {}: {} {}. Undo available.",
                wallet_name,
                format_money(entry.amount_cents),
                entry.description
            ));
        } else {
            self.status = "There are no entries to remove.".to_string();
        }
    }

    fn undo_remove_entry(&mut self) {
        if !self.can_change("Unlock parent mode before undoing.") {
            return;
        }
        self.confirm_delete_wallet = false;

        let Some(removable) = self.undo.take() else {
            return;
        };

        // A pending undo is only valid if the wallet still exists.
        let Some(wallet) = self.data.wallets.get_mut(removable.wallet_index) else {
            self.status = "Can't undo — that wallet no longer exists.".to_string();
            return;
        };

        wallet.entries.push(removable.entry.clone());
        let wallet_name = wallet.child_name.clone();
        self.save_with_success(format!(
            "Restored entry for {}: {} {}.",
            wallet_name,
            format_money(removable.entry.amount_cents),
            removable.entry.description
        ));
    }

    fn delete_selected_wallet(&mut self) {
        if !self.can_change("Unlock parent mode before deleting wallets.") {
            return;
        }

        // Keep at least one wallet so the app is always usable.
        if self.data.wallets.len() <= 1 {
            self.status = "Keep at least one wallet.".to_string();
            return;
        }

        let wallet_name = self.selected_wallet().child_name.clone();
        let removed_index = self.selected_wallet;
        self.data.wallets.remove(removed_index);
        // Deleting a wallet invalidates any pending undo (the entry's wallet may be gone).
        self.undo = None;
        self.confirm_delete_wallet = false;
        // Keep a valid selection.
        if self.selected_wallet >= self.data.wallets.len() {
            self.selected_wallet = self.data.wallets.len() - 1;
        }
        self.save_with_success(format!("Deleted wallet for {wallet_name}."));
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

        // Every caller goes through `can_change`, which requires the app to be
        // unlocked, so a valid PIN is always available here. Always save
        // encrypted once we have one.
        let data = self.data.clone();
        let pin = data.parent_pin.clone();
        let save_result = self.save_encrypted_data_and_refresh(&data, &pin);

        match save_result {
            Ok(()) => self.status = success_status.into(),
            Err(err) => self.status = format!("Could not save: {err}"),
        }
    }

    fn save_encrypted_data_and_refresh(&mut self, data: &AppData, pin: &str) -> Result<(), String> {
        save_encrypted(&self.data_path, data, pin)?;
        self.raw_bytes = Some(
            io::load_raw(&self.data_path)?
                .ok_or_else(|| format!("Saved data missing from {}", self.data_path.display()))?,
        );
        Ok(())
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

        egui::Panel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .inner_margin(egui::Margin::symmetric(18, 10))
                    .stroke(egui::Stroke::new(1.0, theme::BORDER)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        egui::RichText::new(APP_NAME)
                            .size(24.0)
                            .strong()
                            .color(theme::TEXT_PRIMARY),
                    );
                    ui.add_space(6.0);
                    egui::Frame::new()
                        .fill(theme::ACCENT_LIGHT)
                        .corner_radius(egui::CornerRadius::same(12))
                        .inner_margin(egui::Margin::symmetric(10, 5))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("Parent mode unlocked")
                                    .size(11.0)
                                    .strong()
                                    .color(theme::ACCENT_DARK),
                            );
                        });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_sized(
                                [88.0, 36.0],
                                egui::Button::new(
                                    egui::RichText::new("Lock").strong().color(Color32::WHITE),
                                )
                                .fill(theme::ACCENT_DARK)
                                .stroke(egui::Stroke::NONE),
                            )
                            .clicked()
                        {
                            self.lock_parent();
                        }
                        if ui
                            .add_sized([108.0, 36.0], egui::Button::new("Settings"))
                            .clicked()
                        {
                            let wallet = self.selected_wallet();
                            let name = wallet.child_name.clone();
                            let bal = wallet.current_balance_cents();
                            self.child_name_input = name;
                            self.starting_balance_input = format_money_input(bal);
                            self.new_child_name_input.clear();
                            self.new_pin_input.clear();
                            self.confirm_delete_wallet = false;
                            self.show_settings = true;
                        }
                        ui.label(
                            egui::RichText::new("Saved locally")
                                .size(11.0)
                                .color(theme::TEXT_SECONDARY),
                        );
                    });
                });
            });

        egui::Panel::left("wallet_picker")
            .resizable(false)
            .min_size(252.0)
            .max_size(252.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::FAINT_BG)
                    .inner_margin(egui::Margin::same(16))
                    .stroke(egui::Stroke::new(1.0, theme::BORDER)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(
                            egui::RichText::new("Family wallets")
                                .strong()
                                .size(16.0)
                                .color(theme::TEXT_PRIMARY),
                        );
                        ui.label(
                            egui::RichText::new("Choose a child to view")
                                .size(12.0)
                                .color(theme::TEXT_SECONDARY),
                        );
                        ui.add_space(8.0);

                        for index in 0..self.data.wallets.len() {
                            let wallet = &self.data.wallets[index];
                            let selected = self.selected_wallet == index;

                            let response = ui.add_sized(
                                [220.0, 64.0],
                                egui::Button::selectable(selected, "")
                                    .fill(if selected {
                                        theme::ACCENT
                                    } else {
                                        theme::CARD_BG
                                    })
                                    .stroke(if selected {
                                        egui::Stroke::new(1.0, theme::ACCENT)
                                    } else {
                                        egui::Stroke::new(1.0, theme::BORDER)
                                    }),
                            );

                            if response.clicked() {
                                self.selected_wallet = index;
                                self.confirm_delete_wallet = false;
                            }

                            // Draw content inside the button area using painter for card look
                            let rect = response.rect;
                            let painter = ui.painter_at(rect);

                            let text_color = if selected {
                                Color32::WHITE
                            } else {
                                theme::TEXT_PRIMARY
                            };
                            let balance_color = if selected {
                                Color32::WHITE
                            } else {
                                balance_color(wallet.current_balance_cents())
                            };

                            painter.text(
                                rect.left_top() + egui::vec2(14.0, 12.0),
                                egui::Align2::LEFT_TOP,
                                &wallet.child_name,
                                egui::FontId::proportional(15.0),
                                text_color,
                            );

                            painter.text(
                                rect.left_bottom() + egui::vec2(14.0, -12.0),
                                egui::Align2::LEFT_BOTTOM,
                                format_money(wallet.current_balance_cents()),
                                egui::FontId::proportional(13.0),
                                balance_color,
                            );
                        }

                        ui.add_space(6.0);

                        if ui
                            .add_sized([220.0, 34.0], egui::Button::new("Print this wallet"))
                            .clicked()
                        {
                            self.print_selected_wallet();
                        }
                        if ui
                            .add_sized([220.0, 34.0], egui::Button::new("Print all wallets"))
                            .clicked()
                        {
                            self.print_all_wallets();
                        }

                        ui.add_space(12.0);
                        self.entry_form(ui);

                        ui.add_space(10.0);

                        egui::Frame::new()
                            .fill(theme::GOLD_LIGHT)
                            .corner_radius(egui::CornerRadius::same(8))
                            .inner_margin(egui::Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.set_max_width(200.0);
                                ui.label(
                                    egui::RichText::new(&self.status)
                                        .size(11.0)
                                        .color(theme::TEXT_PRIMARY),
                                );
                            });
                    });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::APP_BG)
                    .inner_margin(egui::Margin::same(22)),
            )
            .show(ui, |ui| {
                self.wallet_header(ui);
                ui.add_space(18.0);

                egui::Frame::new()
                    .fill(theme::CARD_BG)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER))
                    .corner_radius(egui::CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("Transaction history")
                                .strong()
                                .size(16.0)
                                .color(theme::TEXT_PRIMARY),
                        );
                        ui.label(
                            egui::RichText::new("A clear record of every change")
                                .size(12.0)
                                .color(theme::TEXT_SECONDARY),
                        );
                        ui.add_space(8.0);
                        self.ledger_table(ui);
                    });
            });

        if self.show_settings {
            self.show_settings_window(ui.ctx());
        }
    }
}

pub(crate) fn pin_digit_id(index: usize) -> egui::Id {
    egui::Id::new(("parent_pin_digit", index))
}

fn load_lock_screen_image(ctx: &egui::Context) -> (Option<egui::TextureHandle>, egui::Color32) {
    let dyn_image = match image::load_from_memory(LOCK_SCREEN_IMAGE_BYTES) {
        Ok(img) => img,
        Err(_) => return (None, egui::Color32::from_rgb(232, 227, 223)),
    };
    let rgba = dyn_image.to_rgba8();

    // Sample top-left corner color from the image to use as seamless background
    let bg_color = if rgba.width() > 0 && rgba.height() > 0 {
        let p = rgba.get_pixel(0, 0);
        egui::Color32::from_rgb(p[0], p[1], p[2])
    } else {
        egui::Color32::from_rgb(232, 227, 223)
    };

    let size = [rgba.width() as usize, rgba.height() as usize];
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());

    let texture = ctx.load_texture(
        "cofferly-lock-image",
        color_image,
        egui::TextureOptions::LINEAR,
    );

    (Some(texture), bg_color)
}

#[cfg(test)]
mod app_tests {
    use super::*;
    use tempfile::{tempdir, TempDir};

    fn test_app() -> (CofferlyApp, TempDir) {
        let dir = tempdir().unwrap();
        let app = CofferlyApp {
            data: default_app_data(),
            raw_bytes: None,
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
            pending_pin_focus: None,
            new_pin_input: String::new(),
            parent_unlocked: true,
            save_enabled: true,
            status: String::new(),
            data_path: dir.path().join(DATA_FILE_NAME),
            lock_screen_image: None,
            lock_screen_bg: theme::APP_BG,
            show_settings: false,
            confirm_delete_wallet: false,
            undo: None,
        };
        (app, dir)
    }

    fn saved_data(app: &CofferlyApp, pin: &str) -> AppData {
        let raw = std::fs::read(&app.data_path).unwrap();
        assert!(crypto::is_encrypted(&raw));
        let plaintext = crypto::decrypt(&raw, pin).unwrap();
        serde_json::from_slice(&plaintext).unwrap()
    }

    #[test]
    fn pasted_pin_digits_are_distributed_and_non_digits_are_ignored() {
        let (mut app, _dir) = test_app();
        app.pin_digits[1] = "9a87".to_owned();

        app.normalize_pin_digit_input(1);

        assert_eq!(app.pin_digits, ["", "9", "8", "7"]);
        assert_eq!(app.pending_pin_focus, Some(3));
        assert!(!app.parent_pin_complete());

        app.pin_digits[0] = "1".to_owned();
        assert!(app.parent_pin_complete());
        assert_eq!(app.entered_parent_pin(), "1987");
    }

    #[test]
    fn encrypted_unlock_accepts_the_right_pin_and_clears_pin_fields() {
        let (mut app, _dir) = test_app();
        let mut stored = default_app_data();
        stored.wallets[0].child_name = "Encrypted wallet".to_owned();
        let serialized = serde_json::to_vec(&stored).unwrap();
        app.raw_bytes = Some(crypto::encrypt(&serialized, "2468").unwrap());
        app.parent_unlocked = false;
        app.pin_digits = ["2".into(), "4".into(), "6".into(), "8".into()];

        app.unlock_parent();

        assert!(app.parent_unlocked);
        assert_eq!(app.selected_wallet().child_name, "Encrypted wallet");
        assert!(app.pin_digits.iter().all(String::is_empty));
        assert_eq!(app.pending_pin_focus, Some(0));
        assert_eq!(app.status, "Parent mode unlocked.");
    }

    #[test]
    fn encrypted_unlock_rejects_wrong_pin_without_exposing_data() {
        let (mut app, _dir) = test_app();
        let mut stored = default_app_data();
        stored.wallets[0].child_name = "Secret wallet".to_owned();
        let serialized = serde_json::to_vec(&stored).unwrap();
        app.raw_bytes = Some(crypto::encrypt(&serialized, "2468").unwrap());
        app.parent_unlocked = false;
        app.pin_digits = ["0".into(), "0".into(), "0".into(), "0".into()];

        app.unlock_parent();

        assert!(!app.parent_unlocked);
        assert_ne!(app.selected_wallet().child_name, "Secret wallet");
        assert!(app.pin_digits.iter().all(String::is_empty));
        assert_eq!(app.status, "Wrong PIN or data has been tampered with.");
    }

    #[test]
    fn transaction_remove_and_undo_workflow_stays_encrypted() {
        let (mut app, _dir) = test_app();
        app.draft.kind = EntryKind::Deposit;
        app.draft.description = "Weekly allowance".to_owned();
        app.draft.amount = "$10.50".to_owned();

        app.add_entry();

        assert_eq!(app.selected_wallet().current_balance_cents(), 1050);
        assert!(app.draft.description.is_empty());
        assert!(app.status.contains("Added $10.50"));
        assert_eq!(saved_data(&app, "1234").wallets[0].entries.len(), 1);

        app.remove_latest_entry();
        assert!(app.selected_wallet().entries.is_empty());
        assert!(app.undo.is_some());
        assert!(app.status.contains("Undo available"));

        app.undo_remove_entry();
        assert_eq!(app.selected_wallet().current_balance_cents(), 1050);
        assert!(app.undo.is_none());
        assert_eq!(saved_data(&app, "1234").wallets[0].entries.len(), 1);
    }

    #[test]
    fn invalid_transaction_does_not_mutate_or_create_a_file() {
        let (mut app, _dir) = test_app();
        app.draft.kind = EntryKind::Deduction;
        app.draft.description = "Toy".to_owned();
        app.draft.amount = "not money".to_owned();

        app.add_entry();

        assert!(app.selected_wallet().entries.is_empty());
        assert!(!app.data_path.exists());
        assert_eq!(app.status, "Enter a valid amount, like 10 or 10.50.");
    }

    #[test]
    fn changing_pin_reencrypts_data_and_rejects_the_old_pin() {
        let (mut app, _dir) = test_app();
        app.new_pin_input = "9876".to_owned();

        app.update_pin();

        assert_eq!(app.data.parent_pin, "9876");
        assert!(app.new_pin_input.is_empty());
        let raw = std::fs::read(&app.data_path).unwrap();
        assert!(crypto::decrypt(&raw, "1234").is_err());
        assert_eq!(saved_data(&app, "9876").parent_pin, "9876");
    }

    #[test]
    fn wallet_management_keeps_at_least_one_wallet() {
        let (mut app, _dir) = test_app();
        app.new_child_name_input = "Sam".to_owned();
        app.add_child_wallet();

        assert_eq!(app.data.wallets.len(), 3);
        assert_eq!(app.selected_wallet().child_name, "Sam");

        app.delete_selected_wallet();
        app.delete_selected_wallet();
        app.delete_selected_wallet();

        assert_eq!(app.data.wallets.len(), 1);
        assert_eq!(app.status, "Keep at least one wallet.");
        assert_eq!(saved_data(&app, "1234").wallets.len(), 1);
    }
}
