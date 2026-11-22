use super::*;
use ts_save_core::cars::{self, CarState};

impl EditorApp {
    pub(super) fn car(&mut self, ui: &mut egui::Ui) {
        let Some(session) = &self.session else {
            return;
        };
        let state = match cars::detect(&session.data) {
            Ok(state) => state,
            Err(error) => {
                ui.heading(tr("Car"));
                ui.label(error.to_string());
                return;
            }
        };
        ui.label(state.message());
        if let CarState::Ready { job, .. } = &state {
            ui.heading(tr("Road Trip journey"));
            ui.label(tr("An active saved car journey is available. Fuel, condition and number-plate edits are staged with Undo/Redo."));
            let source = session.data.game.string(job, "source_company").ok();
            let target = session.data.game.string(job, "target_company").ok();
            if let (Some(source), Some(target)) = (source, target) {
                ui.label(format!(
                    "{} → {}",
                    freight_ui::company_label(&source),
                    freight_ui::company_label(&target)
                ));
            }
        }
        if !state.ready() {
            return;
        }
        ui.separator();
        self.vehicle(ui);
    }
}
