//! The app's transient "pending prompt" state, owned in one place.
//!
//! Every field is private to this module, so a prompt can only be armed or
//! dismissed through the methods below. `dismiss_all()` resets through
//! `Default`, which means a prompt field added later is cleared on lock
//! automatically (the `lock_clears_every_prompt` test enforces it).

use std::path::PathBuf;

use crate::PendingUndo;

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Prompts {
    /// Settings "Delete wallet" awaiting its confirm click.
    delete_wallet: bool,
    /// A Money-out (or correction) that would leave the wallet below $0,
    /// waiting for a second submit of the same amount.
    spend_anyway: Option<i64>,
    /// Entry index of the open correction whose removal awaits a second click.
    remove_edited: Option<usize>,
    /// A backup destination that already exists, awaiting "Replace".
    backup_overwrite: Option<PathBuf>,
    /// The single pending undo (a removal or a correction).
    undo: Option<PendingUndo>,
}

impl Prompts {
    /// Lock: nothing may survive it.
    pub(crate) fn dismiss_all(&mut self) {
        *self = Self::default();
    }

    /// Wallet switch: everything tied to the old wallet, but not a pending
    /// backup overwrite (it is about the vault file, not a wallet).
    pub(crate) fn dismiss_for_wallet_switch(&mut self) {
        self.delete_wallet = false;
        self.spend_anyway = None;
        self.remove_edited = None;
        self.undo = None;
    }

    /// Opening Settings: stale delete and backup-overwrite confirms only.
    pub(crate) fn dismiss_for_settings_open(&mut self) {
        self.delete_wallet = false;
        self.backup_overwrite = None;
    }

    /// Settings/ledger mutations: a new change retires the old undo and any
    /// armed delete-wallet confirm.
    pub(crate) fn dismiss_undo_and_delete_wallet(&mut self) {
        self.undo = None;
        self.delete_wallet = false;
    }

    // Delete wallet.
    pub(crate) fn delete_wallet_pending(&self) -> bool {
        self.delete_wallet
    }
    pub(crate) fn arm_delete_wallet(&mut self) {
        self.delete_wallet = true;
    }
    pub(crate) fn dismiss_delete_wallet(&mut self) {
        self.delete_wallet = false;
    }

    // Spend anyway.
    pub(crate) fn spend_anyway_pending(&self) -> bool {
        self.spend_anyway.is_some()
    }
    pub(crate) fn spend_anyway_is(&self, amount: i64) -> bool {
        self.spend_anyway == Some(amount)
    }
    pub(crate) fn arm_spend_anyway(&mut self, amount: i64) {
        self.spend_anyway = Some(amount);
    }
    pub(crate) fn dismiss_spend_anyway(&mut self) {
        self.spend_anyway = None;
    }
    /// Clears the prompt and reports whether one was pending.
    pub(crate) fn take_spend_anyway(&mut self) -> bool {
        self.spend_anyway.take().is_some()
    }

    // Remove edited entry.
    pub(crate) fn remove_edited_pending(&self) -> bool {
        self.remove_edited.is_some()
    }
    pub(crate) fn remove_edited_is(&self, entry_index: usize) -> bool {
        self.remove_edited == Some(entry_index)
    }
    pub(crate) fn arm_remove_edited(&mut self, entry_index: usize) {
        self.remove_edited = Some(entry_index);
    }
    pub(crate) fn dismiss_remove_edited(&mut self) {
        self.remove_edited = None;
    }

    // Backup overwrite.
    pub(crate) fn backup_overwrite(&self) -> Option<&PathBuf> {
        self.backup_overwrite.as_ref()
    }
    pub(crate) fn arm_backup_overwrite(&mut self, dest: PathBuf) {
        self.backup_overwrite = Some(dest);
    }
    pub(crate) fn dismiss_backup_overwrite(&mut self) {
        self.backup_overwrite = None;
    }

    // Undo.
    pub(crate) fn undo(&self) -> Option<&PendingUndo> {
        self.undo.as_ref()
    }
    pub(crate) fn arm_undo(&mut self, undo: PendingUndo) {
        self.undo = Some(undo);
    }
    pub(crate) fn take_undo(&mut self) -> Option<PendingUndo> {
        self.undo.take()
    }
    /// Puts back a snapshotted undo after a failed save.
    pub(crate) fn restore_undo(&mut self, undo: Option<PendingUndo>) {
        self.undo = undo;
    }

    #[cfg(test)]
    pub(crate) fn is_idle(&self) -> bool {
        *self == Self::default()
    }
}
