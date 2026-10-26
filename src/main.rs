use chrono::{Local, NaiveDate};
use eframe::egui;
use eframe::egui::Color32;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod app_export;
mod capture;
mod crypto;
mod data;
mod export_csv;
mod io;
mod money;
mod print_html;
mod prompts;
mod story;
mod theme;
mod views;

pub const APP_NAME: &str = "Cofferly";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DATA_FILE_NAME: &str = "vault.cofferly";
const PREVIOUS_DATA_FILE_NAME: &str = "data.json";
const PIN_LENGTH: usize = 4;
const LOCK_SCREEN_IMAGE_BYTES: &[u8] = include_bytes!("../assets/cofferly-lock.jpg");
const OPEN_COFFER_IMAGE_BYTES: &[u8] = include_bytes!("../assets/cofferly-open.png");
/// Forgiving default so parents are not locked mid-chore; still protects a
/// shared family PC left open.
const AUTO_LOCK_AFTER: Duration = Duration::from_secs(10 * 60);
/// Show a quiet countdown for the last two minutes before auto-lock.
const AUTO_LOCK_WARN: Duration = Duration::from_secs(2 * 60);
/// Escalating delays after consecutive wrong PINs. This slows automated UI
/// guessing without creating a permanent lockout for a parent.
const UNLOCK_COOLDOWN_MINUTES: [u64; 6] = [1, 2, 5, 15, 30, 60];
const UI_STATE_KEY: &str = "cofferly/ui_state";

use crypto::SessionCrypto;
use data::{
    child_name_taken, default_app_data, format_ledger_date, ledger_filter_summary,
    parse_ledger_date, valid_cents, valid_child_name, valid_description, AppData, Entry, EntryKind,
    LedgerSort, OwnedLedgerRow, Wallet, WeeklyAllowance,
};
use export_csv::{write_csv_ledger, write_csv_ledger_filtered};
use io::{
    cleanup_temp_print_artifacts, data_path, prepare_data_vault, reserve_private_temp_path,
    save_encrypted,
};
use money::{format_money, format_money_input, parse_dollars_to_cents};
use print_html::{ledger_file_stem, write_printable_ledger, write_printable_ledger_filtered};
use theme::{
    app_icon, balance_color, configure_style, wallet_card_chrome, WALLET_CARD_HEIGHT,
    WALLET_PICKER_WIDTH,
};

fn main() -> eframe::Result<()> {
    let capturing = std::env::var_os("COFFERLY_CAPTURE").is_some();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(if capturing {
                // Tall enough for Settings sections used in README screenshots.
                [1280.0, 1200.0]
            } else {
                [1080.0, 720.0]
            })
            .with_min_inner_size([820.0, 560.0])
            .with_title(APP_NAME)
            .with_app_id("com.cofferly.app")
            .with_icon(app_icon()),
        // Avoid restoring a previous window size over documentation captures.
        persist_window: !capturing,
        ..Default::default()
    };

    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(CofferlyApp::new(cc)))),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EntryDraft {
    description: String,
    amount: String,
    kind: EntryKind,
    date_input: String,
    /// The day `date_input` was auto-filled with, while it still holds that
    /// untouched value; `None` once the date is a typed or loaded one.
    auto_date: Option<NaiveDate>,
}

impl EntryDraft {
    fn new() -> Self {
        Self {
            description: String::new(),
            amount: String::new(),
            kind: EntryKind::Deduction,
            date_input: format_ledger_date(Local::now().date_naive()),
            auto_date: Some(Local::now().date_naive()),
        }
    }

    fn fill_today(&mut self, today: NaiveDate) {
        self.date_input = format_ledger_date(today);
        self.auto_date = Some(today);
    }
}

/// The entry most recently removed from a wallet, held briefly so the user can
/// undo the deletion. Cleared by any new mutation or by switching wallets.
#[derive(Debug, Clone, PartialEq)]
struct RemovableEntry {
    wallet_index: usize,
    entry: Entry,
    /// Where the entry sat before removal, so undo can put it back in place.
    /// `None` appends (the newest-entry removal).
    entry_index: Option<usize>,
}

/// The entry most recently corrected in place, held briefly so the user can put
/// it back. Same lifetime rules as `RemovableEntry` — one pending undo, cleared
/// by the next mutation or a wallet switch.
#[derive(Debug, Clone, PartialEq)]
struct EditedEntry {
    wallet_index: usize,
    entry_index: usize,
    previous: Entry,
}

/// The single pending undo. Removal and correction share the slot so there is
/// never more than one "undo" on offer, and it always means the last change.
#[derive(Debug, Clone, PartialEq)]
enum PendingUndo {
    Removed(RemovableEntry),
    Edited(EditedEntry),
}

/// One validated entry form submission, shared by add and correct.
#[derive(Debug, Clone)]
struct ValidatedEntryInput {
    /// Magnitude, always positive — what the warning copy talks about.
    amount: i64,
    /// What gets stored: negative for a deduction.
    signed_amount: i64,
    description: String,
    date: NaiveDate,
}

/// An entry being corrected in place.
///
/// The correction reuses the add form (and therefore its validation, focus, and
/// keyboard handling) by swapping the add draft out for the entry's values and
/// restoring it afterwards — one form, one set of rules.
#[derive(Debug, Clone)]
struct EntryEditSession {
    wallet_index: usize,
    entry_index: usize,
    original: Entry,
    stashed_draft: EntryDraft,
}

/// Pre-mutation snapshot for `save_with_success_or_restore`.
#[derive(Debug, Clone)]
struct MutationSnapshot {
    data: AppData,
    selected_wallet: usize,
    ledger_filter: String,
    ledger_filters: HashMap<String, String>,
    undo: Option<PendingUndo>,
    new_child_name_input: String,
}

/// The secret a vault write encrypts with: an established session already holds
/// the data key, so only a fresh vault needs the parent PIN.
fn save_secret_for(session: &Option<SessionCrypto>, data: &AppData) -> String {
    if session.is_some() {
        String::new()
    } else {
        data.parent_pin.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusSeverity {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub struct Status {
    pub text: String,
    pub severity: StatusSeverity,
}

impl Status {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            severity: StatusSeverity::Info,
        }
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            severity: StatusSeverity::Success,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            severity: StatusSeverity::Error,
        }
    }
}

/// Non-sensitive UI prefs restored via eframe storage (never unlock state or PINs).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct UiState {
    selected_wallet: usize,
    ledger_sort_newest_first: bool,
    /// The selected wallet's `child_name` at last save, used to restore the
    /// same wallet by identity (not position) after unlock -- an index alone
    /// would point at the wrong wallet if one was deleted, or a wrong-but-
    /// in-bounds one if the roster reordered. `#[serde(default)]` so state
    /// persisted by older builds (no such field) still deserializes; those
    /// fall back to the plain index-based restore they always had.
    #[serde(default)]
    selected_wallet_name: Option<String>,
    /// Last-selected Money in/out kind, restored into a fresh `EntryDraft` on
    /// unlock/relaunch. `#[serde(default)]` so state persisted by older
    /// builds (no such field) still deserializes -- `EntryKind::default()`
    /// is `Deduction`, matching today's hardcoded `EntryDraft::new()` value.
    #[serde(default)]
    last_entry_kind: EntryKind,
    /// Selected child's ledger filter; per-child map in `ledger_filters`. See
    /// `CofferlyApp::ledger_filter` / `ledger_filters`. `#[serde(default)]`.
    #[serde(default)]
    ledger_filter: String,
    /// Per-child ledger filters keyed by `child_name`. `#[serde(default)]`.
    #[serde(default)]
    ledger_filters: HashMap<String, String>,
    /// Date (YYYY-MM-DD) of the last verified vault backup, shown in Settings.
    /// `#[serde(default)]` so older state without it reads as "Never backed up".
    #[serde(default)]
    last_backup: Option<String>,
}

