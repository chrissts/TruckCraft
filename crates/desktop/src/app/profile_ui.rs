use super::*;
use ts_save_core::profile_files::{self, Operation};

#[derive(Default)]
pub(super) struct ProfileTools {
    settings: bool,
    settings_files: Vec<ts_save_core::settings::SettingsFile>,
    selected_files: Vec<bool>,
    settings_source: String,
    recovery: bool,
    recovery_slot: Option<PathBuf>,
    recovered_name: String,
    snapshots: Vec<PathBuf>,
    snapshot_index: usize,
    identity: bool,
    portrait_page: usize,
    logo_page: usize,
    pub open: bool,
    pub running: bool,
    name: String,
    rename: bool,
    autosave_only: bool,
}
impl ProfileTools {
    fn set_files(&mut self, files: Vec<ts_save_core::settings::SettingsFile>, source: &str) {
        self.selected_files = vec![true; files.len()];
        self.settings_files = files;
        self.settings_source = source.into();
    }

    fn selected(&self) -> Vec<ts_save_core::settings::SettingsFile> {
        self.settings_files
            .iter()
            .zip(&self.selected_files)
            .filter(|(_, checked)| **checked)
            .map(|(file, _)| file.clone())
            .collect()
    }
}

fn load_bundle(
    game: Game,
    path: &std::path::Path,
) -> Result<Vec<ts_save_core::settings::SettingsFile>> {
    let bytes = ts_save_core::discovery::read_small(path, ts_save_core::settings::MAX_BUNDLE)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| Error::InvalidEdit("Settings bundle must be UTF-8.".into()))?;
    ts_save_core::settings::import(game, text)
}

fn export_bundle(
    game: Game,
    path: &std::path::Path,
    files: &[ts_save_core::settings::SettingsFile],
) -> Result<()> {
    use std::io::Write;
    let text = ts_save_core::settings::export(game, files)?;
    let mut file = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or(Error::InvalidSii("settings export folder"))?,
    )?;
    file.write_all(text.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| Error::Io(error.error))?;
    Ok(())
}

