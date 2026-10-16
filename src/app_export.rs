//! Print and CSV export methods for [`CofferlyApp`], split out of `main.rs`.

use super::*;

impl CofferlyApp {
    pub(crate) fn print_selected_wallet(&mut self) {
        if !self.save_enabled {
            self.set_status_err("Saved data could not be loaded, so printing is disabled.");
            return;
        }

        let Ok(path) = self.print_path(false) else {
            self.set_status_err("Could not create printable ledger: temp file unavailable.");
            return;
        };
        let filter = self.selected_description_filter();
        match write_printable_ledger_filtered(
            &path,
            &[self.selected_wallet().clone()],
            filter.as_deref().unwrap_or(""),
            Local::now().date_naive(),
        ) {
            Ok(path) => self.open_selected_export(path, "printable ledger", filter.as_deref()),
            Err(err) => self.set_status_err(format!("Could not create printable ledger: {err}")),
        }
    }

    pub(crate) fn print_all_wallets(&mut self) {
        if !self.save_enabled {
            self.set_status_err("Saved data could not be loaded, so printing is disabled.");
            return;
        }

        let Ok(path) = self.print_path(true) else {
            self.set_status_err("Could not create printable ledger: temp file unavailable.");
            return;
        };
        // Full book: the selected wallet's description filter does not apply.
        match write_printable_ledger(&path, &self.data.wallets, Local::now().date_naive()) {
            Ok(path) => self.open_export_file(path, "printable ledger"),
            Err(err) => self.set_status_err(format!("Could not create printable ledger: {err}")),
        }
    }

    pub(crate) fn export_selected_wallet_csv(&mut self) {
        if !self.save_enabled {
            self.set_status_err("Saved data could not be loaded, so export is disabled.");
            return;
        }

        let Ok(path) = self.csv_path(false) else {
            self.set_status_err("Could not create CSV ledger: temp file unavailable.");
            return;
        };
        let filter = self.selected_description_filter();
        match write_csv_ledger_filtered(
            &path,
            &[self.selected_wallet().clone()],
            filter.as_deref().unwrap_or(""),
        ) {
            Ok(path) => self.open_selected_export(path, "CSV ledger", filter.as_deref()),
            Err(err) => self.set_status_err(format!("Could not create CSV ledger: {err}")),
        }
    }

    pub(crate) fn export_all_wallets_csv(&mut self) {
        if !self.save_enabled {
            self.set_status_err("Saved data could not be loaded, so export is disabled.");
            return;
        }

        let Ok(path) = self.csv_path(true) else {
            self.set_status_err("Could not create CSV ledger: temp file unavailable.");
            return;
        };
        // Full book: the selected wallet's description filter does not apply.
        match write_csv_ledger(&path, &self.data.wallets) {
            Ok(path) => self.open_export_file(path, "CSV ledger"),
            Err(err) => self.set_status_err(format!("Could not create CSV ledger: {err}")),
        }
    }

    /// Trimmed description filter for the selected wallet, if it would drop rows.
    pub(crate) fn selected_description_filter(&self) -> Option<String> {
        let query = self.ledger_filter.trim();
        if query.is_empty() {
            None
        } else {
            Some(query.to_owned())
        }
    }

    pub(crate) fn filtered_entry_count(&self, query: &str) -> usize {
        let rows = self
            .selected_wallet()
            .ledger_rows_sorted_owned(LedgerSort::OldestFirst);
        ledger_filter_summary(&rows, query).matching_entry_count
    }

    fn open_selected_export(&mut self, path: PathBuf, kind: &str, filter: Option<&str>) {
        let filtered_count = filter.map(|query| self.filtered_entry_count(query));
        let result = opener::open(&path).map_err(|err| err.to_string());
        let opened = result.is_ok();
        self.apply_export_open_result(path.clone(), kind, result);
        if opened {
            if let (Some(query), Some(count)) = (filter, filtered_count) {
                self.status =
                    Status::success(filtered_export_opened_status(kind, &path, query, count));
            }
        }
    }

    fn open_export_file(&mut self, path: PathBuf, kind: &str) {
        let result = opener::open(&path).map_err(|err| err.to_string());
        self.apply_export_open_result(path, kind, result);
    }

    pub(crate) fn apply_export_open_result(
        &mut self,
        path: PathBuf,
        kind: &str,
        result: Result<(), String>,
    ) {
        self.track_temp_artifact(path.clone());
        self.status = match result {
            Ok(()) => export_opened_status(kind, &path),
            Err(err) => export_opener_failed_status(kind, &path, err),
        };
    }

    pub(crate) fn print_path(&self, all_wallets: bool) -> Result<PathBuf, String> {
        self.export_temp_path(all_wallets, "html")
    }

    pub(crate) fn csv_path(&self, all_wallets: bool) -> Result<PathBuf, String> {
        self.export_temp_path(all_wallets, "csv")
    }

    fn export_temp_path(&self, all_wallets: bool, ext: &str) -> Result<PathBuf, String> {
        let stem = if all_wallets {
            "ledgers".to_owned()
        } else {
            format!(
                "{}-ledger",
                ledger_file_stem(&self.selected_wallet().child_name)
            )
        };

        // Ephemeral, unpredictably-named location — never store plaintext
        // ledgers next to encrypted data.
        reserve_private_temp_path(&stem, ext)
    }
}

pub(crate) fn export_opened_status(kind: &str, path: &Path) -> Status {
    Status::success(format!(
        "Opened {kind}. Print or save it from the window that opened, or find it at {}.",
        path.display()
    ))
}

pub(crate) fn export_opener_failed_status(
    kind: &str,
    path: &Path,
    err: impl std::fmt::Display,
) -> Status {
    Status::error(format!(
        "{kind} was saved to {}, but Cofferly could not open it ({err}). Open that file from your file manager to print or import it.",
        path.display()
    ))
}

pub(crate) fn filtered_export_opened_status(
    kind: &str,
    path: &Path,
    filter: &str,
    entry_count: usize,
) -> String {
    let entries = if entry_count == 1 {
        "1 entry".to_owned()
    } else {
        format!("{entry_count} entries")
    };
    format!(
        "Opened {kind} with {entries} included; description filter \"{filter}\" applied. Print or save it from the window that opened, or find it at {}.",
        path.display()
    )
}