/// A backup chosen for restore (#177). Nothing on disk changes until its
/// Coffer Story decrypts it and the parent confirms the replace.
pub(crate) struct PendingRestore {
    pub(crate) file_name: String,
    bytes: Vec<u8>,
    /// Wallets in the vault being replaced (0 on a fresh install).
    pub(crate) replaces_wallets: usize,
    /// Lock screen to return to if the restore is cancelled.
    return_mode: LockMode,
    /// Set once the backup's Coffer Story decrypts it: awaiting confirmation.
    pub(crate) decrypted: Option<(AppData, SessionCrypto)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LockMode {
    SetupReveal,
    SetupConfirm,
    Story,
    LegacyPin,
    MigrateReveal,
    MigrateConfirm,
    ChangeReveal,
    ChangeConfirm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EntryFormField {
    Amount,
    Description,
    Date,
}

pub(crate) struct CofferlyApp {
    data: AppData,
    raw_bytes: Option<Vec<u8>>,
    /// Present while parent mode is unlocked; enables saves without re-running Argon2id.
    session: Option<SessionCrypto>,
    selected_wallet: usize,
    ledger_sort: LedgerSort,
    /// Cached sorted ledger for the selected wallet; invalidated on mutation / selection / sort.
    /// `Arc` so handing a copy to the table each frame is a pointer bump, not a
    /// re-allocation of every row's description.
    ledger_cache: Option<(usize, LedgerSort, Arc<[OwnedLedgerRow]>)>,
    /// Local ledger description filter (display-only): narrows `ledger_table`
    /// rows without mutating `Wallet::entries`, sort order, or `ledger_cache`.
    /// Persisted via `UiState`; selected child's string, others in `ledger_filters`.
    ledger_filter: String,
    /// Per-child ledger filters keyed by `child_name` (same display-only rules).
    ledger_filters: HashMap<String, String>,
    /// Set by the `/` shortcut, consumed (and cleared) the next time
    /// `ledger_table` renders the filter field.
    pending_ledger_filter_focus: bool,
    draft: EntryDraft,
    starting_balance_input: String,
    /// Settings field for the selected wallet's weekly allowance (#179); blank = off.
    weekly_allowance_input: String,
    /// Settings field for the selected wallet's savings goal (#181); blank = none.
    savings_goal_input: String,
    child_name_input: String,
    new_child_name_input: String,
    pin_digits: [String; PIN_LENGTH],
    pending_pin_focus: Option<usize>,
    pending_entry_focus: Option<EntryFormField>,
    lock_mode: LockMode,
    pending_story: Option<[&'static str; story::STORY_LENGTH]>,
    story_selections: Vec<&'static str>,
    display_order: Vec<&'static str>,
    story_icon_textures: HashMap<&'static str, egui::TextureHandle>,
    parent_unlocked: bool,
    save_enabled: bool,
    /// True for the launch that copied `data.json`; keeps the recovery reminder
    /// visible after the parent proves the new vault can be decrypted.
    previous_data_backup_preserved: bool,
    status: Status,
    data_path: PathBuf,
    lock_screen_image: Option<egui::TextureHandle>,
    lock_screen_bg: egui::Color32,
    open_coffer_image: Option<egui::TextureHandle>,
    show_settings: bool,
    /// Pending confirms and the undo slot; see `prompts.rs`.
    prompts: prompts::Prompts,
    entry_edit: Option<EntryEditSession>,
    pending_ledger_edit_focus: Option<usize>,
    last_interaction: Instant,
    /// True while Argon2id / decrypt runs off the UI thread.
    unlocking: bool,
    unlock_rx: Option<std::sync::mpsc::Receiver<BackgroundCryptoResult>>,
    failed_unlock_attempts: u32,
    unlock_cooldown_until: Option<Instant>,
    /// Maintainer-only README capture sequence (`COFFERLY_CAPTURE`).
    capture: Option<capture::CaptureSession>,
    /// Whether `COFFERLY_CAPTURE` was set at launch. Read once here instead of
    /// re-reading the environment every frame — it cannot change mid-run.
    capturing: bool,
    /// Paths of temp exports/recovery cards written this session, so they can
    /// be deleted on lock/exit instead of lingering until the next launch.
    temp_artifact_paths: Vec<PathBuf>,
    /// The wallet name persisted from a previous run, awaiting resolution
    /// against the real wallet list once a vault unlock succeeds (the vault
    /// is still encrypted at construction time, so it can't be resolved to
    /// an index yet). Consumed (set to `None`) by the first `apply_unlock`.
    pending_wallet_selection_name: Option<String>,
    /// Date of the last verified backup, persisted via `UiState`.
    last_backup: Option<String>,
    /// Restore in progress; drives the lock screen while `Some`.
    restore: Option<PendingRestore>,
}

enum BackgroundCryptoResult {
    Unlock(Result<(AppData, SessionCrypto), String>),
    RestoreDecrypt(Result<(AppData, SessionCrypto), String>),
    StorySetup {
        lock_mode: LockMode,
        outcome: Result<StorySetupSuccess, StorySetupError>,
    },
}

struct StorySetupSuccess {
    session: SessionCrypto,
    encrypted: Vec<u8>,
    lock_mode: LockMode,
}

enum StorySetupError {
    Establish,
    Rewrap {
        message: String,
        session: Box<SessionCrypto>,
    },
    SaveFresh {
        message: String,
    },
    SaveRewrap {
        lock_mode: LockMode,
        message: String,
    },
}

fn run_story_setup(
    lock_mode: LockMode,
    secret: &str,
    data: &AppData,
    data_path: &Path,
    existing_session: Option<SessionCrypto>,
) -> Result<StorySetupSuccess, StorySetupError> {
    match lock_mode {
        LockMode::SetupConfirm => {
            let mut session_opt = match SessionCrypto::establish(secret) {
                Ok(session) => Some(session),
                Err(_) => return Err(StorySetupError::Establish),
            };
            match io::save_encrypted(data_path, data, secret, &mut session_opt) {
                Ok(encrypted) => {
                    let session = match session_opt {
                        Some(session) => session,
                        None => {
                            return Err(StorySetupError::SaveFresh {
                                message: "Session missing after save.".into(),
                            });
                        }
                    };
                    Ok(StorySetupSuccess {
                        session,
                        encrypted,
                        lock_mode,
                    })
                }
                Err(message) => Err(StorySetupError::SaveFresh { message }),
            }
        }
        LockMode::MigrateConfirm | LockMode::ChangeConfirm => {
            let mut session = existing_session.expect("session checked before spawn");
            if let Err(message) = session.rewrap_for_secret(secret) {
                return Err(StorySetupError::Rewrap {
                    message,
                    session: Box::new(session),
                });
            }
            let mut session_opt = Some(session);
            match io::save_encrypted(data_path, data, secret, &mut session_opt) {
                Ok(encrypted) => {
                    let session = match session_opt {
                        Some(session) => session,
                        None => {
                            return Err(StorySetupError::SaveRewrap {
                                lock_mode,
                                message: "Session missing after save.".into(),
                            });
                        }
                    };
                    Ok(StorySetupSuccess {
                        session,
                        encrypted,
                        lock_mode,
                    })
                }
                Err(message) => Err(StorySetupError::SaveRewrap { lock_mode, message }),
            }
        }
        _ => unreachable!("caller validates lock mode"),
    }
}

impl CofferlyApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_style(&cc.egui_ctx);
        cleanup_temp_print_artifacts();

        let data_path = data_path();
        let (preserved_previous_file, preparation_error) = match prepare_data_vault() {
            Ok(preparation) => (preparation.preserved_previous_file, None),
            Err(err) => (None, Some(err)),
        };
        let previous_data_backup_preserved = preserved_previous_file.is_some();
        let (raw_bytes, storage_error) = if let Some(err) = preparation_error {
            (None, Some(err))
        } else {
            match io::load_raw(&data_path) {
                Ok(raw_bytes) => (raw_bytes, None),
                Err(err) => (None, Some(err)),
            }
        };

        let (save_enabled, lock_mode, status) = if let Some(err) = storage_error {
            (
                false,
                LockMode::Story,
                Status::error(format!(
                    "Could not prepare saved data: {err}. Changes are disabled."
                )),
            )
        } else if let Some(bytes) = &raw_bytes {
            if crypto::is_current_format(bytes) {
                let mode = if bytes.first() == Some(&crypto::LEGACY_PIN_VERSION) {
                    LockMode::LegacyPin
                } else {
                    LockMode::Story
                };
                let message = if previous_data_backup_preserved {
                    if mode == LockMode::LegacyPin {
                        "Copied encrypted data into vault.cofferly. The original data.json remains untouched as a backup. Enter the legacy PIN to enroll a Coffer Story."
                    } else {
                        "Copied encrypted data into vault.cofferly. The original data.json remains untouched as a backup. Choose your Coffer Story to verify the vault."
                    }
                } else if mode == LockMode::LegacyPin {
                    "Enter the legacy 4-digit PIN to migrate to Coffer Story."
                } else {
                    "Choose your Coffer Story to unlock Cofferly."
                };
                (true, mode, Status::info(message))
            } else {
                (
                    false,
                    LockMode::Story,
                    Status::error(
                        "Saved data uses an unsupported format. Move the file aside to start fresh, or restore a current encrypted backup. Changes are disabled.",
                    ),
                )
            }
        } else {
            (
                true,
                LockMode::SetupReveal,
                Status::info("Cofferly created a six-object Coffer Story for you."),
            )
        };
        let data = default_app_data();

        // An on-disk vault's real wallet count isn't known yet -- it's still
        // encrypted, and `data` above is only the two-wallet placeholder
        // until `apply_unlock` replaces it. Clamping the persisted selection
        // against that placeholder here would silently snap a family with 3+
        // wallets back to wallet 2 on every restart; `apply_unlock` already
        // re-clamps once the real wallet count is known. Only a fresh
        // install (no vault to decrypt) needs the eager bound, since it
        // never routes through `apply_unlock`.
        let restore_wallet_bound = if raw_bytes.is_some() {
            usize::MAX
        } else {
            data.wallets.len()
        };
        let restored = restore_ui_state(cc.storage, restore_wallet_bound);
        let mut draft = EntryDraft::new();
        draft.kind = restored.entry_kind;
        let (lock_screen_image, lock_screen_bg) = load_lock_screen_image(&cc.egui_ctx);
        let open_coffer_image = load_open_coffer_image(&cc.egui_ctx);
        let story_icon_textures = load_story_icon_textures(&cc.egui_ctx);

        Self {
            data,
            raw_bytes,
            session: None,
            selected_wallet: restored.wallet_index,
            ledger_sort: restored.ledger_sort,
            ledger_cache: None,
            ledger_filter: restored.ledger_filter,
            ledger_filters: restored.ledger_filters,
            pending_ledger_filter_focus: false,
            draft,
            starting_balance_input: String::new(),
            weekly_allowance_input: String::new(),
            savings_goal_input: String::new(),
            child_name_input: String::new(),
            new_child_name_input: String::new(),
            pin_digits: Default::default(),
            pending_pin_focus: Some(0),
            pending_entry_focus: None,
            lock_mode,
            pending_story: story::generate().ok(),
            story_selections: Vec::new(),
            display_order: story::shuffled_catalog()
                .unwrap_or_else(|_| story::CATALOG.iter().map(|(id, _)| *id).collect()),
            story_icon_textures,
            parent_unlocked: false,
            save_enabled,
            previous_data_backup_preserved,
            status,
            data_path,
            lock_screen_image,
            lock_screen_bg,
            open_coffer_image,
            show_settings: false,
            prompts: prompts::Prompts::default(),
            entry_edit: None,
            pending_ledger_edit_focus: None,
            last_interaction: Instant::now(),
            unlocking: false,
            unlock_rx: None,
            failed_unlock_attempts: 0,
            unlock_cooldown_until: None,
            capture: capture::CaptureSession::from_env(),
            capturing: std::env::var_os("COFFERLY_CAPTURE").is_some(),
            temp_artifact_paths: Vec::new(),
            pending_wallet_selection_name: restored.wallet_name,
            last_backup: restored.last_backup_date,
            restore: None,
        }
    }

    fn set_status_info(&mut self, text: impl Into<String>) {
        self.status = Status::info(text);
    }

    fn set_status_ok(&mut self, text: impl Into<String>) {
        self.status = Status::success(text);
    }

    fn set_status_err(&mut self, text: impl Into<String>) {
        self.status = Status::error(text);
    }

    fn invalidate_ledger_cache(&mut self) {
        self.ledger_cache = None;
    }

    fn cached_ledger_rows(&mut self) -> Arc<[OwnedLedgerRow]> {
        let wallet_index = self.selected_wallet;
        let sort = self.ledger_sort;

        match &self.ledger_cache {
            Some((idx, cached_sort, rows)) if *idx == wallet_index && *cached_sort == sort => {
                rows.clone()
            }
            _ => {
                let rows: Arc<[OwnedLedgerRow]> = self.data.wallets[wallet_index]
                    .ledger_rows_sorted_owned(sort)
                    .into();
                self.ledger_cache = Some((wallet_index, sort, rows.clone()));
                rows
            }
        }
    }

    fn selected_wallet(&self) -> &Wallet {
        &self.data.wallets[self.selected_wallet]
    }

    fn selected_wallet_mut(&mut self) -> &mut Wallet {
        &mut self.data.wallets[self.selected_wallet]
    }

    /// Switches the active wallet from the sidebar. Settings promises "Undo
    /// remains available until the next wallet change", so a pending undo
    /// from a different wallet must not survive this — otherwise clicking
    /// Undo after switching wallets would silently restore an entry into a
    /// wallet the parent is no longer looking at.
    fn select_wallet(&mut self, index: usize) {
        if index != self.selected_wallet {
            self.remember_selected_ledger_filter();
            self.selected_wallet = index;
            self.ledger_filter = self.ledger_filter_for_selected();
        }
        self.prompts.dismiss_for_wallet_switch();
        self.invalidate_ledger_cache();
    }

    /// Stash the live query under the selected child's name. In-memory only —
    /// eframe writeback happens in `App::save`.
    fn remember_selected_ledger_filter(&mut self) {
        if let Some(name) = self
            .data
            .wallets
            .get(self.selected_wallet)
            .map(|wallet| wallet.child_name.clone())
        {
            self.ledger_filters.insert(name, self.ledger_filter.clone());
        }
    }

    fn ledger_filter_for_selected(&self) -> String {
        self.data
            .wallets
            .get(self.selected_wallet)
            .and_then(|wallet| self.ledger_filters.get(&wallet.child_name).cloned())
            .unwrap_or_default()
    }

    /// ↑/↓ or `[`/`]` move the selected wallet when focus is in the sidebar
    /// or main ledger — not while a text field or Settings has the keyboard.
    fn handle_wallet_keyboard_nav(&mut self, ctx: &egui::Context) {
        if self.show_settings || ctx.text_edit_focused() {
            return;
        }
        let Some(delta) = consume_wallet_keyboard_delta(ctx) else {
            return;
        };
        self.apply_wallet_keyboard_delta(delta);
    }

    /// `/` focuses the ledger description filter — not while a text field
    /// already has focus (so typing a literal `/` elsewhere, e.g. in the
    /// entry form, is unaffected) or Settings is open. The lock/story screen
    /// returns from `ui()` before this is ever called, so it's inert there
    /// without needing its own guard.
    fn handle_ledger_filter_shortcut(&mut self, ctx: &egui::Context) {
        if self.show_settings || ctx.text_edit_focused() {
            return;
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Slash)) {
            self.pending_ledger_filter_focus = true;
        }
    }

    fn apply_wallet_keyboard_delta(&mut self, delta: isize) {
        let next = next_wallet_index(self.selected_wallet, self.data.wallets.len(), delta);
        if next == self.selected_wallet {
            return;
        }
        self.select_wallet(next);
        let announcement = {
            let wallet = self.selected_wallet();
            wallet_selection_announcement(&wallet.child_name, wallet.current_balance_cents())
        };
        self.set_status_info(announcement);
    }

    /// Start PIN verification. Heavy Argon2id work runs on a background thread so
    /// the window stays responsive; results are applied in [`Self::poll_unlock`].
    fn start_unlock(&mut self) {
        if self.unlocking {
            return;
        }

        if let Some(remaining) = self.unlock_cooldown_remaining() {
            self.clear_pin_digits();
            self.set_status_err(format!(
                "Too many wrong PIN attempts. Try again in {}.",
                format_cooldown(remaining)
            ));
            return;
        }

        if !self.save_enabled {
            self.clear_pin_digits();
            self.set_status_err(
                "Cannot unlock while the saved data file is unreadable or unsupported.",
            );
            return;
        }

        let entered = self.entered_parent_pin();
        if entered.len() != PIN_LENGTH {
            self.set_status_err("Enter all 4 digits of the parent PIN.");
            return;
        }

        // Background path only for encrypted blobs (Argon2id is expensive).
        if let Some(raw) = &self.raw_bytes {
            if crypto::is_current_format(raw) {
                let raw = raw.clone();
                let pin = entered;
                let (tx, rx) = std::sync::mpsc::channel();
                self.unlock_rx = Some(rx);
                self.unlocking = true;
                self.set_status_info("Unlocking…");
                std::thread::spawn(move || {
                    let outcome = match io::decrypt_app_data(&raw, &pin) {
                        Ok((normalized, session)) => Ok((normalized, session)),
                        Err(io::DecryptAppDataError::Decrypt(_)) => {
                            Err("Wrong PIN or data has been tampered with.".to_string())
                        }
                        Err(io::DecryptAppDataError::Parse(err)) => {
                            Err(format!("Could not parse decrypted data: {err}"))
                        }
                        Err(io::DecryptAppDataError::Invalid) => {
                            Err("Saved data is invalid after decryption.".to_string())
                        }
                    };
                    let _ = tx.send(BackgroundCryptoResult::Unlock(outcome));
                });
                return;
            }
        }

        // A clean first run does not need Argon2 yet, so stay on the UI thread.
        self.unlock_parent_sync();
    }

