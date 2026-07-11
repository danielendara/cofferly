//! Rendering of the app's screens: the lock/PIN screen, the wallet header,
//! the entry form, the ledger table, and the settings window.
//!
//! These methods all belong to the same `impl CofferlyApp` block declared in
//! `main.rs`. Field privacy is unaffected because the block is in the same
//! crate as the struct definition.

use eframe::egui;

use crate::data::{EntryKind, LedgerRowDate, LedgerSort};
use crate::money::format_money;
use crate::money::format_money_input;
use crate::theme;
use crate::theme::amount_color;
use crate::theme::balance_color;
use crate::CofferlyApp;
use crate::{APP_NAME, PIN_LENGTH};

impl CofferlyApp {
    pub fn lock_screen(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(self.lock_screen_bg))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() * 0.07).clamp(20.0, 64.0));

                    if let Some(texture) = &self.lock_screen_image {
                        let max_width = (ui.available_width() * 0.5).clamp(220.0, 340.0);
                        let aspect = 260.0 / 146.0;
                        let size = egui::vec2(max_width, max_width / aspect);
                        ui.add(egui::Image::new(texture).fit_to_exact_size(size));
                    }
                    ui.add_space(8.0);

                    ui.label(
                        egui::RichText::new(APP_NAME)
                            .size(40.0)
                            .strong()
                            .color(theme::TEXT_PRIMARY),
                    );
                    ui.label(
                        egui::RichText::new("A simple, private allowance wallet")
                            .size(18.0)
                            .color(theme::TEXT_SECONDARY),
                    );
                    ui.add_space(22.0);

                    egui::Frame::new()
                        .fill(theme::CARD_BG)
                        .stroke(egui::Stroke::new(1.0, theme::BORDER))
                        .corner_radius(egui::CornerRadius::same(14))
                        .inner_margin(egui::Margin::symmetric(28, 22))
                        .show(ui, |ui| {
                            ui.set_max_width(460.0);
                            ui.set_min_width((ui.available_width() * 0.7).min(460.0));
                            ui.label(
                                egui::RichText::new("Welcome back")
                                    .size(19.0)
                                    .strong()
                                    .color(theme::TEXT_PRIMARY),
                            );
                            ui.label(
                                egui::RichText::new("Enter the 4-digit parent PIN to continue")
                                    .size(13.0)
                                    .color(theme::TEXT_SECONDARY),
                            );
                            ui.add_space(16.0);

                            if let Some(index) = self.pending_pin_focus.take() {
                                ui.memory_mut(|memory| {
                                    memory.request_focus(crate::pin_digit_id(index))
                                });
                            }

                            let enter_pressed =
                                ui.input(|input| input.key_pressed(egui::Key::Enter));

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                let pin_entry_width = 4.0 * 54.0 + 3.0 * 10.0;
                                ui.add_space(
                                    ((ui.available_width() - pin_entry_width) / 2.0).max(0.0),
                                );

                                for index in 0..PIN_LENGTH {
                                    let response = ui.add_sized(
                                        [54.0, 54.0],
                                        egui::TextEdit::singleline(&mut self.pin_digits[index])
                                            .id(crate::pin_digit_id(index))
                                            .password(true)
                                            .font(egui::TextStyle::Heading)
                                            .horizontal_align(egui::Align::Center)
                                            .vertical_align(egui::Align::Center)
                                            .char_limit(PIN_LENGTH)
                                            .desired_width(54.0),
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

                            ui.add_space(16.0);

                            if ui
                                .add_sized(
                                    [240.0, 42.0],
                                    egui::Button::new(
                                        egui::RichText::new("Unlock")
                                            .color(egui::Color32::WHITE)
                                            .strong(),
                                    )
                                    .fill(theme::ACCENT),
                                )
                                .clicked()
                            {
                                self.unlock_parent();
                            }
                        });

                    ui.add_space(18.0);
                    ui.label(
                        egui::RichText::new(&self.status)
                            .size(13.0)
                            .color(theme::TEXT_SECONDARY),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("Local-only  •  No account  •  No cloud sync")
                            .size(12.0)
                            .color(theme::TEXT_SECONDARY),
                    );
                    ui.label(
                        egui::RichText::new(
                            "First run? Use 1234, then choose a new PIN in Settings.",
                        )
                        .size(11.0)
                        .color(theme::TEXT_SECONDARY),
                    );
                });
            });
    }

    pub fn wallet_header(&mut self, ui: &mut egui::Ui) {
        let wallet = self.selected_wallet();
        let name = wallet.child_name.clone();
        let balance = wallet.current_balance_cents();

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(&name)
                        .size(26.0)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                );
                ui.label(
                    egui::RichText::new("Available balance")
                        .size(12.0)
                        .color(theme::TEXT_SECONDARY),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format_money(balance))
                        .size(32.0)
                        .strong()
                        .color(balance_color(balance)),
                );
            });
        });
    }

    pub fn show_settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }

        let mut open = true;

        egui::Window::new("Settings")
            .open(&mut open)
            .default_width(520.0)
            .default_height(600.0)
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                let selected_name = self.selected_wallet().child_name.clone();
                let current_balance = self.selected_wallet().current_balance_cents();

                ui.label(egui::RichText::new("Wallet details").strong().size(17.0));
                ui.label(
                    egui::RichText::new(format!("Manage {selected_name}'s wallet"))
                        .size(12.0)
                        .color(theme::TEXT_SECONDARY),
                );
                ui.add_space(4.0);

                egui::Grid::new("settings_wallet_grid")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        // Rename
                        ui.label(egui::RichText::new("Rename").size(12.0));
                        ui.add_sized(
                            [220.0, 34.0],
                            egui::TextEdit::singleline(&mut self.child_name_input)
                                .hint_text(&selected_name),
                        );
                        ui.end_row();

                        ui.label("");
                        if ui
                            .add_sized([90.0, 34.0], egui::Button::new("Rename"))
                            .clicked()
                        {
                            self.rename_selected_child();
                        }
                        ui.end_row();

                        ui.separator();
                        ui.end_row();

                        // Starting balance
                        ui.label(egui::RichText::new("Starting balance").size(12.0));
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [130.0, 34.0],
                                egui::TextEdit::singleline(&mut self.starting_balance_input)
                                    .hint_text(format_money_input(current_balance)),
                            );
                            if ui
                                .add_sized([80.0, 34.0], egui::Button::new("Update"))
                                .clicked()
                            {
                                self.update_starting_balance();
                            }
                        });
                        ui.end_row();

                        ui.separator();
                        ui.end_row();

                        // Remove latest (with undo)
                        ui.label("");
                        ui.horizontal(|ui| {
                            if ui
                                .add_sized([165.0, 34.0], egui::Button::new("Remove latest entry"))
                                .clicked()
                            {
                                self.remove_latest_entry();
                            }
                            if let Some(removable) = &self.undo {
                                let enabled =
                                    self.data.wallets.get(removable.wallet_index).is_some();
                                let response = ui
                                    .allocate_ui_with_layout(
                                        egui::vec2(120.0, 34.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.add_enabled(
                                                enabled,
                                                egui::Button::new(format!(
                                                    "Undo {}",
                                                    format_money(removable.entry.amount_cents)
                                                ))
                                                .min_size(egui::vec2(120.0, 34.0)),
                                            )
                                        },
                                    )
                                    .inner;
                                if response.clicked() {
                                    self.undo_remove_entry();
                                }
                            }
                        });
                        ui.end_row();

                        ui.separator();
                        ui.end_row();

                        // Delete wallet (keeps at least one)
                        ui.label("");
                        ui.horizontal(|ui| {
                            if self.confirm_delete_wallet {
                                if ui
                                    .add_sized([100.0, 34.0], egui::Button::new("Confirm delete"))
                                    .on_hover_text(
                                        "Permanently remove this wallet and all its entries.",
                                    )
                                    .clicked()
                                {
                                    self.delete_selected_wallet();
                                }
                                if ui
                                    .add_sized([75.0, 34.0], egui::Button::new("Cancel"))
                                    .clicked()
                                {
                                    self.confirm_delete_wallet = false;
                                    self.status = "Wallet deletion cancelled.".to_string();
                                }
                            } else if ui
                                .add_sized([130.0, 34.0], egui::Button::new("Delete wallet"))
                                .on_hover_text("Ask for confirmation before removing this wallet.")
                                .clicked()
                            {
                                self.confirm_delete_wallet = true;
                                self.status = format!(
                                    "Confirm deletion of {} and all its entries.",
                                    selected_name
                                );
                            }
                        });
                    });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);

                // Add wallet
                ui.label(egui::RichText::new("Add a child").strong().size(17.0));
                ui.label(
                    egui::RichText::new("Names are stored only on this device")
                        .size(12.0)
                        .color(theme::TEXT_SECONDARY),
                );
                ui.add_space(4.0);

                egui::Grid::new("settings_add_grid")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Child name").size(12.0));
                        ui.add_sized(
                            [220.0, 34.0],
                            egui::TextEdit::singleline(&mut self.new_child_name_input)
                                .hint_text("New child"),
                        );
                        ui.end_row();

                        ui.label("");
                        if ui
                            .add_sized([90.0, 34.0], egui::Button::new("Add child"))
                            .clicked()
                        {
                            self.add_child_wallet();
                        }
                    });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);

                // Parent PIN
                ui.label(egui::RichText::new("Parent PIN").strong().size(17.0));
                ui.label(
                    egui::RichText::new(
                        "The PIN encrypts local data. Four digits protect against casual access, not a determined attacker.",
                    )
                    .size(12.0)
                    .color(theme::TEXT_SECONDARY),
                );
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("New PIN").size(12.0));
                    ui.add_sized(
                        [120.0, 34.0],
                        egui::TextEdit::singleline(&mut self.new_pin_input)
                            .password(true)
                            .hint_text("4 digits"),
                    );
                    if ui
                        .add_sized([95.0, 34.0], egui::Button::new("Update PIN"))
                        .clicked()
                    {
                        self.update_pin();
                    }
                });

                ui.add_space(12.0);
                if ui
                    .add_sized([100.0, 36.0], egui::Button::new("Done"))
                    .clicked()
                {
                    self.show_settings = false;
                }
            });

        if !open {
            self.show_settings = false;
        }
    }

    pub fn entry_form(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(theme::CARD_BG)
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Add a transaction").strong().size(15.0));
                ui.label(
                    egui::RichText::new("Record money in or money out")
                        .size(11.0)
                        .color(theme::TEXT_SECONDARY),
                );
                ui.add_space(10.0);

                ui.label(egui::RichText::new("Transaction type").size(11.0).strong());
                ui.columns(2, |columns| {
                    columns[0].selectable_value(
                        &mut self.draft.kind,
                        EntryKind::Deposit,
                        "＋ Money in",
                    );
                    columns[1].selectable_value(
                        &mut self.draft.kind,
                        EntryKind::Deduction,
                        "− Money out",
                    );
                });

                ui.label(egui::RichText::new("What was it for?").size(11.0).strong());
                ui.add_sized(
                    [ui.available_width(), 36.0],
                    egui::TextEdit::singleline(&mut self.draft.description)
                        .char_limit(100)
                        .hint_text("e.g. Weekly allowance"),
                );

                ui.label(egui::RichText::new("Amount").size(11.0).strong());
                ui.add_sized(
                    [ui.available_width(), 36.0],
                    egui::TextEdit::singleline(&mut self.draft.amount).hint_text("$0.00"),
                );

                let action = match self.draft.kind {
                    EntryKind::Deposit => "Add money",
                    EntryKind::Deduction => "Record spending",
                };
                if ui
                    .add_sized(
                        [ui.available_width(), 40.0],
                        egui::Button::new(
                            egui::RichText::new(action)
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .fill(theme::ACCENT),
                    )
                    .clicked()
                {
                    self.add_entry();
                }
            });
    }

    pub fn ledger_table(&mut self, ui: &mut egui::Ui) {
        let ledger_sort = self.ledger_sort;
        let wallet = self.selected_wallet();
        let rows = wallet.ledger_rows_sorted(ledger_sort);
        let mut toggle_sort = false;

        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::initial(100.0).at_least(84.0))
            .column(egui_extras::Column::remainder().at_least(160.0))
            .column(egui_extras::Column::initial(100.0).at_least(82.0))
            .column(egui_extras::Column::initial(110.0).at_least(90.0))
            .header(34.0, |mut header| {
                header.col(|ui| {
                    let tooltip = match ledger_sort {
                        LedgerSort::NewestFirst => "Newest first — click to sort oldest first",
                        LedgerSort::OldestFirst => "Oldest first — click to sort newest first",
                    };
                    let arrow = match ledger_sort {
                        LedgerSort::NewestFirst => "↓",
                        LedgerSort::OldestFirst => "↑",
                    };
                    let response = ui
                        .horizontal(|ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                egui::RichText::new("Date")
                                    .strong()
                                    .size(12.0)
                                    .color(theme::TEXT_PRIMARY),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(arrow)
                                            .size(10.0)
                                            .color(theme::TEXT_SECONDARY),
                                    );
                                },
                            );
                        })
                        .response;
                    let response = response
                        .interact(egui::Sense::click())
                        .on_hover_text(tooltip);
                    if response.clicked() {
                        toggle_sort = true;
                    }
                });
                header.col(|ui| {
                    ui.label(
                        egui::RichText::new("Description")
                            .strong()
                            .size(12.0)
                            .color(theme::TEXT_PRIMARY),
                    );
                });
                header.col(|ui| {
                    ui.label(
                        egui::RichText::new("Amount")
                            .strong()
                            .size(12.0)
                            .color(theme::TEXT_PRIMARY),
                    );
                });
                header.col(|ui| {
                    ui.label(
                        egui::RichText::new("Balance")
                            .strong()
                            .size(12.0)
                            .color(theme::TEXT_PRIMARY),
                    );
                });
            })
            .body(|mut body| {
                for ledger_row in &rows {
                    let is_start = matches!(ledger_row.date, LedgerRowDate::Start);
                    let row_h = if is_start { 34.0 } else { 42.0 };
                    body.row(row_h, |mut row| {
                        row.col(|ui| {
                            let date_text = egui::RichText::new(ledger_row.date.label())
                                .size(if is_start { 9.0 } else { 10.0 })
                                .color(theme::TEXT_SECONDARY);
                            ui.label(date_text);
                        });
                        row.col(|ui| {
                            let desc = if is_start {
                                egui::RichText::new(ledger_row.description)
                                    .size(11.0)
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
                            let amount_prefix = if ledger_row.amount_cents > 0 && !is_start {
                                "+"
                            } else {
                                ""
                            };
                            let amt = egui::RichText::new(format!(
                                "{amount_prefix}{}",
                                format_money(ledger_row.amount_cents)
                            ))
                            .size(if is_start { 10.0 } else { 11.0 })
                            .color(amount_color(ledger_row.amount_cents));
                            ui.label(amt);
                        });
                        row.col(|ui| {
                            ui.label(
                                egui::RichText::new(format_money(ledger_row.balance_cents))
                                    .size(11.0)
                                    .strong()
                                    .color(balance_color(ledger_row.balance_cents)),
                            );
                        });
                    });
                }
            });

        if toggle_sort {
            self.ledger_sort.toggle();
        }
    }
}
