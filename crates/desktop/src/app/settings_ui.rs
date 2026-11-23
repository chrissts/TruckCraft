use super::*;

pub(super) struct SettingsUi {
    pub open: bool,
    viewer: bool,
    page: usize,
    text: Option<String>,
    viewer_error: Option<String>,
    log_error: Option<String>,
    log: crate::activity::EventLog,
    last_status: String,
    last_error: Option<String>,
}
impl SettingsUi {
    pub fn new(isolated: bool, enabled: bool) -> Self {
        let mut log = crate::activity::EventLog::new(isolated || cfg!(test));
        log.enabled = enabled;
        Self {
            open: false,
            viewer: false,
            page: 0,
            text: None,
            viewer_error: None,
            log_error: None,
            log,
            last_status: String::new(),
            last_error: None,
        }
    }
    pub fn record(&mut self, level: &str, text: &str) {
        self.log_error = self
            .log
            .record(level, text)
            .err()
            .map(|error| error.to_string());
    }
    pub fn observe(&mut self, status: &str, error: &Option<String>) {
        if status != self.last_status {
            self.record("INFO", status);
            self.last_status = status.into();
        }
        if error != &self.last_error {
            if let Some(error) = error {
                self.record("ERROR", error);
            }
            self.last_error = error.clone();
        }
    }
    fn refresh_log(&mut self) {
        match self.log.read() {
            Ok(text) => {
                self.page = text
                    .as_ref()
                    .map_or(0, |text| text.lines().count().saturating_sub(1) / 200);
                self.text = text;
                self.viewer_error = None;
            }
            Err(error) => {
                self.text = None;
                self.viewer_error = Some(error.to_string());
            }
        }
    }
}