    /// Synchronous unlock for first-run paths and unit tests.
    fn unlock_parent_sync(&mut self) {
        let entered = self.entered_parent_pin();

        if let Some(raw) = &self.raw_bytes {
            if !crypto::is_current_format(raw) {
                self.clear_pin_digits();
                self.session = None;
                self.set_status_err(
                    "Cannot unlock while the saved data file is unreadable or unsupported.",
                );
                return;
            }

            match io::decrypt_app_data(raw, &entered) {
                Ok((normalized, session)) => {
                    self.apply_unlock(normalized, session);
                    return;
                }
                Err(_) => {
                    self.clear_pin_digits();
                    self.session = None;
                    self.register_unlock_failure(
                        "Wrong credential or data has been tampered with.",
                    );
                    return;
                }
            }
        }

        // First run: establish a session when the parent first saves, but unlock now.
        if entered == self.data.parent_pin {
            self.parent_unlocked = true;
            self.clear_pin_digits();
            self.invalidate_ledger_cache();
            self.touch_interaction();
            self.reset_pin_failures();
            self.set_status_ok("Parent mode unlocked.");
        } else {
            self.clear_pin_digits();
            self.register_unlock_failure("Wrong PIN.");
        }
    }

    /// Opens the vault, then posts any weekly allowance due as of today. Returns
    /// the allowance status (also folded into the status line) so callers that
    /// replace the status line, like restore, can keep it.
    fn apply_unlock(&mut self, data: AppData, session: SessionCrypto) -> Option<Status> {
        self.apply_unlock_on(data, session, Local::now().date_naive())
    }

    fn apply_unlock_on(
        &mut self,
        data: AppData,
        session: SessionCrypto,
        today: NaiveDate,
    ) -> Option<Status> {
        // Restore the wallet the parent had open last, by identity rather
        // than position -- an index alone can't tell a deleted wallet from
        // a merely-reordered one. `pending_wallet_selection_name` is only
        // populated once (from the last run's persisted state), so a
        // second unlock later in the same run (e.g. after an in-process
        // lock) falls through to the index branch, which by then already
        // holds whatever the parent had selected before locking.
        let requested_wallet_name = self.pending_wallet_selection_name.take();
        self.selected_wallet = match requested_wallet_name.as_deref() {
            Some(name) => data
                .wallets
                .iter()
                .position(|wallet| wallet.child_name == name)
                // Named wallet no longer exists (deleted) -- fall back to
                // the first wallet, per issue #135.
                .unwrap_or(0),
            // No name was ever persisted (state saved by a pre-#135 build,
            // or this is a later unlock in the same run): keep the
            // existing index-based clamp so upgrading doesn't regress #113.
            None if self.selected_wallet < data.wallets.len() => self.selected_wallet,
            None => 0,
        };
        self.data = data;
        // Relaunch restores `ledger_filter` as the last selected kid's query.
        // If that child is gone, swap to the fallback kid's saved string
        // (missing → empty) so we don't carry a deleted child's search.
        // Same-run lock/unlock has no pending name — leave the live query.
        if let Some(requested) = requested_wallet_name {
            let resolved = self
                .data
                .wallets
                .get(self.selected_wallet)
                .map(|wallet| wallet.child_name.as_str());
            if resolved != Some(requested.as_str()) {
                self.ledger_filter = self.ledger_filter_for_selected();
            }
        }
        let legacy = session.version() == crypto::LEGACY_PIN_VERSION;
        self.session = Some(session);
        self.parent_unlocked = !legacy;
        self.clear_pin_digits();
        self.reset_story_entry();
        self.invalidate_ledger_cache();
        self.touch_interaction();
        self.reset_pin_failures();
        self.refresh_draft_date_on(today);
        if legacy {
            self.lock_mode = LockMode::MigrateReveal;
            self.pending_story = story::generate().ok();
            self.set_status_info(
                "Legacy PIN accepted. Enroll your Coffer Story to finish migration.",
            );
            None
        } else {
            let unlocked = if self.previous_data_backup_preserved {
                "Coffer Story unlocked. Verify your wallets before removing the data.json backup."
            } else {
                "Coffer Story unlocked."
            };
            let allowance = self.post_due_allowances(today);
            match &allowance {
                None => self.set_status_ok(unlocked),
                Some(status) => {
                    self.status = Status {
                        text: format!("{unlocked} {}", status.text),
                        severity: status.severity,
                    }
                }
            }
            allowance
        }
    }

    /// Posts every wallet's due weekly allowance (#179) into a copy of the
    /// ledger and saves that copy; the in-memory ledger is replaced only after
    /// the encrypted save succeeds. Entries and each wallet's `last_posted`
    /// travel in the same vault write, so a failed save changes nothing (the
    /// next unlock simply tries again) and a successful one can't be repeated.
    fn post_due_allowances(&mut self, today: NaiveDate) -> Option<Status> {
        if !self.save_enabled || !self.parent_unlocked {
            return None;
        }

        let mut next = self.data.clone();
        let mut posted = Vec::new();
        let mut capped = Vec::new();
        let mut at_limit = Vec::new();
        for wallet in &mut next.wallets {
            let result = wallet.post_due_allowance(today);
            if result.posted > 0 {
                posted.push(format!(
                    "{} for {}",
                    allowance_entry_count_label(result.posted),
                    wallet.child_name
                ));
            }
            if result.skipped_weeks > 0 {
                capped.push(format!(
                    "{} ({} older {} skipped)",
                    wallet.child_name,
                    result.skipped_weeks,
                    if result.skipped_weeks == 1 {
                        "week"
                    } else {
                        "weeks"
                    }
                ));
            }
            if result.stopped_at_limit {
                at_limit.push(wallet.child_name.clone());
            }
        }

        let mut parts = Vec::new();
        if !posted.is_empty() {
            let secret = save_secret_for(&self.session, &next);
            match save_encrypted(&self.data_path, &next, &secret, &mut self.session) {
                Ok(encrypted) => {
                    self.raw_bytes = Some(encrypted);
                    self.data = next;
                    self.invalidate_ledger_cache();
                }
                Err(err) => {
                    return Some(Status::error(format!(
                        "Weekly allowance was not added: could not save ({err}). Nothing changed; it will be tried again at the next unlock."
                    )));
                }
            }
            parts.push(format!("Weekly allowance added: {}.", posted.join(", ")));
            if !capped.is_empty() {
                parts.push(format!(
                    "Catch-up is capped at {} weeks: {}.",
                    data::MAX_ALLOWANCE_CATCH_UP_WEEKS,
                    capped.join(", ")
                ));
            }
        }
        if !at_limit.is_empty() {
            parts.push(format!(
                "Weekly allowance paused for {}: another deposit would exceed Cofferly's supported balance.",
                at_limit.join(", ")
            ));
        }

        if parts.is_empty() {
            None
        } else if at_limit.is_empty() {
            Some(Status::success(parts.join(" ")))
        } else {
            Some(Status::error(parts.join(" ")))
        }
    }

    fn poll_unlock(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.unlock_rx else {
            return;
        };

        match rx.try_recv() {
            Ok(result) => {
                self.unlocking = false;
                self.unlock_rx = None;
                match result {
                    BackgroundCryptoResult::Unlock(outcome) => match outcome {
                        Ok((data, session)) => {
                            self.apply_unlock(data, session);
                        }
                        Err(err) => {
                            self.clear_pin_digits();
                            self.reset_story_entry();
                            self.session = None;
                            self.register_unlock_failure(&err);
                        }
                    },
                    BackgroundCryptoResult::RestoreDecrypt(outcome) => {
                        self.apply_restore_decrypt(outcome)
                    }
                    BackgroundCryptoResult::StorySetup { lock_mode, outcome } => {
                        if self.lock_mode != lock_mode {
                            if let Err(StorySetupError::Rewrap { session, .. }) = outcome {
                                if self.session.is_none() {
                                    self.session = Some(*session);
                                }
                            }
                        } else {
                            match outcome {
                                Ok(success) => self.apply_story_setup_success(success),
                                Err(err) => self.apply_story_setup_failure(err),
                            }
                        }
                    }
                }
                ctx.request_repaint();
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                // Argon2id is running on another thread; polling at max FPS just
                // burns CPU alongside it. The spinner stays responsive at ~20Hz.
                ctx.request_repaint_after(Duration::from_millis(50));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.unlocking = false;
                self.unlock_rx = None;
                self.set_status_err("Unlock failed unexpectedly. Try again.");
            }
        }
    }

    fn lock_parent(&mut self) {
        self.parent_unlocked = false;
        self.session = None;
        self.show_settings = false;
        self.prompts.dismiss_all();
        if let Some(session) = self.entry_edit.take() {
            self.draft = session.stashed_draft;
        }
        self.clear_pin_digits();
        self.cleanup_temp_artifacts();
        self.set_status_info("Locked. Unlock parent mode to make changes.");
    }

    /// Tracks a temp export/recovery-card path so it can be deleted on lock/exit.
    fn track_temp_artifact(&mut self, path: PathBuf) {
        self.temp_artifact_paths.push(path);
    }

    /// Deletes every tracked temp artifact. Best-effort: a file already opened
    /// by another app may still be in use, and that's fine to leave for the
    /// next launch's `cleanup_temp_print_artifacts` sweep.
    fn cleanup_temp_artifacts(&mut self) {
        for path in self.temp_artifact_paths.drain(..) {
            let _ = std::fs::remove_file(path);
        }
    }

    fn auto_lock_if_idle(&mut self, ctx: &egui::Context) {
        if !self.parent_unlocked {
            return;
        }

        let idle = self.last_interaction.elapsed();
        if idle >= AUTO_LOCK_AFTER {
            self.lock_parent();
            self.set_status_info("Locked automatically after inactivity.");
            return;
        }

        let remaining = AUTO_LOCK_AFTER.saturating_sub(idle);
        if remaining <= AUTO_LOCK_WARN {
            ctx.request_repaint_after(Duration::from_secs(1));
        } else {
            ctx.request_repaint_after(remaining.saturating_sub(AUTO_LOCK_WARN));
        }
    }

    fn auto_lock_remaining(&self) -> Option<Duration> {
        if !self.parent_unlocked {
            return None;
        }
        Some(AUTO_LOCK_AFTER.saturating_sub(self.last_interaction.elapsed()))
    }

    fn touch_interaction(&mut self) {
        self.last_interaction = Instant::now();
    }

    fn unlock_cooldown_remaining(&self) -> Option<Duration> {
        self.unlock_cooldown_until
            .and_then(|until| until.checked_duration_since(Instant::now()))
            .filter(|remaining| !remaining.is_zero())
    }

    fn register_unlock_failure(&mut self, message: &str) {
        self.failed_unlock_attempts = self.failed_unlock_attempts.saturating_add(1);
        let cooldown = unlock_cooldown_duration(self.failed_unlock_attempts);
        if cooldown.is_zero() {
            self.unlock_cooldown_until = None;
            self.set_status_err(message.to_owned());
            return;
        }
        self.unlock_cooldown_until = Some(Instant::now() + cooldown);
        self.set_status_err(format!(
            "{message} Try again in {}.",
            format_cooldown(cooldown)
        ));
    }

    fn reset_pin_failures(&mut self) {
        self.failed_unlock_attempts = 0;
        self.unlock_cooldown_until = None;
    }

    fn reset_story_entry(&mut self) {
        self.story_selections.clear();
        if let Ok(order) = story::shuffled_catalog() {
            self.display_order = order;
        }
    }

    fn regenerate_story(&mut self) {
        match story::generate() {
            Ok(story) => {
                self.pending_story = Some(story);
            }
            Err(err) => self.set_status_err(err),
        }
    }