impl EditorApp {
    pub(super) fn identity_controls(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(
                self.input.identity.is_some(),
                egui::Button::new(tr("Portrait and company logo…")),
            )
            .clicked()
        {
            self.profile_tools.identity = true;
        }
        if !self.profile_tools.identity {
            return;
        }
        let Some(mut identity) = self.input.identity.clone() else {
            return;
        };
        let mut open = true;
        let mut apply = false;
        let faces = crate::game_artwork::GameArtwork::stock_choices(self.game, false);
        let logos = crate::game_artwork::GameArtwork::stock_choices(self.game, true);
        egui::Window::new(tr("Profile portrait and company logo"))
            .id(egui::Id::unique("Profile portrait and company logo"))
            .open(&mut open)
            .collapsible(false)
            .default_width(480.0)
            .vscroll(true)
            .max_height(480.0)
            .show(ui.ctx(), |ui| {
                let hint = if faces.is_empty() && logos.is_empty() {
                    "Stock portraits and logos require local game artwork."
                } else {
                    "Choose stock game artwork. Existing custom choices are retained until you select a replacement."
                };
                ui.label(tr(hint));
                for (logo, choices, page) in [
                    (false, &faces, &mut self.profile_tools.portrait_page),
                    (true, &logos, &mut self.profile_tools.logo_page),
                ] {
                    if choices.is_empty() { continue; }
                    let label = tr(if logo { "Company logo" } else { "Portrait" });
                    ui.strong(label);
                    let pages = choices.len().div_ceil(24);
                    *page = (*page).min(pages - 1);
                    ui.horizontal_wrapped(|ui| {
                        if ui.add_enabled(*page > 0, egui::Button::new(tr("Previous"))).clicked() {
                            *page -= 1;
                        }
                        ui.label(tr("Page {page} of {total}")
                            .replace("{page}", &(*page + 1).to_string())
                            .replace("{total}", &pages.to_string()));
                        if ui.add_enabled(*page + 1 < pages, egui::Button::new(tr("Next"))).clicked() {
                            *page += 1;
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        for &choice in choices.iter().skip(*page * 24).take(24) {
                            let token = if logo { format!("logo_{choice}") } else { choice.to_string() };
                            let selected = if logo { identity.logo == token } else { identity.face == choice };
                            let texture = if logo {
                                self.artwork.game.company_logo(ui.ctx(), self.game, &token)
                            } else {
                                self.artwork.game.portrait(ui.ctx(), self.game, &token)
                            };
                            if let Some(texture) = texture {
                                let image = egui::Image::new((texture.id(), egui::vec2(56.0, 56.0)));
                                let response = ui.add(egui::Button::image(image).selected(selected))
                                    .on_hover_text(format!("{label} {choice}"));
                                response.widget_info(|| egui::WidgetInfo::selected(
                                    egui::Role::RadioButton, ui.is_enabled(), selected, format!("{label} {choice}")));
                                if response.clicked() {
                                    if logo { identity.logo = token; } else { identity.face = choice; }
                                }
                            }
                        }
                    });
                    ui.separator();
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label(tr("Saved driver gender"));
                    ui.radio_value(&mut identity.male, true, tr("Male"));
                    ui.radio_value(&mut identity.male, false, tr("Female"));
                });
                apply = ui.button(tr("Apply portrait and logo")).clicked();
            });
        self.profile_tools.identity = open;
        self.input.identity = Some(identity.clone());
        if apply {
            self.edit_named("Change portrait and company logo", |data| {
                profile::set_identity(data, &identity, &faces, &logos)
            });
        }
    }
    pub(super) fn profile_tools_button(&mut self, ui: &mut egui::Ui) {
        let ready = !self.busy && self.session.is_some() && !self.unsaved();
        if self.session.is_some() {
            self.starter_button(ui);
        }
        if ui
            .add_enabled(ready, egui::Button::new(tr("Rename / clone profile…")))
            .on_hover_text(tr(
                "Load a save and save or undo pending edits before managing its profile folder.",
            ))
            .clicked()
        {
            self.profile_tools.open = true;
            self.profile_tools.name.clear();
        }
        if ui
            .add_enabled(
                !self.busy && self.session.is_some(),
                egui::Button::new(tr("Profile settings…")),
            )
            .clicked()
        {
            let result = self.session.as_mut().unwrap().settings_files();
            match result {
                Ok(files) => {
                    self.profile_tools
                        .set_files(files, "Current profile (including staged settings)");
                    self.profile_tools.settings = true;
                }
                Err(error) => self.error = Some(error.to_string()),
            }
        }
        if ui
            .add_enabled(ready, egui::Button::new(tr("Restore recovery copy…")))
            .clicked()
        {
            match self.session.as_ref().unwrap().recovery_snapshots() {
                Ok(snapshots) => {
                    self.profile_tools.recovery_slot = None;
                    self.profile_tools.snapshots = snapshots;
                    self.profile_tools.snapshot_index = 0;
                    self.profile_tools.recovery = true;
                }
                Err(error) => self.error = Some(error.to_string()),
            }
        }
    }
    pub(super) fn settings_dialog(&mut self, context: &egui::Context) {
        if self.profile_tools.settings && self.session.is_some() {
            let mut open = true;
            egui::Window::new(tr("Profile settings"))
                .id(egui::Id::unique("Profile settings")).open(&mut open)
                .collapsible(false).default_width(520.0).vscroll(true).max_height(480.0)
                .show(context, |ui| {
                    ui.label(tr("Transfer game configuration, local preferences, controls and gearbox layouts. Only selected files are included."));
                    ui.label(tr(&self.profile_tools.settings_source));
                    if self.profile_tools.settings_files.is_empty() {
                        ui.label(tr("This profile has no transferable settings. You can load a bundle from another profile of the same game."));
                    }
                    for (file, checked) in self.profile_tools.settings_files.iter().zip(&mut self.profile_tools.selected_files) {
                        ui.push_id(&file.name, |ui| ui.checkbox(checked, format!("{} · {} {}",file.name,file.bytes.len(),tr("bytes"))));
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(tr("Load settings bundle…")).clicked() {
                            self.choose_settings_bundle();
                        }
                        let selected = self.profile_tools.selected_files.iter().any(|v| *v);
                        if ui.add_enabled(selected, egui::Button::new(tr("Export selected…"))).clicked() {
                            self.export_selected_settings();
                        }
                        if ui.add_enabled(selected && !self.busy, egui::Button::new(tr("Stage selected settings"))).clicked() {
                            let files = self.profile_tools.selected();
                            if let Some(session) = &mut self.session {
                                match session.import_settings(&files) {
                                    Ok(()) => self.status = "Profile settings staged. Use Undo/Redo to review, then Save changes to write them with a recovery copy.".into(),
                                    Err(error) => self.error = Some(error.to_string()),
                                }
                            }
                        }
                    });
                    ui.label(tr("Staging changes no files. Save changes writes settings and save edits together, with changed-file protection and a recovery copy."));
                });
            self.profile_tools.settings = open;
        }
        if self.profile_tools.recovery {
            self.recovery_dialog(context);
        }
    }

    fn choose_settings_bundle(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title(tr("Import TruckCraft profile settings"))
            .add_filter(tr("TruckCraft settings"), &["json"])
            .pick_file()
        {
            match load_bundle(self.game, &path) {
                Ok(files) => self
                    .profile_tools
                    .set_files(files, "Loaded settings bundle"),
                Err(error) => self.error = Some(error.to_string()),
            }
        }
    }

    fn export_selected_settings(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title(tr("Export selected profile settings"))
            .set_file_name("truckcraft-settings.json")
            .add_filter(tr("TruckCraft settings"), &["json"])
            .save_file()
        {
            match export_bundle(self.game, &path, &self.profile_tools.selected()) {
                Ok(()) => self.status = "Selected profile settings exported.".into(),
                Err(error) => self.error = Some(error.to_string()),
            }
        }
    }

    pub(super) fn pick_recovery_slot(&mut self) {
        if let Some(slot) = rfd::FileDialog::new()
            .set_title(tr("Choose the damaged save slot folder"))
            .pick_folder()
        {
            self.show_copy_recovery(slot);
        }
    }

    pub(super) fn show_copy_recovery(&mut self, slot: PathBuf) {
        match ts_save_core::session::recovery_snapshots(&slot) {
            Ok(snapshots) => {
                self.profile_tools.snapshots = snapshots;
                self.profile_tools.snapshot_index = 0;
                self.profile_tools.recovery = true;
                self.profile_tools.recovered_name = tr("Recovered profile").into();
                self.profile_tools.recovery_slot = Some(slot);
                self.browser.missing_save = false;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn recovery_dialog(&mut self, context: &egui::Context) {
        let copy = self.profile_tools.recovery_slot.is_some();
        let mut open = true;
        let mut apply = false;
        egui::Window::new(tr("Restore a recovery copy")).id(egui::Id::unique("Restore a recovery copy"))
            .open(&mut open).collapsible(false).default_width(520.0).show(context, |ui| {
                if let Some(slot) = &self.profile_tools.recovery_slot {
                    ui.label(tr("Create a new profile from a snapshot of this slot."));
                    ui.label(slot.display().to_string());
                    ui.label(tr("New profile name"));
                    ui.text_edit_singleline(&mut self.profile_tools.recovered_name);
                } else {
                    ui.label(tr("Choose one of this save's recovery snapshots, newest first. Restoration is staged with Undo/Redo. Save changes retains the current files in another recovery copy before writing."));
                }
                if self.profile_tools.snapshots.is_empty() { ui.label(tr("No recovery snapshots were found for this save.")); }
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for (index, path) in self.profile_tools.snapshots.iter().enumerate() {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        let date = name.split_once('-')
                            .and_then(|(hex, _)| u128::from_str_radix(hex, 16).ok())
                            .and_then(|ns| u64::try_from(ns / 1_000_000_000).ok())
                            .filter(|seconds| *seconds < 253402300800)
                            .map(|seconds| format!("{} UTC", crate::activity::timestamp(seconds)))
                            .unwrap_or_else(|| name.into_owned());
                        ui.selectable_value(&mut self.profile_tools.snapshot_index, index, format!("{} {} · {date}",tr("Recovery copy"),index+1));
                    }
                });
                let ready = !self.busy && !self.profile_tools.snapshots.is_empty() &&
                    if copy { profile_files::folder_name(&self.profile_tools.recovered_name).is_ok() }
                    else { self.session.is_some() && !self.unsaved() };
                apply = ui.add_enabled(ready, egui::Button::new(tr(if copy { "Create recovered profile" } else { "Stage restoration" }))).clicked();
            });
        self.profile_tools.recovery = open;
        if !apply {
            return;
        }
        let snapshot = self.profile_tools.snapshots[self.profile_tools.snapshot_index].clone();
        if let Some(slot) = self.profile_tools.recovery_slot.clone() {
            let game = self.game;
            let name = self.profile_tools.recovered_name.clone();
            let tx = self.tx.clone();
            let ctx = context.clone();
            self.busy = true;
            self.profile_tools.running = true;
            self.profile_tools.recovery = false;
            thread::spawn(move || {
                let result = std::panic::catch_unwind(|| {
                    profile_files::recover_profile(&slot, &snapshot, &name, game)
                })
                .unwrap_or(Err(Error::InvalidSii("recovery copy failed")));
                let _ = tx.send(Task::Recovered(game, result));
                ctx.request_repaint();
            });
        } else if let Some(session) = &mut self.session {
            match session.restore_recovery(&snapshot) {
                Ok(_) => {
                    self.refresh();
                    self.status = "Recovery staged. Review the changes before saving.".into();
                    self.profile_tools.recovery = false;
                }
                Err(error) => self.error = Some(error.to_string()),
            }
        }
    }
    pub(super) fn profile_tools_dialog(&mut self, context: &egui::Context) {
        if !self.profile_tools.open {
            return;
        }
        let Some(session) = &self.session else {
            self.profile_tools.open = false;
            return;
        };
        let source = session
            .save_folder()
            .parent()
            .and_then(|path| path.parent())
            .map(PathBuf::from);
        let current_name = session.data.profile_name();
        let mut open = true;
        let mut apply = false;
        egui::Window::new(tr("Profile folders")).id(egui::Id::unique("Profile folders")).open(&mut open).collapsible(false).resizable(false).default_width(560.0)
            .show(context, |ui| {
                ui.heading(&current_name);
                design::caption(ui, tr("Close the game before changing profile folders."));
                ui.horizontal_wrapped(|ui| {
                    ui.radio_value(&mut self.profile_tools.rename, false, tr("Clone profile"));
                    ui.radio_value(&mut self.profile_tools.rename, true, tr("Rename profile"));
                });
                ui.label(tr("New profile name"));
                ui.add(egui::TextEdit::singleline(&mut self.profile_tools.name).char_limit(30).desired_width(460.0));
                let target = profile_files::folder_name(&self.profile_tools.name);
                let destination = source.as_ref().and_then(|path|path.parent()).zip(target.as_ref().ok()).map(|(root,name)|root.join(name));
                if let Some(path) = &destination { ui.small(format!("{} · {}", tr("Destination"), path.display())); }
                if self.profile_tools.rename {
                    ui.label(tr("The original profile is retained in a recovery folder. All saves, settings and unknown profile files are preserved."));
                } else {
                    ui.checkbox(&mut self.profile_tools.autosave_only, tr("Copy the autosave only"));
                    ui.label(tr("The original profile stays in place. Settings and album files are copied; previous tool recovery snapshots are excluded."));
                }
                let exists = destination.as_ref().is_some_and(|path|path.exists());
                if exists { ui.colored_label(Color32::from_rgb(172,55,43), tr("A profile with this name already exists.")); }
                apply = design::primary(ui, tr(if self.profile_tools.rename { "Rename profile" } else { "Clone profile" }),
                    target.is_ok() && !exists && !self.busy && !self.unsaved()).clicked();
            });
        self.profile_tools.open = open;
        if apply && let Some(source) = source {
            let operation = if self.profile_tools.rename {
                Operation::Rename
            } else {
                Operation::Clone {
                    autosave_only: self.profile_tools.autosave_only,
                }
            };
            self.begin_profile_copy(context, source, operation);
        }
    }
    fn begin_profile_copy(
        &mut self,
        context: &egui::Context,
        source: PathBuf,
        operation: Operation,
    ) {
        if self.busy || self.unsaved() {
            return;
        }
        let name = self.profile_tools.name.clone();
        let slot = self
            .session
            .as_ref()
            .and_then(|session| session.save_folder().file_name())
            .map(|name| PathBuf::from("save").join(name).join("game.sii"));
        let game = self.game;
        let sender = self.tx.clone();
        let context = context.clone();
        self.profile_tools.open = false;
        self.profile_tools.running = true;
        self.busy = true;
        self.status = "Copying and validating profile files…".into();
        thread::spawn(move || {
            let result = std::panic::catch_unwind(||profile_files::copy_profile(&source,&name,operation))
                .unwrap_or_else(|_|Err(Error::InvalidEdit("Profile operation failed. Check the profile recovery folder before continuing.".into())));
            let _ = sender.send(Task::ProfileCopied {
                source,
                game,
                operation,
                slot,
                result,
            });
            context.request_repaint();
        });
    }
    pub(super) fn finish_profile_copy(
        &mut self,
        context: &egui::Context,
        source: PathBuf,
        game: Game,
        operation: Operation,
        slot: Option<PathBuf>,
        result: Result<profile_files::Outcome>,
    ) {
        self.profile_tools.running = false;
        self.busy = false;
        match result {
            Ok(outcome) => {
                self.status = if operation == Operation::Rename {
                    format!(
                        "Profile renamed. Original recovery copy: {}",
                        outcome.backup.as_ref().unwrap().display()
                    )
                } else {
                    "Profile cloned. The original profile is unchanged.".into()
                };
                if operation == Operation::Rename {
                    self.switch_game(game);
                    if let Some(slot) = slot {
                        self.begin_load(context.clone(), outcome.path.join(slot), game);
                    }
                    if let Some(backup) = outcome.backup {
                        self.error = Some(format!(
                            "Profile renamed. The complete original is retained at {}",
                            backup.display()
                        ));
                    }
                }
                self.browser_refresh(context);
            }
            Err(error) => {
                if !source.is_dir() {
                    self.switch_game(game);
                }
                self.status =
                    "Profile operation did not finish. Review the message before continuing."
                        .into();
                self.error = Some(error.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_settings_checkboxes_can_be_toggled_by_keyboard_at_minimum_size() {
        use ts_save_core::settings::SettingsFile;
        let directory = tempfile::tempdir().unwrap();
        let slot = directory.path().join("save/1");
        std::fs::create_dir_all(&slot).unwrap();
        std::fs::write(
            directory.path().join("profile.sii"),
            "SiiNunit\n{\nuser_profile : profile.a {\n}\n}\n",
        )
        .unwrap();
        std::fs::write(
            slot.join("info.sii"),
            "SiiNunit\n{\nsave_container : info.a {\n version: 105\n}\n}\n",
        )
        .unwrap();
        std::fs::write(slot.join("game.sii"), "SiiNunit\n{\neconomy : economy.a {\n player: player.a\n bank: bank.a\n}\nplayer : player.a {\n}\nbank : bank.a {\n money_account: 100\n}\n}\n").unwrap();
        let ctx = egui::Context::default();
        let mut app = EditorApp::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        app.session = Some(SaveSession::load(slot.join("game.sii"), Game::Ets2).unwrap());
        app.profile_tools.settings = true;
        app.profile_tools.set_files(
            vec![SettingsFile {
                name: "controls.sii".into(),
                bytes: vec![42],
            }],
            "Loaded settings bundle",
        );
        let frame = |app: &mut EditorApp, key: Option<egui::Key>| {
            let events = key
                .into_iter()
                .flat_map(|key| {
                    [true, false].map(|pressed| egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    })
                })
                .collect();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1180.0, 720.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.settings_dialog(ui.ctx()),
            );
            output.textures_delta.clear();
            let focus = ctx
                .memory(|m| m.focused())
                .and_then(|id| ctx.read_response(id));
            output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(t)
                if t.galley.text().contains("controls.sii") && focus.as_ref().is_some_and(|r| r.rect.contains(t.pos + t.galley.rect.center().to_vec2()))))
        };
        frame(&mut app, None);
        frame(&mut app, None);
        assert!(
            (0..12).any(|_| frame(&mut app, Some(egui::Key::Tab))),
            "Tab should reach the settings checkbox"
        );
        frame(&mut app, Some(egui::Key::Space));
        assert_eq!(app.profile_tools.selected_files, [false]);
    }

    #[test]
    fn identity_picker_preserves_custom_choices_until_selection() {
        let context = egui::Context::default();
        let mut app = EditorApp::new(&eframe::CreationContext::_new_kittest(context.clone()));
        app.input.identity = Some(profile::Identity {
            face: 999,
            logo: "custom_logo".into(),
            male: true,
        });
        let original = app.input.identity.clone();
        app.profile_tools.identity = true;
        for game in [Game::Ets2, Game::Ats] {
            app.game = game;
            for _ in 0..3 {
                let mut output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(680.0, 600.0),
                        )),
                        ..Default::default()
                    },
                    |ui| app.identity_controls(ui),
                );
                output.textures_delta.clear();
            }
            assert_eq!(app.input.identity, original);
        }
    }
}
