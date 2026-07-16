//! Rendering of the app's screens: the lock/PIN screen, the wallet header,
//! the entry form, the ledger table, and the settings window.
//!
//! These methods all belong to the same `impl CofferlyApp` block declared in
//! `main.rs`. Field privacy is unaffected because the block is in the same
//! crate as the struct definition.

use eframe::egui;

use crate::data::{LedgerRowDate, LedgerSort};
use crate::money::format_money;
use crate::money::format_money_input;
use crate::theme;
use crate::theme::amount_color;
use crate::theme::balance_color;
use crate::CofferlyApp;
use crate::{StatusSeverity, APP_NAME, PIN_LENGTH};

impl CofferlyApp {
    pub fn lock_screen(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(self.lock_screen_bg))
            .show(ui, |ui| {
                let viewport_height = ui.available_height();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.vertical_centered(|ui| {
                            // Preserve enough room for the privacy and first-run guidance at
                            // common laptop window heights while retaining breathing room on
                            // larger displays.
                            let compact = viewport_height < 800.0;
                            ui.add_space(if compact {
                                12.0
                            } else {
                                (viewport_height * 0.03).clamp(18.0, 30.0)
                            });

                            if let Some(texture) = &self.lock_screen_image {
                                let art_max_width = if compact { 250.0 } else { 340.0 };
                                let max_width =
                                    (ui.available_width() * 0.5).clamp(200.0, art_max_width);
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
                                    .color(theme::LOCK_TEXT_SECONDARY),
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
                                        egui::RichText::new(
                                            "Enter the 4-digit parent PIN to continue",
                                        )
                                        .size(13.0)
                                        .color(theme::LOCK_TEXT_SECONDARY),
                                    );
                                    ui.add_space(16.0);

                                    if let Some(index) = self.pending_pin_focus.take() {
                                        ui.memory_mut(|memory| {
                                            memory.request_focus(crate::pin_digit_id(index))
                                        });
                                    }

                                    let enter_pressed =
                                        ui.input(|input| input.key_pressed(egui::Key::Enter));
                                    let mut pin_changed = false;

                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 14.0;
                                        let pin_entry_width = 4.0 * 58.0 + 3.0 * 14.0;
                                        ui.add_space(
                                            ((ui.available_width() - pin_entry_width) / 2.0)
                                                .max(0.0),
                                        );

                                        for index in 0..PIN_LENGTH {
                                            let (coin_rect, _) = ui.allocate_exact_size(
                                                egui::vec2(58.0, 58.0),
                                                egui::Sense::hover(),
                                            );
                                            let response = ui.put(
                                                coin_rect,
                                                egui::TextEdit::singleline(
                                                    &mut self.pin_digits[index],
                                                )
                                                .id(crate::pin_digit_id(index))
                                                .password(true)
                                                .frame(egui::Frame::NONE)
                                                .background_color(egui::Color32::TRANSPARENT)
                                                .text_color(egui::Color32::TRANSPARENT)
                                                .horizontal_align(egui::Align::Center)
                                                .vertical_align(egui::Align::Center)
                                                .char_limit(PIN_LENGTH)
                                                .desired_width(54.0),
                                            );

                                            if response.changed() {
                                                self.normalize_pin_digit_input(index);
                                                pin_changed = true;
                                                ui.ctx().request_repaint();
                                            }

                                            if response.has_focus()
                                                && self.pin_digits[index].is_empty()
                                                && ui.input(|input| {
                                                    input.key_pressed(egui::Key::Backspace)
                                                })
                                                && index > 0
                                            {
                                                self.pending_pin_focus = Some(index - 1);
                                                ui.ctx().request_repaint();
                                            }

                                            draw_pin_coin(
                                                ui,
                                                index,
                                                coin_rect,
                                                !self.pin_digits[index].is_empty(),
                                                response.has_focus(),
                                            );
                                        }
                                    });

                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new("Gold coins fill as you type")
                                            .size(12.0)
                                            .strong()
                                            .color(theme::LOCK_TEXT_SECONDARY),
                                    );

                                    // Auto-submit as soon as the 4th digit lands (ATM / phone
                                    // lock convention). Enter and the Unlock button still work
                                    // for paste / partial flows.
                                    let should_unlock = !self.unlocking
                                        && self.parent_pin_complete()
                                        && (pin_changed || enter_pressed);

                                    if should_unlock {
                                        self.start_unlock();
                                    }

                                    ui.add_space(16.0);

                                    let unlock_enabled = !self.unlocking;
                                    let unlock_label = if self.unlocking {
                                        "Unlocking…"
                                    } else {
                                        "Unlock"
                                    };
                                    if ui
                                        .add_enabled(
                                            unlock_enabled,
                                            egui::Button::new(
                                                egui::RichText::new(unlock_label)
                                                    .size(15.0)
                                                    .color(egui::Color32::WHITE)
                                                    .strong(),
                                            )
                                            .fill(theme::ACCENT_DARK)
                                            .min_size(egui::vec2(240.0, 42.0)),
                                        )
                                        .clicked()
                                    {
                                        self.start_unlock();
                                    }
                                });

                            ui.add_space(14.0);
                            let status_color = match self.status.severity {
                                StatusSeverity::Error => theme::NEGATIVE,
                                StatusSeverity::Success => theme::POSITIVE,
                                StatusSeverity::Info => theme::LOCK_TEXT_SECONDARY,
                            };
                            let status_text = if self.status.severity == StatusSeverity::Error {
                                format!("⚠ {}", self.status.text)
                            } else {
                                self.status.text.clone()
                            };
                            ui.label(
                                egui::RichText::new(status_text)
                                    .size(13.0)
                                    .color(status_color),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new("Local-only  •  No account  •  No cloud sync")
                                    .size(13.0)
                                    .color(theme::LOCK_TEXT_SECONDARY),
                            );
                            ui.label(
                                egui::RichText::new(
                                    "First run? Use 1234, then choose a new PIN in Settings.",
                                )
                                .size(12.0)
                                .color(theme::LOCK_TEXT_SECONDARY),
                            );
                        });
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

        // Esc-to-dismiss is the universal dialog convention.
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.show_settings = false;
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

                ui.label(
                    egui::RichText::new("Wallet details")
                        .strong()
                        .size(17.0)
                        .color(theme::TEXT_PRIMARY),
                );
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
                                    self.set_status_info("Wallet deletion cancelled.");
                                }
                            } else if ui
                                .add_sized([130.0, 34.0], egui::Button::new("Delete wallet"))
                                .on_hover_text("Ask for confirmation before removing this wallet.")
                                .clicked()
                            {
                                self.confirm_delete_wallet = true;
                                self.set_status_info(format!(
                                    "Confirm deletion of {} and all its entries.",
                                    selected_name
                                ));
                            }
                        });
                    });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);

                // Add wallet
                ui.label(
                    egui::RichText::new("Add a child")
                        .strong()
                        .size(17.0)
                        .color(theme::TEXT_PRIMARY),
                );
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
                ui.label(
                    egui::RichText::new("Parent PIN")
                        .strong()
                        .size(17.0)
                        .color(theme::TEXT_PRIMARY),
                );
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
                ui.label(
                    egui::RichText::new("Add a transaction")
                        .strong()
                        .size(15.0)
                        .color(theme::TEXT_PRIMARY),
                );
                ui.label(
                    egui::RichText::new("Record money in or money out")
                        .size(12.0)
                        .color(theme::TEXT_SECONDARY),
                );
                ui.add_space(10.0);

                ui.label(
                    egui::RichText::new("Transaction type")
                        .size(11.0)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                );
                ui.scope(|ui| {
                    ui.spacing_mut().button_padding.x = 4.0;
                    ui.columns(2, |columns| {
                        columns[0].selectable_value(
                            &mut self.draft.kind,
                            crate::data::EntryKind::Deposit,
                            egui::RichText::new("Money in").size(13.0),
                        );
                        columns[1].selectable_value(
                            &mut self.draft.kind,
                            crate::data::EntryKind::Deduction,
                            egui::RichText::new("Money out").size(13.0),
                        );
                    });
                });

                ui.label(
                    egui::RichText::new("What was it for?")
                        .size(11.0)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                );
                let desc_response = ui.add_sized(
                    [ui.available_width(), 36.0],
                    egui::TextEdit::singleline(&mut self.draft.description)
                        .char_limit(100)
                        .hint_text("e.g. Weekly allowance"),
                );

                ui.label(
                    egui::RichText::new("Amount")
                        .size(11.0)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                );
                let amount_response = ui.add_sized(
                    [ui.available_width(), 36.0],
                    egui::TextEdit::singleline(&mut self.draft.amount).hint_text("$0.00"),
                );

                let enter_submit = ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && (desc_response.lost_focus()
                        || amount_response.lost_focus()
                        || desc_response.has_focus()
                        || amount_response.has_focus());

                let action = match self.draft.kind {
                    crate::data::EntryKind::Deposit => "Add money",
                    crate::data::EntryKind::Deduction => "Record spending",
                };
                let clicked = ui
                    .add_sized(
                        [ui.available_width(), 40.0],
                        egui::Button::new(
                            egui::RichText::new(action)
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .fill(theme::ACCENT_DARK),
                    )
                    .clicked();

                if clicked || enter_submit {
                    self.add_entry();
                }
            });
    }

    pub fn ledger_table(&mut self, ui: &mut egui::Ui) {
        let ledger_sort = self.ledger_sort;
        // Rebuild cache if needed, then clone the slice for the table body so we
        // do not hold a borrow across the TableBuilder (which may need &mut self).
        let rows = self.cached_ledger_rows().to_vec();
        let mut toggle_sort = false;
        const ROW_HEIGHT: f32 = 42.0;

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
                    let order_label = match ledger_sort {
                        LedgerSort::NewestFirst => "Newest",
                        LedgerSort::OldestFirst => "Oldest",
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
                                        egui::RichText::new(order_label)
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
            .body(|body| {
                // Virtualized rows: only visible rows are laid out each frame.
                body.rows(ROW_HEIGHT, rows.len(), |mut row| {
                    let index = row.index();
                    let ledger_row = &rows[index];
                    let is_start = matches!(ledger_row.date, LedgerRowDate::Start);

                    row.col(|ui| {
                        let date_text = egui::RichText::new(ledger_row.date.label())
                            .size(if is_start { 10.0 } else { 11.0 })
                            .color(theme::TEXT_SECONDARY);
                        ui.label(date_text);
                    });
                    row.col(|ui| {
                        let desc = if is_start {
                            egui::RichText::new(&ledger_row.description)
                                .size(11.0)
                                .italics()
                                .color(theme::TEXT_SECONDARY)
                        } else {
                            egui::RichText::new(&ledger_row.description)
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
            });

        if toggle_sort {
            self.ledger_sort.toggle();
            self.invalidate_ledger_cache();
        }
    }
}

fn draw_pin_coin(ui: &egui::Ui, index: usize, rect: egui::Rect, filled: bool, active: bool) {
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new(("pin_coin", index)),
    ));
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.35;

    if active {
        painter.circle_filled(center, radius + 8.0, theme::GOLD_LIGHT);
    }

    painter.circle_filled(
        center,
        radius,
        if filled { theme::GOLD } else { theme::CARD_BG },
    );
    painter.circle_stroke(
        center,
        radius,
        egui::Stroke::new(if active { 2.5 } else { 1.5 }, theme::GOLD_DARK),
    );

    if filled {
        let check_color = theme::ACCENT_DARK;
        painter.line_segment(
            [
                center + egui::vec2(-9.0, 0.0),
                center + egui::vec2(-2.0, 7.0),
            ],
            egui::Stroke::new(2.5, check_color),
        );
        painter.line_segment(
            [
                center + egui::vec2(-2.0, 7.0),
                center + egui::vec2(11.0, -8.0),
            ],
            egui::Stroke::new(2.5, check_color),
        );
    } else if active {
        painter.circle_filled(center, 3.5, theme::GOLD_DARK);
    }
}
