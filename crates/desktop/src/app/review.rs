use super::*;
use ts_save_core::document::FieldChange;

impl EditorApp {
    pub(super) fn draft_links(&mut self, ui: &mut egui::Ui) {
        let mut tabs = self
            .baseline
            .as_ref()
            .map(|base| self.input.draft_tabs(base, self.tab))
            .unwrap_or_default();
        if self.freight.has_drafts() {
            tabs.push(5);
        }
        if self.management.has_drafts() {
            tabs.push(1);
        }
        tabs.sort_unstable();
        tabs.dedup();
        ui.horizontal_wrapped(|ui| {
            for tab in tabs {
                let label = tr(self.artwork.tabs(self.game)[tab].0);
                if ui
                    .add_enabled(
                        tab == self.tab || !self.vehicle_draft(),
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    if self.tab != tab {
                        self.tab = tab;
                        self.input.vehicle_index = 0;
                        self.refresh_vehicle();
                    }
                    if tab == 1 {
                        self.management.page = if self.management.has_drafts() { 2 } else { 0 };
                    }
                }
            }
        });
    }

    pub(super) fn review_dialog(&mut self, ctx: &egui::Context) {
        if !self.review_open {
            return;
        }
        let Some(session) = &self.session else {
            return;
        };
        egui::Window::new(tr("Review changes"))
            .id(egui::Id::unique("review_changes"))
            .open(&mut self.review_open)
            .collapsible(false)
            .resizable(false)
            .default_width(510.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .show(ui, |ui| {
                        review_changes(ui, session, self.game);
                    });
            });
    }
}

fn review_changes(ui: &mut egui::Ui, session: &SaveSession, game: Game) {
    if !session.is_dirty() {
        ui.label(tr("No staged changes."));
    }
    for file in session.replaced_files() {
        ui.label(format!("{} · {file}", tr("Restore recovery copy")));
    }
    for (name, written) in session.staged_settings() {
        ui.label(format!(
            "{} · {name}",
            tr(if written {
                "Write settings"
            } else {
                "Remove settings"
            })
        ));
    }
    for (file, doc) in [
        ("profile.sii", &session.data.profile),
        ("info.sii", &session.data.info),
        ("game.sii", &session.data.game),
    ] {
        if !doc.is_dirty() {
            continue;
        }
        ui.strong(file);
        for (i, change) in doc.changes().enumerate() {
            if i == 200 {
                ui.label(tr("Additional edits are included in this file."));
                break;
            }
            let text = match change {
                FieldChange::Structure => tr("Rebuilt units and their references").into(),
                FieldChange::Removed(unit) => format!("{} · {unit}", tr("Remove unit")),
                FieldChange::Array { unit, field, count } => {
                    format!("{unit} · {field} · {count} {}", tr("entries"))
                }
                FieldChange::Value {
                    unit,
                    field,
                    item,
                    before,
                    after,
                } => {
                    let label = match field {
                        "money_account" => tr("Account balance").to_owned(),
                        "company_name" => tr("Company name").to_owned(),
                        "hq_city" => tr("Headquarters").to_owned(),
                        "experience_points" => tr("XP").to_owned(),
                        _ => format!(
                            "{unit} · {field}{}",
                            item.map(|n| format!("[{n}]")).unwrap_or_default()
                        ),
                    };
                    let display = |raw: &str| {
                        if field == "money_account"
                            && let Ok(value) = raw.parse()
                        {
                            return format!(
                                "{}{}",
                                if game == Game::Ats { "$" } else { "€" },
                                balance_text(value)
                            );
                        }
                        let raw = raw.trim_matches('"');
                        if field == "hq_city" {
                            return city_label(raw);
                        }
                        let mut chars = raw.chars();
                        let mut text: String = chars.by_ref().take(100).collect();
                        if chars.next().is_some() {
                            text.push('…');
                        }
                        text
                    };
                    format!("{label}   {} → {}", display(before), display(after))
                }
            };
            ui.add(egui::Label::new(text).wrap());
        }
    }
}