    pub(crate) fn begin_story_change(&mut self) {
        if !self.can_change("Unlock parent mode before changing the Coffer Story.") {
            return;
        }
        match story::generate() {
            Ok(story) => {
                self.pending_story = Some(story);
                self.reset_story_entry();
                self.parent_unlocked = false;
                self.show_settings = false;
                self.lock_mode = LockMode::ChangeReveal;
                self.set_status_info("Cofferly generated a replacement Coffer Story.");
            }
            Err(err) => self.set_status_err(err),
        }
    }

    /// Cancels a Coffer Story change and restores the already-unlocked parent
    /// mode. No crypto work is needed: the session was never rewrapped and
    /// the vault file was never touched until a successful confirm.
    pub(crate) fn cancel_story_change(&mut self) {
        self.pending_story = None;
        self.reset_story_entry();
        self.lock_mode = LockMode::Story;
        self.parent_unlocked = true;
        self.set_status_info("Coffer Story unchanged.");
    }

    /// Cancels a legacy-PIN-to-Coffer-Story migration entirely, dropping the
    /// in-memory session and returning to the PIN screen. Mirrors what the
    /// failed-save path in `confirm_story_setup` already does.
    pub(crate) fn cancel_story_migration(&mut self) {
        self.session = None;
        self.pending_story = None;
        self.clear_pin_digits();
        self.reset_story_entry();
        self.lock_mode = LockMode::LegacyPin;
        self.set_status_info("Migration cancelled. Enter the legacy PIN to unlock.");
    }

    /// Returns from a confirm step to the matching reveal step so the parent
    /// can look at the story again. The pending story stays in memory by
    /// design until a successful (or cancelled) confirm.
    pub(crate) fn back_to_story_reveal(&mut self) {
        self.lock_mode = match self.lock_mode {
            LockMode::SetupConfirm => LockMode::SetupReveal,
            LockMode::MigrateConfirm => LockMode::MigrateReveal,
            LockMode::ChangeConfirm => LockMode::ChangeReveal,
            other => other,
        };
        self.reset_story_entry();
        self.set_status_info("Here is your Coffer Story again.");
    }