impl EditorApp {
    pub(super) fn application_settings(&mut self, context: &egui::Context) {
        if self.activity.open {
            let mut open = true;
            egui::Window::new(tr("TruckCraft settings")).id(egui::Id::unique("application_settings")).open(&mut open).collapsible(false).default_width(480.0).show(context,|ui| {
                ui.heading(tr("Language"));
                let languages = crate::i18n::languages();
                let current = self.browser.language();
                let mut selected = crate::i18n::normalize(current);
                design::combo(ui, "application_language", 240.0)
                    .selected_text(&languages[selected].name)
                    .show_ui(ui, |ui| {
                        for (code, language) in crate::i18n::menu_languages() {
                            ui.selectable_value(&mut selected, code, &language.name);
                        }
                    });
                if selected != current {
                    match self.browser.set_language(selected, !self.isolated) {
                        Ok(()) => context.request_repaint(),
                        Err(error) => self.error = Some(error.to_string()),
                    }
                }
                ui.hyperlink_to(tr("Want another language? Submit a translation PR."), concat!("https://github.com/chrissts/TruckCraft", "/blob/main/AGENTS.md#add-a-language"));
                if !self.isolated {
                    ui.separator();
                    ui.heading(tr("Automatic update checks"));
                    let mut enabled=self.browser.check_updates_on_launch().unwrap_or(false);
                    if ui.checkbox(&mut enabled,tr("Check for updates at launch")).changed()
                        && let Err(error)=self.browser.set_check_updates_on_launch(enabled,true)
                    {
                        self.error=Some(error.to_string());
                    }
                    ui.label(tr("TruckCraft can contact GitHub over the internet at launch to check for a newer version."));
                }
                ui.separator();
                self.game_data_controls(ui);
                ui.separator();ui.heading(tr("Activity log"));
                let mut enabled=self.browser.logging_enabled();
                if ui.checkbox(&mut enabled,tr("Write TruckCraft.log")).changed() {
                    match self.browser.set_logging(enabled,!self.isolated) {
                        Ok(())=> {
                            if !enabled {self.activity.record("INFO","Activity logging disabled");}
                            self.activity.log.enabled=enabled;
                            if enabled {self.activity.record("INFO","Activity logging enabled");}
                        },
                        Err(error)=>self.error=Some(error.to_string()),
                    }
                }
                ui.label(tr("Events are written in time order, with the newest events at the bottom. Turning logging off retains the existing log."));
                ui.small(tr("The log keeps recent activity up to 2 MB, removing older entries when needed."));
                if let Some(path)=&self.activity.log.path {
                    ui.label(format!("{}: {}",tr("Log file"),path.display()));
                    if ui.button(tr("Copy log path")).clicked() {ui.ctx().copy_text(path.display().to_string());}
                }
                if let Some(error)=&self.activity.log_error {ui.colored_label(Color32::from_rgb(172,55,43),format!("{}: {error}",tr("Could not write activity log")));}
                if ui.button(tr("Log viewer…")).clicked() {self.activity.refresh_log();self.activity.viewer=true;}
            });
            self.activity.open = open;
        }
        if self.activity.viewer {
            let mut open = true;
            egui::Window::new(tr("TruckCraft log viewer")).id(egui::Id::unique("TruckCraft log viewer")).open(&mut open).collapsible(false).default_width(800.0).default_height(450.0).show(context,|ui| {
                ui.horizontal_wrapped(|ui| {ui.label(tr("Oldest first · newest at the bottom · timestamps in UTC"));if ui.button(tr("Refresh")).clicked() {self.activity.refresh_log();}});
                if let Some(error)=&self.activity.viewer_error {ui.label(format!("{}: {error}",tr("The log could not be read")));}
                else if let Some(text)=&self.activity.text {
                    if text.is_empty() {ui.label(tr("The log file is empty. Events will appear here when logging is enabled."));}
                    else {
                        let lines:Vec<_>=text.lines().collect();let last=lines.len().saturating_sub(1)/200;self.activity.page=self.activity.page.min(last);
                        ui.horizontal_wrapped(|ui| {
                            if ui.add_enabled(self.activity.page>0,egui::Button::new(tr("Older"))).clicked() {self.activity.page-=1;}
                            ui.label(tr("Page {current} of {total}")
                                .replace("{current}", &(self.activity.page + 1).to_string())
                                .replace("{total}", &(last + 1).to_string()));
                            if ui.add_enabled(self.activity.page<last,egui::Button::new(tr("Newer"))).clicked() {self.activity.page+=1;}
                            if ui.button(tr("Latest")).clicked() {self.activity.page=last;}
                        });
                        let start=self.activity.page*200;let end=(start+200).min(lines.len());let page=lines[start..end].join("\n");
                        egui::ScrollArea::vertical().id_salt(("activity_log_page",self.activity.page)).stick_to_bottom(true).max_height(380.0).show(ui,|ui| {ui.add(egui::Label::new(RichText::new(page).monospace()).wrap().selectable(true));});
                    }
                } else {ui.label(tr("No TruckCraft.log file was found. Enable activity logging in Settings, then perform an action to create it."));}
            });
            self.activity.viewer = open;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn viewer_handles_missing_empty_and_existing_logs_without_creating_one() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("TruckCraft.log");
        let context = egui::Context::default();
        let mut app = EditorApp::new(&eframe::CreationContext::_new_kittest(context.clone()));
        app.activity.log = crate::activity::EventLog {
            path: Some(path.clone()),
            enabled: false,
        };
        app.activity.open = true;
        app.activity.viewer = true;
        app.activity.refresh_log();
        assert!(app.activity.text.is_none());
        assert!(app.activity.viewer_error.is_none());
        for _ in 0..3 {
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 600.0),
                    )),
                    ..Default::default()
                },
                |_| app.application_settings(&context),
            );
            output.textures_delta.clear();
        }
        assert!(
            !path.exists(),
            "viewing a missing log must not create a log file"
        );
        std::fs::write(&path, "").unwrap();
        app.activity.refresh_log();
        assert_eq!(app.activity.text.as_deref(), Some(""));
        app.activity.log.enabled = true;
        app.activity.record("INFO", "Example action");
        app.activity.refresh_log();
        assert!(
            app.activity
                .text
                .as_deref()
                .unwrap()
                .ends_with("Example action\n")
        );
        std::fs::remove_file(path).unwrap();
        app.activity.refresh_log();
        assert!(app.activity.text.is_none());
    }
}
