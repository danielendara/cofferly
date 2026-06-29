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
    default_app_data, valid_cents, valid_child_name, valid_pin, AppData, Entry, EntryKind,
    LedgerRowDate, LedgerSort, Wallet,
};
use io::{data_path, load_app_data_with_legacy, save_app_data, save_encrypted};
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
            lock_screen_image: load_lock_screen_image(&cc.egui_ctx),
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

        // Legacy plain JSON path (or first run after migration)
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

        let save_result = if self.parent_unlocked {
            // Always save encrypted once we have a valid PIN.
            let data = self.data.clone();
            let pin = data.parent_pin.clone();
            self.save_encrypted_data_and_refresh(&data, &pin)
        } else {
            // Should not normally happen for mutable operations.
            save_app_data(&self.data_path, &self.data)
        };

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

        egui::Panel::top("header").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new(APP_NAME)
                        .size(26.0)
                        .strong()
                        .color(theme::ACCENT),
                );
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Parent mode")
                        .size(13.0)
                        .color(theme::TEXT_SECONDARY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_sized([80.0, 28.0], egui::Button::new("Lock").fill(theme::ACCENT))
                        .clicked()
                    {
                        self.lock_parent();
                    }
                });
            });
            ui.add_space(6.0);
        });

        egui::Panel::left("wallet_picker")
            .resizable(false)
            .min_size(180.0)
            .max_size(220.0)
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Wallets")
                        .strong()
                        .size(15.0)
                        .color(theme::TEXT_PRIMARY),
                );
                ui.add_space(6.0);

                for index in 0..self.data.wallets.len() {
                    let wallet = &self.data.wallets[index];
                    let selected = self.selected_wallet == index;

                    let response = ui.add_sized(
                        [188.0, 58.0],
                        egui::Button::selectable(selected, "")
                            .fill(if selected {
                                theme::ACCENT_LIGHT
                            } else {
                                Color32::WHITE
                            })
                            .stroke(if selected {
                                egui::Stroke::new(1.5, theme::ACCENT)
                            } else {
                                egui::Stroke::new(1.0, theme::BORDER)
                            }),
                    );

                    if response.clicked() {
                        self.selected_wallet = index;
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
                        rect.left_top() + egui::vec2(12.0, 10.0),
                        egui::Align2::LEFT_TOP,
                        &wallet.child_name,
                        egui::FontId::proportional(15.0),
                        text_color,
                    );

                    painter.text(
                        rect.left_bottom() + egui::vec2(12.0, -10.0),
                        egui::Align2::LEFT_BOTTOM,
                        format_money(wallet.current_balance_cents()),
                        egui::FontId::proportional(13.0),
                        balance_color,
                    );
                }

                ui.add_space(8.0);

                if ui
                    .add_sized([188.0, 28.0], egui::Button::new("Print this ledger"))
                    .clicked()
                {
                    self.print_selected_wallet();
                }
                if ui
                    .add_sized([188.0, 28.0], egui::Button::new("Print both ledgers"))
                    .clicked()
                {
                    self.print_all_wallets();
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                ui.label(egui::RichText::new("Change PIN").strong().size(13.0));
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_pin_input)
                        .password(true)
                        .desired_width(100.0)
                        .hint_text("4 digits"),
                );
                if ui
                    .add_sized([80.0, 26.0], egui::Button::new("Save").fill(theme::ACCENT))
                    .clicked()
                {
                    self.update_pin();
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        egui::RichText::new(&self.status)
                            .size(11.0)
                            .color(theme::TEXT_SECONDARY),
                    );
                });
            });

        egui::CentralPanel::default().show(ui, |ui| {
            self.wallet_header(ui);
            ui.add_space(10.0);
            self.quick_actions(ui);
            ui.add_space(8.0);
            self.wallet_settings(ui);
            ui.add_space(8.0);
            self.balance_tools(ui);
            ui.add_space(8.0);
            self.entry_form(ui);
            ui.add_space(12.0);
            self.ledger_table(ui);
        });
    }
}

impl CofferlyApp {
    fn lock_screen(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.15); // Responsive top padding

                // Logo / artwork - scale nicely
                if let Some(texture) = &self.lock_screen_image {
                    let max_width = (ui.available_width() * 0.55).min(280.0);
                    let aspect = 260.0 / 146.0;
                    let size = egui::vec2(max_width, max_width / aspect);
                    ui.add(egui::Image::new(texture).fit_to_exact_size(size));
                }
                ui.add_space(16.0);

                ui.label(
                    egui::RichText::new(APP_NAME)
                        .size(38.0)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Parent PIN required")
                        .size(20.0)
                        .color(theme::TEXT_SECONDARY),
                );
                ui.add_space(20.0);

                ui.label(
                    egui::RichText::new(
                        "Balances and ledgers stay private until a parent unlocks the app.",
                    )
                    .size(14.0)
                    .color(theme::TEXT_SECONDARY),
                );
                ui.add_space(28.0);