    pub(crate) fn print_recovery_card(&mut self) {
        let Some(story) = self.pending_story else {
            self.set_status_err("No Coffer Story is available to print.");
            return;
        };
        let items = story
            .iter()
            .enumerate()
            .map(|(index, id)| {
                format!(
                    "<li><strong>{}</strong> — {}</li>",
                    index + 1,
                    story::label(id).unwrap_or(id)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let html = format!(
            "<!doctype html><meta charset=\"utf-8\"><title>Cofferly recovery card</title><h1>Cofferly recovery card</h1><p>This six-object Coffer Story unlocks your encrypted ledger. Store this card away from the computer and children. Without it, recovery is impossible.</p><ol>{items}</ol>"
        );
        let path = match reserve_private_temp_path("recovery-card", "html") {
            Ok(path) => path,
            Err(err) => {
                self.set_status_err(format!("Could not create recovery card: {err}"));
                return;
            }
        };
        match std::fs::write(&path, html)
            .and_then(|_| opener::open(&path).map_err(std::io::Error::other))
        {
            Ok(()) => {
                self.track_temp_artifact(path);
                self.set_status_ok("Opened recovery card. Store the printed copy safely.");
            }
            Err(err) => {
                let _ = std::fs::remove_file(&path);
                self.set_status_err(format!("Could not create recovery card: {err}"));
            }
        }
    }

    fn confirm_story_setup(&mut self) {
        if self.unlocking {
            return;
        }

        let Some(story) = self.pending_story else {
            self.set_status_err("Could not create a Coffer Story.");
            return;
        };
        let Ok(secret) = story::encode(&story) else {
            self.set_status_err("Could not encode Coffer Story.");
            return;
        };

        let lock_mode = self.lock_mode;
        match lock_mode {
            LockMode::SetupConfirm | LockMode::MigrateConfirm | LockMode::ChangeConfirm => {}
            _ => return,
        }

        let existing_session = match lock_mode {
            LockMode::MigrateConfirm | LockMode::ChangeConfirm => match self.session.take() {
                Some(session) => Some(session),
                None => {
                    self.set_status_err(
                        "Your unlock session expired. Unlock Cofferly again to retry.",
                    );
                    return;
                }
            },
            _ => None,
        };

        let data = self.data.clone();
        let data_path = self.data_path.clone();
        self.unlocking = true;
        self.set_status_info("Saving Coffer Story…");
        let (tx, rx) = std::sync::mpsc::channel();
        self.unlock_rx = Some(rx);
        std::thread::spawn(move || {
            let outcome = run_story_setup(lock_mode, &secret, &data, &data_path, existing_session);
            let _ = tx.send(BackgroundCryptoResult::StorySetup { lock_mode, outcome });
        });
    }

    fn apply_story_setup_success(&mut self, success: StorySetupSuccess) {
        self.session = Some(success.session);
        self.raw_bytes = Some(success.encrypted);
        match success.lock_mode {
            LockMode::SetupConfirm => {
                self.parent_unlocked = true;
                self.lock_mode = LockMode::Story;
                self.pending_story = None;
                self.reset_story_entry();
                self.reset_pin_failures();
                self.set_status_ok("Coffer Story saved. Parent mode unlocked.");
            }
            LockMode::MigrateConfirm => {
                self.data.parent_pin.clear();
                self.parent_unlocked = true;
                self.lock_mode = LockMode::Story;
                self.pending_story = None;
                self.reset_story_entry();
                self.reset_pin_failures();
                self.set_status_ok(
                    "Coffer Story enrolled. Legacy PIN no longer unlocks this file.",
                );
            }
            LockMode::ChangeConfirm => {
                self.data.parent_pin.clear();
                self.parent_unlocked = true;
                self.lock_mode = LockMode::Story;
                self.pending_story = None;
                self.reset_story_entry();
                self.reset_pin_failures();
                self.set_status_ok(
                    "Coffer Story changed. The previous story no longer unlocks this file.",
                );
            }
            _ => {}
        }
    }

    fn apply_story_setup_failure(&mut self, err: StorySetupError) {
        match err {
            StorySetupError::Establish => {
                self.session = None;
                self.set_status_err("Could not secure the new Coffer Story.");
            }
            StorySetupError::Rewrap { message, session } => {
                self.session = Some(*session);
                self.set_status_err(format!("Could not prepare migration: {message}"));
            }
            StorySetupError::SaveFresh { message } => {
                self.session = None;
                self.set_status_err(format!("Could not save new Coffer Story: {message}"));
            }
            StorySetupError::SaveRewrap { lock_mode, message } => {
                self.session = None;
                self.lock_mode = if lock_mode == LockMode::MigrateConfirm {
                    LockMode::LegacyPin
                } else {
                    LockMode::Story
                };
                self.clear_pin_digits();
                self.reset_story_entry();
                self.set_status_err(format!(
                    "Could not save the new Coffer Story: {message}. The existing encrypted file is unchanged; unlock it again to retry."
                ));
            }
        }
    }

    fn submit_story(&mut self) {
        if self.story_selections.len() != story::STORY_LENGTH {
            return;
        }
        if let Some(remaining) = self.unlock_cooldown_remaining() {
            self.reset_story_entry();
            self.set_status_err(format!(
                "Too many wrong attempts. Try again in {}.",
                format_cooldown(remaining)
            ));
            return;
        }
        match self.lock_mode {
            LockMode::SetupConfirm | LockMode::MigrateConfirm | LockMode::ChangeConfirm => {
                if self
                    .pending_story
                    .as_ref()
                    .is_some_and(|expected| expected.as_slice() == self.story_selections.as_slice())
                {
                    self.confirm_story_setup();
                } else {
                    // A mismatch here can't be an attacker guessing — the
                    // expected story was just shown one screen back — so this
                    // never touches the wrong-credential cooldown.
                    self.reset_story_entry();
                    self.set_status_err("That didn't match. Try selecting it again.");
                }
            }
            LockMode::Story => {
                let Ok(secret) = story::encode(&self.story_selections) else {
                    self.reset_story_entry();
                    self.register_unlock_failure("Invalid Coffer Story.");
                    return;
                };
                if let Some(restore) = &self.restore {
                    let bytes = restore.bytes.clone();
                    self.unlocking = true;
                    self.set_status_info("Checking the backup…");
                    let (tx, rx) = std::sync::mpsc::channel();
                    self.unlock_rx = Some(rx);
                    std::thread::spawn(move || {
                        let outcome = io::open_backup(&bytes, &secret);
                        let _ = tx.send(BackgroundCryptoResult::RestoreDecrypt(outcome));
                    });
                    return;
                }
                let Some(raw) = self.raw_bytes.clone() else {
                    return;
                };
                self.unlocking = true;
                self.set_status_info("Unlocking…");
                let (tx, rx) = std::sync::mpsc::channel();
                self.unlock_rx = Some(rx);
                std::thread::spawn(move || {
                    let outcome = match io::decrypt_app_data(&raw, &secret) {
                        Ok((data, session)) => Ok((data, session)),
                        Err(io::DecryptAppDataError::Decrypt(_)) => {
                            Err("Wrong Coffer Story or data has been tampered with.".to_owned())
                        }
                        Err(
                            io::DecryptAppDataError::Parse(_) | io::DecryptAppDataError::Invalid,
                        ) => Err("Saved data is invalid after decryption.".to_owned()),
                    };
                    let _ = tx.send(BackgroundCryptoResult::Unlock(outcome));
                });
            }
            _ => {}
        }
    }

    pub(crate) fn select_story_object(&mut self, id: &'static str) {
        if self.unlocking
            || self.unlock_cooldown_remaining().is_some()
            || self.story_selections.len() >= story::STORY_LENGTH
            || self.story_selections.contains(&id)
        {
            return;
        }
        self.story_selections.push(id);
        if self.story_selections.len() == story::STORY_LENGTH {
            self.submit_story();
        }
    }

    /// Undoes the most recent pick. Submission fires automatically at the
    /// sixth pick, so this is only ever reachable with up to five selected.
    pub(crate) fn remove_last_story_selection(&mut self) {
        if self.unlocking || self.unlock_cooldown_remaining().is_some() {
            return;
        }
        self.story_selections.pop();
    }

    fn note_input_activity(&mut self, ctx: &egui::Context) {
        let has_events = ctx.input(|i| !i.events.is_empty());
        if has_events {
            self.touch_interaction();
        }
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

    /// Validates the entry form once, for both adding and correcting.
    ///
    /// Returns the signed amount alongside the raw magnitude: deductions are
    /// stored negative (see `Wallet::latest_deposit`), and the negative-balance
    /// warning talks in magnitudes.
    fn validated_entry_input(&mut self) -> Option<ValidatedEntryInput> {
        let amount = match parse_dollars_to_cents(&self.draft.amount) {
            Ok(amount) if amount > 0 => amount,
            _ => {
                self.set_status_err("Enter a valid amount, like 10, 10.50, or $1,234.56.");
                self.pending_entry_focus = Some(EntryFormField::Amount);
                return None;
            }
        };
        if !valid_cents(amount) {
            self.set_status_err("Enter a smaller amount.");
            self.pending_entry_focus = Some(EntryFormField::Amount);
            return None;
        }

        let description = self.draft.description.trim().to_owned();
        if !valid_description(&self.draft.description) {
            self.set_status_err("Add a description (1-100 characters).");
            self.pending_entry_focus = Some(EntryFormField::Description);
            return None;
        }

        let date = match parse_ledger_date(&self.draft.date_input) {
            Ok(date) => date,
            Err(err) => {
                self.set_status_err(err);
                self.pending_entry_focus = Some(EntryFormField::Date);
                return None;
            }
        };
        if date > Local::now().date_naive() {
            self.set_status_err("Use today or an earlier date.");
            self.pending_entry_focus = Some(EntryFormField::Date);
            return None;
        }

        let signed_amount = match self.draft.kind {
            EntryKind::Deposit => amount,
            EntryKind::Deduction => -amount,
        };

        Some(ValidatedEntryInput {
            amount,
            signed_amount,
            description,
            date,
        })
    }

    fn add_entry(&mut self) {
        if !self.can_change("Unlock parent mode before adding entries.") {
            return;
        }
        let snapshot = self.snapshot_mutation();
        self.prompts.dismiss_undo_and_delete_wallet();

        let Some(input) = self.validated_entry_input() else {
            return;
        };
        let ValidatedEntryInput {
            amount,
            signed_amount,
            description,
            date,
        } = input;

        let action = match self.draft.kind {
            EntryKind::Deposit => "Added",
            EntryKind::Deduction => "Deducted",
        };
        if self.draft.kind == EntryKind::Deduction {
            let next_balance = self
                .selected_wallet()
                .current_balance_cents()
                .saturating_sub(amount);
            if next_balance < 0 && !self.prompts.spend_anyway_is(amount) {
                self.prompts.arm_spend_anyway(amount);
                self.set_status_info(format!(
                    "This spending would leave {} at {}. Submit again to confirm.",
                    self.selected_wallet().child_name,
                    format_money(next_balance),
                ));
                return;
            }
        }
        self.prompts.dismiss_spend_anyway();

        let wallet_name = self.selected_wallet().child_name.clone();

        self.selected_wallet_mut().entries.push(Entry {
            date,
            description: description.clone(),
            amount_cents: signed_amount,
        });
        if !self.selected_wallet().balances_are_valid() {
            self.selected_wallet_mut().entries.pop();
            self.set_status_err(
                "That entry would put the wallet outside Cofferly's supported range.",
            );
            return;
        }

        let status = format!(
            "{action} {} for {}: {description}.",
            format_money(amount),
            wallet_name
        );

        // On failure the snapshot rolls the entry (and the old undo) back; keep
        // the draft so Enter retries it.
        if !self.persist_or_restore(status, snapshot, ". The entry was not recorded.") {
            self.pending_entry_focus = Some(EntryFormField::Description);
            return;
        }

        self.draft.description.clear();
        self.draft.amount.clear();
        self.draft.fill_today(Local::now().date_naive());
        // Same focus path as validation failures — lands the parent back in
        // Description for rapid multi-entry logging instead of wherever
        // focus happened to be.
        self.pending_entry_focus = Some(EntryFormField::Description);
        self.invalidate_ledger_cache();
    }

    /// Opens a correction for one entry: the add form is stashed and reloaded
    /// with that entry's values, so every validation, focus, and keyboard rule
    /// is the one the parent already knows.
    fn begin_entry_edit(&mut self, entry_index: usize) {
        if !self.can_change("Unlock parent mode before editing entries.") {
            return;
        }
        let Some(entry) = self.selected_wallet().entries.get(entry_index).cloned() else {
            self.set_status_err("That entry is no longer in the ledger.");
            return;
        };

        self.prompts.dismiss_delete_wallet();
        self.prompts.dismiss_spend_anyway();
        self.prompts.dismiss_remove_edited();

        // Mid-correction, `self.draft` holds the open correction's values, so
        // the real half-typed new entry is the one the session already stashed.
        let stashed_draft = match &self.entry_edit {
            Some(open) => open.stashed_draft.clone(),
            None => self.draft.clone(),
        };
        self.draft.kind = if entry.amount_cents < 0 {
            EntryKind::Deduction
        } else {
            EntryKind::Deposit
        };
        self.draft.amount = format_money_input(entry.amount_cents.abs());
        self.draft.description = entry.description.clone();
        self.draft.date_input = format_ledger_date(entry.date);
        self.draft.auto_date = None;

        self.entry_edit = Some(EntryEditSession {
            wallet_index: self.selected_wallet,
            entry_index,
            original: entry,
            stashed_draft,
        });
        self.pending_entry_focus = Some(EntryFormField::Amount);
        self.set_status_info(
            "Correcting an entry. Save the correction, or cancel to leave it as it was.",
        );
    }

    /// Leaves the entry exactly as it was and restores the half-typed new entry
    /// the parent had going before they started correcting.
    fn cancel_entry_edit(&mut self) {
        let Some(session) = self.entry_edit.take() else {
            return;
        };
        self.draft = session.stashed_draft;
        self.prompts.dismiss_spend_anyway();
        self.prompts.dismiss_remove_edited();
        self.pending_ledger_edit_focus = Some(session.entry_index);
        self.set_status_info("Correction cancelled.");
    }

    /// Ends a correction whose entry no longer sits where it was opened,
    /// writing nothing, and puts the half-typed new entry back.
    fn end_stale_entry_edit(&mut self, stashed_draft: EntryDraft) {
        self.entry_edit = None;
        self.prompts.dismiss_spend_anyway();
        self.prompts.dismiss_remove_edited();
        self.draft = stashed_draft;
        self.set_status_err("That entry changed while you were correcting it.");
    }

    /// Removes the entry being corrected, wherever it sits in the ledger. The
    /// first call arms a confirmation; the second removes. A failed vault write
    /// restores the ledger, the correction session, the form and the old undo.
    fn remove_edited_entry(&mut self) {
        if !self.can_change("Unlock parent mode before removing entries.") {
            return;
        }
        let Some(session) = self.entry_edit.clone() else {
            return;
        };
        if session.wallet_index != self.selected_wallet {
            self.cancel_entry_edit();
            return;
        }
        let Some(entry) = self
            .selected_wallet()
            .entries
            .get(session.entry_index)
            .cloned()
        else {
            self.entry_edit = None;
            self.prompts.dismiss_remove_edited();
            self.draft = session.stashed_draft;
            self.set_status_err("That entry is no longer in the ledger.");
            return;
        };
        if entry != session.original {
            self.end_stale_entry_edit(session.stashed_draft);
            return;
        }
        let wallet_name = self.selected_wallet().child_name.clone();

        if !self.prompts.remove_edited_is(session.entry_index) {
            self.prompts.arm_remove_edited(session.entry_index);
            let mut text = format!(
                "Remove {} · {} · {}? Click confirm to remove it.",
                format_ledger_date(entry.date),
                entry.description,
                format_money(entry.amount_cents),
            );
            if self
                .selected_wallet()
                .goes_negative_without(session.entry_index)
            {
                let _ = write!(
                    text,
                    " Removing it would leave {wallet_name} below zero later in the ledger."
                );
            }
            self.set_status_info(text);
            return;
        }
        self.prompts.dismiss_remove_edited();

        let snapshot = self.snapshot_mutation();
        let edit_snapshot = (self.entry_edit.clone(), self.draft.clone());
        self.prompts.dismiss_delete_wallet();
        self.prompts.dismiss_spend_anyway();

        self.selected_wallet_mut()
            .entries
            .remove(session.entry_index);
        self.prompts.arm_undo(PendingUndo::Removed(RemovableEntry {
            wallet_index: session.wallet_index,
            entry: entry.clone(),
            entry_index: Some(session.entry_index),
        }));
        self.entry_edit = None;
        self.draft = session.stashed_draft;
        self.invalidate_ledger_cache();
        self.pending_ledger_edit_focus = None;
        self.pending_entry_focus = Some(EntryFormField::Description);
        let saved = self.persist_or_restore(
            format!(
                "Removed entry from {}: {} {}. Undo available.",
                wallet_name,
                format_money(entry.amount_cents),
                entry.description
            ),
            snapshot,
            "",
        );
        if !saved {
            // Save failed: snapshot restored ledger and undo; put the form back.
            self.entry_edit = edit_snapshot.0;
            self.draft = edit_snapshot.1;
        }
    }

    /// Would this candidate entry drive the running balance negative *at its own
    /// position* (or anywhere after it)? A final-balance check misses exactly the
    /// case this is for: a correction in the middle of the ledger.
    fn edit_would_go_negative(
        &self,
        entry_index: usize,
        date: NaiveDate,
        signed_amount: i64,
    ) -> bool {
        self.selected_wallet()
            .goes_negative_with(entry_index, date, signed_amount)
    }

    /// Applies the correction. A failed vault write puts the previous entry back
    /// rather than leaving a "saved" ledger that is not on disk.
    fn commit_entry_edit(&mut self) {
        if !self.can_change("Unlock parent mode before editing entries.") {
            return;
        }
        let Some(session) = self.entry_edit.clone() else {
            return;
        };
        if session.wallet_index != self.selected_wallet {
            self.cancel_entry_edit();
            return;
        }
        match self.selected_wallet().entries.get(session.entry_index) {
            None => {
                self.entry_edit = None;
                self.draft = session.stashed_draft;
                self.set_status_err("That entry is no longer in the ledger.");
                return;
            }
            Some(current) if *current != session.original => {
                self.end_stale_entry_edit(session.stashed_draft);
                return;
            }
            Some(_) => {}
        }

        let Some(input) = self.validated_entry_input() else {
            return;
        };

        if input.signed_amount < 0
            && self.edit_would_go_negative(session.entry_index, input.date, input.signed_amount)
            && !self.prompts.spend_anyway_is(input.amount)
        {
            self.prompts.arm_spend_anyway(input.amount);
            self.set_status_info(format!(
                "This correction would leave {} below zero at that point in the ledger. Submit again to confirm.",
                self.selected_wallet().child_name,
            ));
            return;
        }
        self.prompts.dismiss_spend_anyway();

        let snapshot = self.snapshot_mutation();
        let previous = session.original.clone();
        let wallet_name = self.selected_wallet().child_name.clone();
        let updated = Entry {
            date: input.date,
            description: input.description.clone(),
            amount_cents: input.signed_amount,
        };

        if updated == previous {
            self.entry_edit = None;
            self.draft = session.stashed_draft;
            self.pending_ledger_edit_focus = Some(session.entry_index);
            self.set_status_info("Nothing changed — the entry is as it was.");
            return;
        }

        self.selected_wallet_mut().entries[session.entry_index] = updated;
        if !self.selected_wallet().balances_are_valid() {
            self.selected_wallet_mut().entries[session.entry_index] = previous;
            self.set_status_err(
                "That correction would put the wallet outside Cofferly's supported range.",
            );
            self.pending_entry_focus = Some(EntryFormField::Amount);
            return;
        }

        self.invalidate_ledger_cache();

        // The undo is set before the save so a failed write restores the old one
        // from the snapshot; the correction session stays open for a retry.
        self.prompts.arm_undo(PendingUndo::Edited(EditedEntry {
            wallet_index: session.wallet_index,
            entry_index: session.entry_index,
            previous,
        }));
        let saved = self.persist_or_restore(
            format!(
                "Corrected entry for {wallet_name}: {} {}. Undo available.",
                format_money(input.signed_amount),
                input.description
            ),
            snapshot,
            "",
        );
        if !saved {
            return;
        }
        self.entry_edit = None;
        self.draft = session.stashed_draft;
        self.pending_ledger_edit_focus = Some(session.entry_index);
    }

    /// Wallet index and amount the pending undo would restore, for the button label.
    fn pending_undo_summary(&self) -> Option<(usize, i64)> {
        match self.prompts.undo()? {
            PendingUndo::Removed(removable) => {
                Some((removable.wallet_index, removable.entry.amount_cents))
            }
            PendingUndo::Edited(edited) => {
                Some((edited.wallet_index, edited.previous.amount_cents))
            }
        }
    }

    fn cancel_negative_spend_confirm(&mut self) {
        if self.prompts.take_spend_anyway() {
            self.set_status_info("Spending not recorded.");
        }
    }

    /// Moves an auto-filled, untouched entry date forward when the local day
    /// has changed (app left open overnight). A typed date or one loaded for
    /// a correction is never overwritten.
    fn refresh_draft_date_on(&mut self, today: NaiveDate) {
        let Some(filled) = self.draft.auto_date else {
            return;
        };
        if filled != today && self.draft.date_input == format_ledger_date(filled) {
            self.draft.fill_today(today);
        }
    }

    fn fill_entry_date_today(&mut self) {
        self.prompts.dismiss_remove_edited();
        self.draft.fill_today(Local::now().date_naive());
        self.pending_entry_focus = Some(EntryFormField::Date);
    }

    /// Prefills the draft from the selected wallet's newest deposit (amount +
    /// description), with today's date, so a parent can log a repeat deposit
    /// (e.g. weekly allowance) without retyping it. Does not submit -- the
    /// parent still confirms via the normal Add button and its validation.
    fn repeat_last_deposit(&mut self) {
        let Some((amount_cents, description)) = self
            .selected_wallet()
            .latest_deposit()
            .map(|deposit| (deposit.amount_cents, deposit.description.clone()))
        else {
            return;
        };
        self.draft.kind = EntryKind::Deposit;
        self.draft.amount = format_money_input(amount_cents);
        self.draft.description = description;
        self.draft.fill_today(Local::now().date_naive());
        self.prompts.dismiss_spend_anyway();
        self.pending_entry_focus = Some(EntryFormField::Amount);
    }

    fn prefill_settings_from_selected(&mut self) {
        let wallet = self.selected_wallet();
        let name = wallet.child_name.clone();
        let starting = wallet.starting_balance_cents;
        let allowance = wallet
            .weekly_allowance
            .map(|allowance| allowance.amount_cents);
        let goal = wallet.savings_goal_cents;
        self.child_name_input = name;
        self.starting_balance_input = format_money_input(starting);
        self.weekly_allowance_input = allowance.map(format_money_input).unwrap_or_default();
        self.savings_goal_input = goal.map(format_money_input).unwrap_or_default();
    }

    fn open_settings(&mut self) {
        self.prefill_settings_from_selected();
        self.new_child_name_input.clear();
        self.prompts.dismiss_for_settings_open();
        self.show_settings = true;
    }

    fn starting_balance_save_ready(&self) -> bool {
        match parse_dollars_to_cents(&self.starting_balance_input) {
            Ok(cents) => {
                valid_cents(cents) && cents != self.selected_wallet().starting_balance_cents
            }
            Err(_) => false,
        }
    }

    fn update_starting_balance(&mut self) {
        if !self.can_change("Unlock parent mode before changing balances.") {
            return;
        }
        let snapshot = self.snapshot_mutation();
        self.prompts.dismiss_undo_and_delete_wallet();

        let Ok(balance) = parse_dollars_to_cents(&self.starting_balance_input) else {
            self.set_status_err("Enter a valid starting balance, like 90, 90.00, or $1,234.56.");
            return;
        };
        if !valid_cents(balance) {
            self.set_status_err("Enter a smaller starting balance.");
            return;
        }
        if balance == self.selected_wallet().starting_balance_cents {
            return;
        }

        let wallet = self.selected_wallet_mut();
        let previous_balance = wallet.starting_balance_cents;
        wallet.starting_balance_cents = balance;
        if !wallet.balances_are_valid() {
            wallet.starting_balance_cents = previous_balance;
            self.set_status_err(
                "That starting balance would put the wallet outside Cofferly's supported range.",
            );
            return;
        }

        let wallet_name = self.selected_wallet().child_name.clone();
        self.starting_balance_input.clear();
        self.invalidate_ledger_cache();
        self.save_with_success_or_restore(
            format!(
                "Updated {} starting balance to {}.",
                wallet_name,
                format_money(balance)
            ),
            snapshot,
        );
    }

    fn weekly_allowance_save_ready(&self) -> bool {
        let current = self
            .selected_wallet()
            .weekly_allowance
            .map(|allowance| allowance.amount_cents);
        let input = self.weekly_allowance_input.trim();
        if input.is_empty() {
            return current.is_some();
        }
        match parse_dollars_to_cents(input) {
            Ok(cents) => cents > 0 && valid_cents(cents) && Some(cents) != current,
            Err(_) => false,
        }
    }

    fn save_weekly_allowance(&mut self) {
        self.save_weekly_allowance_on(Local::now().date_naive());
    }

    /// One amount field (#179): blank turns the allowance off; a new amount
    /// turns it on with today's weekday; changing the amount keeps the weekday
    /// and `last_posted` (only future weeks use the new amount).
    fn save_weekly_allowance_on(&mut self, today: NaiveDate) {
        if !self.can_change("Unlock parent mode before changing the weekly allowance.") {
            return;
        }
        let snapshot = self.snapshot_mutation();
        self.prompts.dismiss_undo_and_delete_wallet();

        let wallet_name = self.selected_wallet().child_name.clone();
        let current = self.selected_wallet().weekly_allowance;
        let input = self.weekly_allowance_input.trim().to_owned();

        let status = if input.is_empty() {
            if current.is_none() {
                return;
            }
            self.selected_wallet_mut().weekly_allowance = None;
            format!("Weekly allowance turned off for {wallet_name}.")
        } else {
            let Ok(cents) = parse_dollars_to_cents(&input) else {
                self.set_status_err(
                    "Enter a weekly allowance like 5 or 5.00, or leave it blank to turn it off.",
                );
                return;
            };
            if cents <= 0 {
                self.set_status_err("Enter a weekly allowance above $0.00, or leave it blank.");
                return;
            }
            if !valid_cents(cents) {
                self.set_status_err("Enter a smaller weekly allowance.");
                return;
            }
            match current {
                Some(allowance) if allowance.amount_cents == cents => return,
                Some(mut allowance) => {
                    allowance.amount_cents = cents;
                    self.selected_wallet_mut().weekly_allowance = Some(allowance);
                    format!(
                        "Weekly allowance for {wallet_name} is now {} every {}.",
                        format_money(cents),
                        allowance.weekday_name()
                    )
                }
                None => {
                    let allowance = WeeklyAllowance::starting(cents, today);
                    self.selected_wallet_mut().weekly_allowance = Some(allowance);
                    format!(
                        "Weekly allowance of {} turned on for {wallet_name}. It posts every {}, starting {}.",
                        format_money(cents),
                        allowance.weekday_name(),
                        format_ledger_date(today + chrono::Duration::weeks(1))
                    )
                }
            }
        };

        self.prefill_settings_from_selected();
        self.save_with_success_or_restore(status, snapshot);
    }

    fn savings_goal_save_ready(&self) -> bool {
        let current = self.selected_wallet().savings_goal_cents;
        let input = self.savings_goal_input.trim();
        if input.is_empty() {
            return current.is_some();
        }
        match parse_dollars_to_cents(input) {
            Ok(cents) => cents > 0 && valid_cents(cents) && Some(cents) != current,
            Err(_) => false,
        }
    }

    /// One amount field (#181). Blank clears the goal. Zero, negative, and
    /// amounts outside `MAX_ABSOLUTE_CENTS` are rejected, same as other money.
    fn save_savings_goal(&mut self) {
        if !self.can_change("Unlock parent mode before changing the savings goal.") {
            return;
        }
        let snapshot = self.snapshot_mutation();
        self.prompts.dismiss_undo_and_delete_wallet();

        let wallet_name = self.selected_wallet().child_name.clone();
        let current = self.selected_wallet().savings_goal_cents;
        let input = self.savings_goal_input.trim().to_owned();

        let status = if input.is_empty() {
            if current.is_none() {
                return;
            }
            self.selected_wallet_mut().savings_goal_cents = None;
            format!("Savings goal cleared for {wallet_name}.")
        } else {
            let Ok(cents) = parse_dollars_to_cents(&input) else {
                self.set_status_err(
                    "Enter a savings goal like 120 or 120.00, or leave it blank to clear it.",
                );
                return;
            };
            if cents <= 0 {
                self.set_status_err("Enter a savings goal above $0.00, or leave it blank.");
                return;
            }
            if !valid_cents(cents) {
                self.set_status_err("Enter a smaller savings goal.");
                return;
            }
            if Some(cents) == current {
                return;
            }
            self.selected_wallet_mut().savings_goal_cents = Some(cents);
            format!("Savings goal for {wallet_name} is {}.", format_money(cents))
        };

        self.prefill_settings_from_selected();
        self.save_with_success_or_restore(status, snapshot);
    }

    fn rename_selected_child(&mut self) {
        if !self.can_change("Unlock parent mode before renaming wallets.") {
            return;
        }
        let snapshot = self.snapshot_mutation();
        self.prompts.dismiss_undo_and_delete_wallet();

        let name = self.child_name_input.trim().to_owned();
        if !valid_child_name(&name) {
            self.set_status_err("Use a child name between 1 and 40 characters.");
            return;
        }
        if child_name_taken(&self.data.wallets, &name, Some(self.selected_wallet)) {
            self.set_status_err(format!(
                "Another wallet is already named {name}. Choose a different name."
            ));
            return;
        }

        self.remember_selected_ledger_filter();
        let previous_child_name = std::mem::take(&mut self.selected_wallet_mut().child_name);
        self.selected_wallet_mut().child_name = name;
        if let Some(query) = self.ledger_filters.remove(&previous_child_name) {
            self.ledger_filters
                .insert(self.selected_wallet().child_name.clone(), query);
        }
        // Keep name + opening filled (same helper as add/delete) so Save name
        // stays off: the field matches the selected wallet again.
        self.prefill_settings_from_selected();
        self.invalidate_ledger_cache();
        self.save_with_success_or_restore(
            format!(
                "Renamed {previous_child_name} to {}.",
                self.selected_wallet().child_name
            ),
            snapshot,
        );
    }

    fn add_child_wallet(&mut self) {
        if !self.can_change("Unlock parent mode before adding wallets.") {
            return;
        }
        let snapshot = self.snapshot_mutation();
        self.prompts.dismiss_undo_and_delete_wallet();

        let name = self.new_child_name_input.trim().to_owned();
        if !valid_child_name(&name) {
            self.set_status_err("Use a child name between 1 and 40 characters.");
            return;
        }
        if child_name_taken(&self.data.wallets, &name, None) {
            self.set_status_err(format!(
                "Another wallet is already named {name}. Choose a different name."
            ));
            return;
        }

        self.data.wallets.push(Wallet {
            child_name: name.clone(),
            starting_balance_cents: 0,
            entries: Vec::new(),
            weekly_allowance: None,
            savings_goal_cents: None,
        });
        self.select_wallet(self.data.wallets.len() - 1);
        self.new_child_name_input.clear();
        // New wallets open at $0. Drop the previous kid's starting-balance
        // prefill so Save stays disabled until this wallet's opening is edited.
        self.prefill_settings_from_selected();
        self.invalidate_ledger_cache();
        self.save_with_success_or_restore(format!("Added wallet for {name}."), snapshot);
    }

    fn newest_entry_index(entries: &[Entry]) -> Option<usize> {
        entries
            .iter()
            .enumerate()
            .max_by(|(left_index, left), (right_index, right)| {
                left.date
                    .cmp(&right.date)
                    .then_with(|| left_index.cmp(right_index))
            })
            .map(|(index, _)| index)
    }

    /// What "Remove latest entry" would take out, as the ledger shows it.
    /// Uses the same pick as the removal (newest by date, then list order).
    fn latest_entry_preview(&self) -> Option<String> {
        let entries = &self.selected_wallet().entries;
        let entry = &entries[Self::newest_entry_index(entries)?];
        Some(format!(
            "{} · {} · {}",
            format_ledger_date(entry.date),
            entry.description,
            format_money(entry.amount_cents)
        ))
    }

    fn remove_latest_entry(&mut self) {
        if !self.can_change("Unlock parent mode before removing entries.") {
            return;
        }
        // Positions may shift below, so an open correction can't stay open.
        self.cancel_entry_edit();
        let snapshot = self.snapshot_mutation();
        self.prompts.dismiss_delete_wallet();

        let wallet_name = self.selected_wallet().child_name.clone();
        let index = Self::newest_entry_index(&self.selected_wallet().entries);
        if let Some(index) = index {
            let entry = self.selected_wallet_mut().entries.remove(index);
            self.prompts.arm_undo(PendingUndo::Removed(RemovableEntry {
                wallet_index: self.selected_wallet,
                entry: entry.clone(),
                entry_index: None,
            }));
            self.invalidate_ledger_cache();
            self.save_with_success_or_restore(
                format!(
                    "Removed latest entry from {}: {} {}. Undo available.",
                    wallet_name,
                    format_money(entry.amount_cents),
                    entry.description
                ),
                snapshot,
            );
        } else {
            self.set_status_info("There are no entries to remove.");
        }
    }

    /// Undoes the last ledger change — a removal or a correction. One slot, so
    /// this always means "put back what I just changed".
    fn undo_remove_entry(&mut self) {
        if !self.can_change("Unlock parent mode before undoing.") {
            return;
        }
        // Positions may shift below, so an open correction can't stay open.
        self.cancel_entry_edit();
        let snapshot = self.snapshot_mutation();
        self.prompts.dismiss_delete_wallet();

        let Some(pending) = self.prompts.take_undo() else {
            return;
        };

        match pending {
            PendingUndo::Removed(removable) => {
                let Some(wallet) = self.data.wallets.get_mut(removable.wallet_index) else {
                    self.set_status_err("Can't undo — that wallet no longer exists.");
                    return;
                };

                match removable.entry_index {
                    Some(at) => {
                        let at = at.min(wallet.entries.len());
                        wallet.entries.insert(at, removable.entry.clone());
                    }
                    None => wallet.entries.push(removable.entry.clone()),
                }
                let wallet_name = wallet.child_name.clone();
                self.invalidate_ledger_cache();
                self.save_with_success_or_restore(
                    format!(
                        "Restored entry for {}: {} {}.",
                        wallet_name,
                        format_money(removable.entry.amount_cents),
                        removable.entry.description
                    ),
                    snapshot,
                );
            }
            PendingUndo::Edited(edited) => {
                let Some(wallet) = self.data.wallets.get_mut(edited.wallet_index) else {
                    self.set_status_err("Can't undo — that wallet no longer exists.");
                    return;
                };
                let Some(slot) = wallet.entries.get_mut(edited.entry_index) else {
                    self.set_status_err("Can't undo — that entry is no longer in the ledger.");
                    return;
                };

                *slot = edited.previous.clone();
                let wallet_name = wallet.child_name.clone();
                self.invalidate_ledger_cache();
                self.save_with_success_or_restore(
                    format!(
                        "Reverted correction for {}: {} {}.",
                        wallet_name,
                        format_money(edited.previous.amount_cents),
                        edited.previous.description
                    ),
                    snapshot,
                );
            }
        }
    }

    fn delete_selected_wallet(&mut self) {
        if !self.can_change("Unlock parent mode before deleting wallets.") {
            return;
        }

        if self.data.wallets.len() <= 1 {
            self.set_status_err("Keep at least one wallet.");
            return;
        }

        let snapshot = self.snapshot_mutation();
        let wallet_name = self.selected_wallet().child_name.clone();
        let removed_index = self.selected_wallet;
        self.data.wallets.remove(removed_index);
        self.ledger_filters.remove(&wallet_name);
        self.prompts.dismiss_undo_and_delete_wallet();
        if self.selected_wallet >= self.data.wallets.len() {
            self.selected_wallet = self.data.wallets.len() - 1;
        }
        self.ledger_filter = self.ledger_filter_for_selected();
        // Settings stays open after delete. Drop the removed wallet's
        // name/opening prefills so Save cannot overwrite the remaining kid.
        self.prefill_settings_from_selected();
        self.invalidate_ledger_cache();
        self.save_with_success_or_restore(format!("Deleted wallet for {wallet_name}."), snapshot);
    }

    /// Settings → "Back up vault…": native save dialog, then a verified copy.
    pub(crate) fn back_up_vault(&mut self) {
        if !self.can_change("Unlock parent mode to back up the vault.") {
            return;
        }
        let file_name = io::backup_file_name(Local::now().date_naive());
        let Some(dest) = rfd::FileDialog::new()
            .set_title("Back up Cofferly vault")
            .set_file_name(&file_name)
            .add_filter("Cofferly vault", &["cofferly"])
            .save_file()
        else {
            self.set_status_info("Backup cancelled.");
            return;
        };
        self.back_up_vault_to(dest, false);
    }

    /// Copies the on-disk encrypted vault to `dest` byte-for-byte and verifies
    /// it. An existing `dest` is only replaced after the parent confirms.
    pub(crate) fn back_up_vault_to(&mut self, dest: PathBuf, replace_existing: bool) {
        self.prompts.dismiss_backup_overwrite();
        if !self.can_change("Unlock parent mode to back up the vault.") {
            return;
        }
        let vault = match io::load_raw(&self.data_path) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                self.set_status_err(
                    "Nothing to back up yet. Make a change first so the vault is saved.",
                );
                return;
            }
            Err(err) => {
                self.set_status_err(format!("Could not read the vault to back it up: {err}"));
                return;
            }
        };
        if dest.exists() && !replace_existing {
            let name = dest
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| dest.display().to_string());
            self.prompts.arm_backup_overwrite(dest);
            self.set_status_info(format!("{name} already exists. Replace it?"));
            return;
        }
        match io::write_backup(&dest, &vault, replace_existing) {
            Ok(()) => {
                self.last_backup = Some(format_ledger_date(Local::now().date_naive()));
                self.set_status_ok(format!(
                    "Backed up the encrypted vault to {} and verified the copy.",
                    dest.display()
                ));
            }
            Err(err) => self.set_status_err(format!("Backup failed: {err}")),
        }
    }

    pub(crate) fn cancel_backup_overwrite(&mut self) {
        self.prompts.dismiss_backup_overwrite();
        self.set_status_info("Backup cancelled. The existing file was not changed.");
    }

    /// Restore is offered in Settings (parent unlocked) and on a fresh
    /// install's lock screen, never over a locked vault nobody has opened.
    pub(crate) fn can_start_restore(&self) -> bool {
        !self.unlocking
            && self.restore.is_none()
            && (self.parent_unlocked
                || (self.raw_bytes.is_none()
                    && matches!(
                        self.lock_mode,
                        LockMode::SetupReveal | LockMode::SetupConfirm
                    )))
    }

    /// "Restore from backup…": native open dialog, then the backup's Coffer Story.
    pub(crate) fn restore_from_backup(&mut self) {
        if !self.can_start_restore() {
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_title("Restore Cofferly backup")
            .add_filter("Cofferly vault", &["cofferly"])
            .pick_file()
        else {
            self.set_status_info("Restore cancelled. Nothing on this PC was changed.");
            return;
        };
        self.begin_restore_from(&path);
    }

    /// Validates the chosen file and switches the lock screen to asking for
    /// that backup's Coffer Story. Nothing on disk changes here.
    pub(crate) fn begin_restore_from(&mut self, path: &Path) {
        if !self.can_start_restore() {
            return;
        }
        let bytes = match io::read_backup(path) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.set_status_err(format!("Restore failed: {err}."));
                return;
            }
        };
        let replaces_wallets = if self.parent_unlocked {
            self.data.wallets.len()
        } else {
            0
        };
        if self.parent_unlocked {
            self.lock_parent();
        }
        let return_mode = self.lock_mode;
        self.restore = Some(PendingRestore {
            file_name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            bytes,
            replaces_wallets,
            return_mode,
            decrypted: None,
        });
        self.lock_mode = LockMode::Story;
        self.reset_story_entry();
        self.set_status_info("Choose the Coffer Story for this backup.");
    }

