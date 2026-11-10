//! Native file dialogs that leave the window responsive.
//!
//! A blocking dialog stops the event loop, so the window stops answering the
//! desktop's "are you alive?" pings and GNOME offers to kill the app while
//! you browse. Instead the dialog's future is awaited on its own thread and
//! the answer is picked up on a later frame. Meanwhile [`waiting`] covers the
//! app, so nothing in it changes until the dialog has closed.

use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use egui::RichText;

pub use rfd::AsyncFileDialog as Dialog;

use crate::ui::{icons, theme};

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
            // A dialog that fails (a broken desktop portal) counts as
            // cancelled, rather than staying open for good.
            let path = std::panic::catch_unwind(AssertUnwindSafe(|| {
                pollster::block_on(dialog).map(|h| h.path().to_path_buf())
            }))
            .unwrap_or(None);
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

/// Covers the app while a file dialog is open, so it can't be changed (or
/// another dialog opened) until the dialog has its answer. Draw it after
/// everything else, to be on top.
pub fn waiting(ui: &egui::Ui) {
    egui::Modal::new(egui::Id::unique("file-dialog-open")).frame(theme::modal_frame(ui)).show(ui.ctx(), |ui| {
        ui.set_width(380.0);
        ui.horizontal(|ui| {
            theme::icon_badge(ui, icons::FLOPPY_DISK, theme::ACCENT, 40.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(theme::serif("Choose a file", 22.0));
                ui.add(
                    egui::Label::new(
                        RichText::new("In the window that opened. If you can't see it, it may be behind this one.")
                            .color(theme::muted(ui)),
                    )
                    .wrap(),
                );
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(pending: &Pending) -> Option<PathBuf> {
        for _ in 0..500 {
            if let Some(answer) = pending.poll() {
                return answer;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("no answer")
    }

    #[test]
    fn a_dialog_that_fails_counts_as_cancelled() {
        let ctx = egui::Context::default();
        let failing = async { panic!("the desktop portal went away") };
        assert_eq!(wait(&Pending::open(&ctx, failing)), None);
        let chosen = async { Some(rfd::FileHandle::from(PathBuf::from("/tmp/chosen.ics"))) };
        let pending = Pending::open(&ctx, chosen);
        assert_eq!(wait(&pending), Some(PathBuf::from("/tmp/chosen.ics")));
        assert_eq!(pending.poll(), None, "taken once");
    }
}