                // PIN input area - framed for modern card look
                egui::Frame::new()
                    .fill(theme::CARD_BG)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(20, 16))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("Enter 4-digit parent PIN")
                                .size(13.0)
                                .strong()
                                .color(theme::TEXT_PRIMARY),
                        );
                        ui.add_space(10.0);

                        if let Some(index) = self.pending_pin_focus.take() {
                            ui.memory_mut(|memory| memory.request_focus(pin_digit_id(index)));
                        }

                        let enter_pressed = ui.input(|input| input.key_pressed(egui::Key::Enter));

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            let pin_entry_width = 4.0 * 58.0 + 3.0 * 10.0;
                            ui.add_space(((ui.available_width() - pin_entry_width) / 2.0).max(0.0));

                            for index in 0..PIN_LENGTH {
                                let response = ui.add_sized(
                                    [58.0, 58.0],
                                    egui::TextEdit::singleline(&mut self.pin_digits[index])
                                        .id(pin_digit_id(index))
                                        .password(true)
                                        .font(egui::TextStyle::Heading)
                                        .horizontal_align(egui::Align::Center)
                                        .vertical_align(egui::Align::Center)
                                        .char_limit(PIN_LENGTH)
                                        .desired_width(58.0),
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

                        ui.add_space(14.0);

                        if ui
                            .add_sized(
                                [160.0, 36.0],
                                egui::Button::new("Unlock").fill(theme::ACCENT),
                            )
                            .clicked()
                        {
                            self.unlock_parent();
                        }
                    });

                ui.add_space(20.0);
                ui.label(
                    egui::RichText::new(&self.status)
                        .size(13.0)
                        .color(theme::TEXT_SECONDARY),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("First run default PIN: 1234")
                        .size(12.0)
                        .color(theme::TEXT_SECONDARY),
                );
            });
        });
    }

    fn wallet_header(&self, ui: &mut egui::Ui) {
        let wallet = self.selected_wallet();

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(&wallet.child_name)
                        .size(28.0)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                );
                ui.label(
                    egui::RichText::new("Running balance")
                        .size(12.0)
                        .color(theme::TEXT_SECONDARY),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format_money(wallet.current_balance_cents()))
                        .size(36.0)
                        .strong()
                        .color(balance_color(wallet.current_balance_cents())),
                );
            });
        });
    }

    fn quick_actions(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(theme::CARD_BG)
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Quick add").strong().size(13.0));
                ui.add_space(6.0);

                ui.horizontal_wrapped(|ui| {
                    let deposit_buttons = [
                        ("+ $5", 500, "Allowance"),
                        ("+ $10", 1000, "Allowance"),
                        ("+ $20", 2000, "Gift"),
                        ("+ $50", 5000, "Gift"),
                    ];
                    for (label, amount, desc) in deposit_buttons {
                        if ui.button(label).clicked() {
                            self.quick_entry(desc, amount, EntryKind::Deposit);
                        }
                    }

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    let deduct_buttons = [
                        ("- $5", 500, "Game purchase"),
                        ("- $10", 1000, "Purchase"),
                        ("- $15", 1500, "Purchase"),
                    ];
                    for (label, amount, desc) in deduct_buttons {
                        if ui.button(label).clicked() {
                            self.quick_entry(desc, amount, EntryKind::Deduction);
                        }
                    }
                });
            });
    }

    fn wallet_settings(&mut self, ui: &mut egui::Ui) {
        let selected_child_name = self.selected_wallet().child_name.clone();

        egui::Frame::new()
            .fill(theme::CARD_BG)
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Child names").strong().size(13.0));
                ui.add_space(6.0);

                egui::Grid::new("wallet_settings_grid")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Rename selected").size(12.0));
                        ui.add_sized(
                            [160.0, 24.0],
                            egui::TextEdit::singleline(&mut self.child_name_input)
                                .hint_text(selected_child_name),
                        );
                        ui.end_row();

                        ui.label("");
                        if ui
                            .add_sized([80.0, 24.0], egui::Button::new("Rename"))
                            .clicked()
                        {
                            self.rename_selected_child();
                        }
                        ui.end_row();

                        ui.separator();
                        ui.end_row();

                        ui.label(egui::RichText::new("Add new wallet").size(12.0));
                        ui.add_sized(
                            [160.0, 24.0],
                            egui::TextEdit::singleline(&mut self.new_child_name_input)
                                .hint_text("Child name"),
                        );
                        ui.end_row();

                        ui.label("");
                        if ui
                            .add_sized([80.0, 24.0], egui::Button::new("Add"))
                            .clicked()
                        {
                            self.add_child_wallet();
                        }
                    });
            });
    }

    fn balance_tools(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(theme::CARD_BG)
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Balance tools").strong().size(13.0));
                ui.add_space(6.0);

                egui::Grid::new("balance_grid")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Starting balance").size(12.0));
                        ui.add_sized(
                            [100.0, 24.0],
                            egui::TextEdit::singleline(&mut self.starting_balance_input)
                                .hint_text("90.00"),
                        );
                        if ui
                            .add_sized([70.0, 24.0], egui::Button::new("Update"))
                            .clicked()
                        {
                            self.update_starting_balance();
                        }
                        ui.end_row();

                        ui.separator();
                        ui.end_row();

                        if ui
                            .add_sized([140.0, 24.0], egui::Button::new("Remove latest entry"))
                            .clicked()
                        {
                            self.remove_latest_entry();
                        }
                    });
            });
    }

    fn entry_form(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(theme::CARD_BG)
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("New entry").strong().size(13.0));
                ui.add_space(6.0);

                egui::Grid::new("entry_grid")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Type").size(12.0));
                        ui.horizontal(|ui| {
                            ui.selectable_value(
                                &mut self.draft.kind,
                                EntryKind::Deposit,
                                "Deposit",
                            );
                            ui.selectable_value(
                                &mut self.draft.kind,
                                EntryKind::Deduction,
                                "Deduction",
                            );
                        });
                        ui.end_row();

                        ui.label(egui::RichText::new("Description").size(12.0));
                        ui.add_sized(
                            [240.0, 24.0],
                            egui::TextEdit::singleline(&mut self.draft.description)
                                .hint_text("Game, birthday, allowance"),
                        );
                        ui.end_row();

                        ui.label(egui::RichText::new("Amount").size(12.0));
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [90.0, 24.0],
                                egui::TextEdit::singleline(&mut self.draft.amount)
                                    .hint_text("10.00"),
                            );
                            if ui
                                .add_sized([70.0, 24.0], egui::Button::new("Add"))
                                .clicked()
                            {
                                self.add_entry();
                            }
                        });
                    });
            });
    }

    fn ledger_table(&mut self, ui: &mut egui::Ui) {
        let ledger_sort = self.ledger_sort;
        let wallet = self.selected_wallet();
        let rows = wallet.ledger_rows_sorted(ledger_sort);
        let mut toggle_sort = false;

        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::initial(80.0).at_least(70.0))
            .column(egui_extras::Column::remainder().at_least(200.0))
            .column(egui_extras::Column::initial(90.0).at_least(70.0))
            .column(egui_extras::Column::initial(100.0).at_least(80.0))
            .header(20.0, |mut header| {
                header.col(|ui| {
                    let label = match ledger_sort {
                        LedgerSort::NewestFirst => "Date ▼",
                        LedgerSort::OldestFirst => "Date ▲",
                    };
                    let tooltip = match ledger_sort {
                        LedgerSort::NewestFirst => "Newest first — click to sort oldest first",
                        LedgerSort::OldestFirst => "Oldest first — click to sort newest first",
                    };
                    if ui
                        .small_button(
                            egui::RichText::new(label)
                                .strong()
                                .size(11.0)
                                .color(theme::TEXT_PRIMARY),
                        )
                        .on_hover_text(tooltip)
                        .clicked()
                    {
                        toggle_sort = true;
                    }
                });
                header.col(|ui| {
                    ui.label(egui::RichText::new("Description").strong().size(11.0));
                });
                header.col(|ui| {
                    ui.label(egui::RichText::new("Amount").strong().size(11.0));
                });
                header.col(|ui| {
                    ui.label(egui::RichText::new("Balance").strong().size(11.0));
                });
            })
            .body(|mut body| {
                for (i, ledger_row) in rows.iter().enumerate() {
                    let is_start = i == 0 && matches!(ledger_row.date, LedgerRowDate::Start);
                    let row_h = if is_start { 18.0 } else { 20.0 };
                    body.row(row_h, |mut row| {
                        row.col(|ui| {
                            let date_text = egui::RichText::new(ledger_row.date.label())
                                .size(if is_start { 10.0 } else { 11.0 })
                                .color(if is_start {
                                    theme::TEXT_SECONDARY
                                } else {
                                    theme::TEXT_SECONDARY
                                });
                            ui.label(date_text);
                        });
                        row.col(|ui| {
                            let desc = if is_start {
                                egui::RichText::new(ledger_row.description)
                                    .size(10.0)
                                    .italics()
                                    .color(theme::TEXT_SECONDARY)
                            } else {
                                egui::RichText::new(ledger_row.description)
                                    .size(12.0)
                                    .color(theme::TEXT_PRIMARY)
                            };
                            ui.label(desc);
                        });
                        row.col(|ui| {
                            let amt = egui::RichText::new(format_money(ledger_row.amount_cents))
                                .size(if is_start { 11.0 } else { 12.0 })
                                .color(amount_color(ledger_row.amount_cents));
                            ui.label(amt);
                        });
                        row.col(|ui| {
                            let bal = egui::RichText::new(format_money(ledger_row.balance_cents))
                                .size(if is_start { 11.0 } else { 12.0 })
                                .strong()
                                .color(balance_color(ledger_row.balance_cents));
                            ui.label(bal);
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

fn load_lock_screen_image(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let image = image::load_from_memory(LOCK_SCREEN_IMAGE_BYTES).ok()?;
    let rgba = image.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());

    Some(ctx.load_texture(
        "cofferly-lock-image",
        color_image,
        egui::TextureOptions::LINEAR,
    ))
}