    fn apply_restore_decrypt(&mut self, outcome: Result<(AppData, SessionCrypto), String>) {
        self.reset_story_entry();
        let Some(restore) = self.restore.as_mut() else {
            return;
        };
        match outcome {
            Ok(decrypted) => {
                let count = decrypted.0.wallets.len();
                restore.decrypted = Some(decrypted);
                self.reset_pin_failures();
                self.set_status_info(format!(
                    "Backup unlocked: {} in this backup. Check them, then confirm the restore.",
                    wallet_count_label(count)
                ));
            }
            Err(err) => self.register_unlock_failure(&err),
        }
    }

    pub(crate) fn cancel_restore(&mut self) {
        if self.unlocking {
            return;
        }
        if let Some(restore) = self.restore.take() {
            self.lock_mode = restore.return_mode;
        }
        self.reset_story_entry();
        self.set_status_info("Restore cancelled. Nothing on this PC was changed.");
    }

    /// Replaces the vault with the decrypted backup (keeping a pre-restore
    /// copy) and opens it. Any failure leaves the current vault untouched.
    pub(crate) fn confirm_restore(&mut self) {
        let Some(restore) = self.restore.take() else {
            return;
        };
        let Some((data, session)) = restore.decrypted else {
            self.restore = Some(PendingRestore {
                decrypted: None,
                ..restore
            });
            return;
        };
        let stamp = Local::now().format("%Y%m%d-%H%M%S").to_string();
        match io::restore_vault(&self.data_path, &restore.bytes, &stamp) {
            Ok(kept) => {
                let count = data.wallets.len();
                self.raw_bytes = Some(restore.bytes);
                self.save_enabled = true;
                self.lock_mode = LockMode::Story;
                let allowance = self.apply_unlock(data, session);
                let kept = kept
                    .and_then(|path| {
                        path.file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                    })
                    .map(|name| format!(" The previous vault was kept as {name}."))
                    .unwrap_or_default();
                let restored = format!(
                    "Restored {} from {}.{kept}",
                    wallet_count_label(count),
                    restore.file_name
                );
                match allowance {
                    Some(status) => {
                        self.status = Status {
                            text: format!("{restored} {}", status.text),
                            severity: status.severity,
                        }
                    }
                    None => self.set_status_ok(restored),
                }
            }
            Err(err) => {
                self.lock_mode = restore.return_mode;
                self.reset_story_entry();
                self.set_status_err(format!(
                    "Restore failed: {err}. The vault on this PC was not changed."
                ));
            }
        }
    }

