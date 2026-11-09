//! Native file dialogs that leave the window responsive.
//!
//! A blocking dialog stops the event loop, so the window stops answering the
//! desktop's "are you alive?" pings and GNOME offers to kill the app while
//! you browse. Instead the dialog's future is awaited on its own thread and
//! the answer is picked up on a later frame.

use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub use rfd::AsyncFileDialog as Dialog;

/// A dialog that is open, or has closed with an answer not yet taken.
pub struct Pending(Arc<Mutex<Option<Option<PathBuf>>>>);

impl Pending {
    /// Open the dialog. Build it with [`Dialog`] on the UI thread (macOS
    /// needs that) and pass its `pick_file()`, `save_file()`, … future.
    pub fn open(ctx: &egui::Context, dialog: impl Future<Output = Option<rfd::FileHandle>> + Send + 'static) -> Self {
        let slot = Arc::new(Mutex::new(None));
        let answer = Arc::clone(&slot);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let path = pollster::block_on(dialog).map(|h| h.path().to_path_buf());
            *answer.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
            ctx.request_repaint();
        });
        Self(slot)
    }

    /// `None` while the dialog is open; then once, the chosen path, or
    /// `Some(None)` if it was cancelled.
    pub fn poll(&self) -> Option<Option<PathBuf>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}