    /// Snapshot of everything a settings/wallet/ledger mutation can touch
    /// before it saves, so a failed vault write can put memory back to what
    /// is on disk.
    fn snapshot_mutation(&self) -> MutationSnapshot {
        MutationSnapshot {
            data: self.data.clone(),
            selected_wallet: self.selected_wallet,
            ledger_filter: self.ledger_filter.clone(),
            ledger_filters: self.ledger_filters.clone(),
            undo: self.prompts.undo().cloned(),
            new_child_name_input: self.new_child_name_input.clone(),
        }
    }

    /// Saves the vault; on success reports `success_status`. On every failure
    /// (changes disabled or a failed write) restores the pre-mutation snapshot
    /// so memory equals disk, then reports the error followed by `error_suffix`.
    /// Returns whether the save succeeded.
    fn persist_or_restore(
        &mut self,
        success_status: impl Into<String>,
        snapshot: MutationSnapshot,
        error_suffix: &str,
    ) -> bool {
        let failure = if self.save_enabled {
            let secret = save_secret_for(&self.session, &self.data);
            match self.save_encrypted_data_and_refresh_ref(&secret) {
                Ok(()) => {
                    self.set_status_ok(success_status);
                    return true;
                }
                Err(err) => format!("Could not save: {err}{error_suffix}"),
            }
        } else {
            "Saved data could not be loaded, so changes are disabled.".to_owned()
        };
        self.data = snapshot.data;
        self.selected_wallet = snapshot.selected_wallet;
        self.ledger_filter = snapshot.ledger_filter;
        self.ledger_filters = snapshot.ledger_filters;
        self.prompts.restore_undo(snapshot.undo);
        self.new_child_name_input = snapshot.new_child_name_input;
        // Re-derive the settings fields from the restored wallet so
        // Save buttons stay off and the fields match the ledger again.
        self.prefill_settings_from_selected();
        self.invalidate_ledger_cache();
        self.set_status_err(failure);
        false
    }

    fn save_with_success_or_restore(
        &mut self,
        success_status: impl Into<String>,
        snapshot: MutationSnapshot,
    ) {
        self.persist_or_restore(success_status, snapshot, "");
    }

    fn save_encrypted_data_and_refresh_ref(&mut self, pin: &str) -> Result<(), String> {
        let encrypted = save_encrypted(&self.data_path, &self.data, pin, &mut self.session)?;
        self.raw_bytes = Some(encrypted);
        Ok(())
    }

    fn can_change(&mut self, locked_status: &str) -> bool {
        if !self.save_enabled {
            self.set_status_err("Saved data could not be loaded, so changes are disabled.");
            return false;
        }

        if !self.parent_unlocked {
            self.set_status_err(locked_status);
            return false;
        }

        true
    }
}

impl eframe::App for CofferlyApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.remember_selected_ledger_filter();
        let state = UiState {
            selected_wallet: self.selected_wallet,
            ledger_sort_newest_first: matches!(self.ledger_sort, LedgerSort::NewestFirst),
            selected_wallet_name: self
                .data
                .wallets
                .get(self.selected_wallet)
                .map(|wallet| wallet.child_name.clone()),
            last_entry_kind: self.draft.kind,
            ledger_filter: self.ledger_filter.clone(),
            ledger_filters: self.ledger_filters.clone(),
            last_backup: self.last_backup.clone(),
        };
        eframe::set_value(storage, UI_STATE_KEY, &state);
    }

    fn on_exit(&mut self) {
        self.cleanup_temp_artifacts();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Capture before auto-lock so demo frames are not interrupted.
        if let Some(mut session) = self.capture.take() {
            session.tick(self, &ctx);
            self.capture = Some(session);
        }
        self.note_input_activity(&ctx);
        self.poll_unlock(&ctx);
        self.refresh_draft_date_on(Local::now().date_naive());
        if self.capture.is_none() {
            self.auto_lock_if_idle(&ctx);
        }

        if !self.parent_unlocked {
            self.lock_screen(ui);
            return;
        }

        self.handle_wallet_keyboard_nav(&ctx);
        self.handle_ledger_filter_shortcut(&ctx);

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
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("Parent mode unlocked")
                                        .size(11.0)
                                        .strong()
                                        .color(theme::ACCENT_DARK),
                                );
                                if let Some(remaining) = self.auto_lock_remaining() {
                                    if remaining <= AUTO_LOCK_WARN && !remaining.is_zero() {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "· Locks in {}",
                                                format_cooldown(remaining)
                                            ))
                                            .size(11.0)
                                            .color(theme::TEXT_SECONDARY),
                                        );
                                    }
                                }
                            });
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
                            self.open_settings();
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
            .min_size(WALLET_PICKER_WIDTH)
            .max_size(WALLET_PICKER_WIDTH)
            .frame(
                egui::Frame::new()
                    .fill(theme::FAINT_BG)
                    .inner_margin(egui::Margin::same(11))
                    .stroke(egui::Stroke::new(1.0, theme::BORDER)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        let panel_content_width = ui.available_width();
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
                        ui.add_space(5.0);

                        for index in 0..self.data.wallets.len() {
                            let selected = self.selected_wallet == index;
                            let child_name = self.data.wallets[index].child_name.clone();
                            let balance = self.data.wallets[index].current_balance_cents();
                            let accessible_label =
                                wallet_selection_announcement(&child_name, balance);

                            let idle_chrome = wallet_card_chrome(selected, false);
                            let response = ui.add_sized(
                                [panel_content_width, WALLET_CARD_HEIGHT],
                                egui::Button::selectable(selected, "")
                                    .fill(idle_chrome.fill)
                                    .stroke(idle_chrome.stroke),
                            );
                            let chrome = wallet_card_chrome(selected, response.has_focus());
                            if let Some(ring) = chrome.focus_ring {
                                ui.painter_at(response.rect).rect_stroke(
                                    response.rect,
                                    egui::CornerRadius::same(8),
                                    ring,
                                    egui::StrokeKind::Inside,
                                );
                            }

                            response.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::Role::Button,
                                    true,
                                    selected,
                                    accessible_label.clone(),
                                )
                            });

                            if response.clicked() {
                                self.select_wallet(index);
                            }

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
                                balance_color(balance)
                            };

                            painter.text(
                                rect.left_top() + egui::vec2(12.0, 9.0),
                                egui::Align2::LEFT_TOP,
                                &child_name,
                                egui::FontId::proportional(15.0),
                                text_color,
                            );

                            painter.text(
                                rect.left_bottom() + egui::vec2(12.0, -9.0),
                                egui::Align2::LEFT_BOTTOM,
                                format_money(balance),
                                egui::FontId::proportional(13.0),
                                balance_color,
                            );
                        }

                        ui.add_space(4.0);

                        if ui
                            .add_sized(
                                [panel_content_width, 29.0],
                                egui::Button::new("Print this wallet"),
                            )
                            .clicked()
                        {
                            self.print_selected_wallet();
                        }
                        if ui
                            .add_sized(
                                [panel_content_width, 29.0],
                                egui::Button::new("Print all wallets"),
                            )
                            .clicked()
                        {
                            self.print_all_wallets();
                        }
                        if ui
                            .add_sized(
                                [panel_content_width, 29.0],
                                egui::Button::new("Export this wallet CSV"),
                            )
                            .clicked()
                        {
                            self.export_selected_wallet_csv();
                        }
                        if ui
                            .add_sized(
                                [panel_content_width, 29.0],
                                egui::Button::new("Export all wallets CSV"),
                            )
                            .clicked()
                        {
                            self.export_all_wallets_csv();
                        }

                        ui.add_space(7.0);
                        self.entry_form(ui);

                        ui.add_space(6.0);
                        self.status_area(ui);
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

impl CofferlyApp {
    fn status_area(&self, ui: &mut egui::Ui) {
        let (fill, text_color, prefix) = match self.status.severity {
            StatusSeverity::Info => (theme::GOLD_LIGHT, theme::TEXT_PRIMARY, ""),
            StatusSeverity::Success => (theme::SUCCESS_LIGHT, theme::ACCENT_DARK, ""),
            StatusSeverity::Error => (theme::ERROR_LIGHT, theme::NEGATIVE, "⚠ "),
        };

        let display = format!("{prefix}{}", self.status.text);

        egui::Frame::new()
            .fill(fill)
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_max_width(300.0);
                show_live_status(
                    ui,
                    "parent_mode_status",
                    &display,
                    text_color,
                    11.0,
                    true,
                    self.status.severity,
                );
            });
    }
}

/// Status chip/label. `id_salt` is a fixed surface id, not the message, so a
/// repeated or replaced string updates the same AccessKit node. With AccessKit
/// off the builder returns `None` and the label is unchanged.
pub(crate) fn show_live_status(
    ui: &mut egui::Ui,
    id_salt: &'static str,
    text: &str,
    color: egui::Color32,
    size: f32,
    strong: bool,
    severity: StatusSeverity,
) -> egui::Response {
    let mut rich = egui::RichText::new(text).size(size).color(color);
    if strong {
        rich = rich.strong();
    }
    let response = ui.push_id(id_salt, |ui| ui.label(rich)).inner;
    let live = match severity {
        StatusSeverity::Error => egui::accesskit::Live::Assertive,
        StatusSeverity::Info | StatusSeverity::Success => egui::accesskit::Live::Polite,
    };
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(egui::accesskit::Role::Status);
        node.set_live(live);
        node.set_label(text);
        node.set_value(text);
    });
    response
}

pub(crate) fn allowance_entry_count_label(count: usize) -> String {
    if count == 1 {
        "1 entry".to_owned()
    } else {
        format!("{count} entries")
    }
}

fn wallet_count_label(count: usize) -> String {
    if count == 1 {
        "1 wallet".to_owned()
    } else {
        format!("{count} wallets")
    }
}

/// UI prefs restored from eframe storage at startup.
#[derive(Debug, PartialEq)]
struct RestoredUiState {
    /// The eagerly-clamped index (a placeholder only good enough for the
    /// pre-unlock/fresh-install path -- see the call site in `CofferlyApp::new`).
    wallet_index: usize,
    ledger_sort: LedgerSort,
    /// The persisted wallet name (if any), which `apply_unlock` resolves
    /// against the real wallet list once it's known.
    wallet_name: Option<String>,
    /// The last-selected Money in/out kind.
    entry_kind: EntryKind,
    /// The selected kid's ledger description filter.
    ledger_filter: String,
    /// The per-child filter map (applied directly -- unlike the wallet name,
    /// the filters need no real data to resolve against).
    ledger_filters: HashMap<String, String>,
    last_backup_date: Option<String>,
}

impl Default for RestoredUiState {
    fn default() -> Self {
        Self {
            wallet_index: 0,
            ledger_sort: LedgerSort::NewestFirst,
            wallet_name: None,
            entry_kind: EntryKind::default(),
            ledger_filter: String::new(),
            ledger_filters: HashMap::new(),
            last_backup_date: None,
        }
    }
}

fn restore_ui_state(storage: Option<&dyn eframe::Storage>, wallet_count: usize) -> RestoredUiState {
    let wallet_count = wallet_count.max(1);
    let Some(storage) = storage else {
        return RestoredUiState::default();
    };
    let Some(state) = eframe::get_value::<UiState>(storage, UI_STATE_KEY) else {
        return RestoredUiState::default();
    };
    let wallet_index = state.selected_wallet.min(wallet_count.saturating_sub(1));
    let ledger_sort = if state.ledger_sort_newest_first {
        LedgerSort::NewestFirst
    } else {
        LedgerSort::OldestFirst
    };
    RestoredUiState {
        wallet_index,
        ledger_sort,
        wallet_name: state.selected_wallet_name,
        entry_kind: state.last_entry_kind,
        ledger_filter: restore_ledger_filter(state.ledger_filter),
        ledger_filters: restore_ledger_filters(state.ledger_filters),
        last_backup_date: state.last_backup,
    }
}

const LEDGER_FILTER_RESTORE_MAX_CHARS: usize = 80;

fn restore_ledger_filter(saved: String) -> String {
    if saved.chars().count() <= LEDGER_FILTER_RESTORE_MAX_CHARS {
        saved
    } else {
        saved
            .chars()
            .take(LEDGER_FILTER_RESTORE_MAX_CHARS)
            .collect()
    }
}

fn restore_ledger_filters(saved: HashMap<String, String>) -> HashMap<String, String> {
    saved
        .into_iter()
        .map(|(name, query)| (name, restore_ledger_filter(query)))
        .collect()
}

pub(crate) fn pin_digit_id(index: usize) -> egui::Id {
    egui::Id::unique(("parent_pin_digit", index))
}

pub(crate) fn entry_field_id(field: EntryFormField) -> egui::Id {
    egui::Id::unique(("entry_form_field", field))
}

pub(crate) fn ledger_filter_id() -> egui::Id {
    egui::Id::unique("ledger_filter")
}

fn wallet_keyboard_delta(key: egui::Key) -> Option<isize> {
    match key {
        egui::Key::ArrowDown | egui::Key::CloseBracket => Some(1),
        egui::Key::ArrowUp | egui::Key::OpenBracket => Some(-1),
        _ => None,
    }
}

fn consume_wallet_keyboard_delta(ctx: &egui::Context) -> Option<isize> {
    ctx.input_mut(|input| {
        for key in [
            egui::Key::ArrowDown,
            egui::Key::CloseBracket,
            egui::Key::ArrowUp,
            egui::Key::OpenBracket,
        ] {
            if input.consume_key(egui::Modifiers::NONE, key) {
                return wallet_keyboard_delta(key);
            }
        }
        None
    })
}

fn next_wallet_index(current: usize, count: usize, delta: isize) -> usize {
    if count == 0 {
        return 0;
    }
    let max_index = count - 1;
    if delta >= 0 {
        let step = usize::try_from(delta).unwrap_or(usize::MAX);
        current.saturating_add(step).min(max_index)
    } else {
        let step = usize::try_from(-delta).unwrap_or(usize::MAX);
        current.saturating_sub(step)
    }
}

fn wallet_selection_announcement(name: &str, balance_cents: i64) -> String {
    format!("{}, balance {}", name, format_money(balance_cents))
}

/// Free wrong attempts before the cooldown starts. Absorbs an honest misclick
/// without materially changing brute-force math against the ~4×10⁸-story keyspace.
const UNLOCK_GRACE_ATTEMPTS: u32 = 2;

fn unlock_cooldown_duration(failed_attempts: u32) -> Duration {
    if failed_attempts <= UNLOCK_GRACE_ATTEMPTS {
        return Duration::ZERO;
    }
    let max_index =
        u32::try_from(UNLOCK_COOLDOWN_MINUTES.len() - 1).expect("cooldown table is non-empty");
    let offset = failed_attempts.saturating_sub(UNLOCK_GRACE_ATTEMPTS + 1);
    let index = usize::try_from(offset.min(max_index)).unwrap_or(0);
    Duration::from_secs(UNLOCK_COOLDOWN_MINUTES[index] * 60)
}

fn format_cooldown(duration: Duration) -> String {
    let seconds = duration
        .as_secs()
        .saturating_add(u64::from(duration.subsec_nanos() > 0))
        .max(1);
    if seconds >= 60 {
        let minutes = seconds.div_ceil(60);
        if minutes == 1 {
            "1 minute".to_owned()
        } else {
            format!("{minutes} minutes")
        }
    } else if seconds == 1 {
        "1 second".to_owned()
    } else {
        format!("{seconds} seconds")
    }
}

fn load_lock_screen_image(ctx: &egui::Context) -> (Option<egui::TextureHandle>, egui::Color32) {
    let dyn_image = match image::load_from_memory(LOCK_SCREEN_IMAGE_BYTES) {
        Ok(img) => img,
        Err(_) => return (None, egui::Color32::from_rgb(232, 227, 223)),
    };
    let rgba = dyn_image.to_rgba8();

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

fn load_open_coffer_image(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let rgba = image::load_from_memory(OPEN_COFFER_IMAGE_BYTES)
        .ok()?
        .to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());

    Some(ctx.load_texture(
        "cofferly-open-coffer-image",
        color_image,
        egui::TextureOptions::NEAREST,
    ))
}

fn load_story_icon_textures(ctx: &egui::Context) -> HashMap<&'static str, egui::TextureHandle> {
    macro_rules! add_icons {
        ($($id:literal),+ $(,)?) => {{
            let mut textures = HashMap::new();
            $(
                let bytes = include_bytes!(concat!("../assets/story-icons/", $id, ".png"));
                if let Ok(image) = image::load_from_memory(bytes) {
                    let rgba = image.to_rgba8();
                    let size = [rgba.width() as usize, rgba.height() as usize];
                    textures.insert(
                        $id,
                        ctx.load_texture(
                            concat!("coffer-story-icon-", $id),
                            egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
                            egui::TextureOptions::LINEAR,
                        ),
                    );
                }
            )+
            textures
        }};
    }

    add_icons!(
        "acorn", "anchor", "apple", "balloon", "book", "bridge", "candle", "castle", "cat",
        "cloud", "compass", "crown", "diamond", "drum", "feather", "fish", "flower", "fox",
        "globe", "sun", "hammer", "hat", "heart", "house", "key", "kite", "lantern", "leaf",
        "lemon", "map",
    )
}

#[cfg(test)]
mod app_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod ui_tests;
