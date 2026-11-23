use super::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use ts_save_core::discovery::{self, Catalog};

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Preferences {
    logging_enabled: bool,
    #[serde(deserialize_with = "stored_update_checks")]
    check_updates_on_launch: Option<bool>,
    #[serde(deserialize_with = "stored_language")]
    language: String,
    last_game: Option<Game>,
    last_saves: Vec<(Game, PathBuf)>,
    roots: Vec<(Game, PathBuf)>,
    include_backups: bool,
}

fn stored_update_checks<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<bool>, D::Error> {
    Ok(serde_json::Value::deserialize(deserializer)?.as_bool())
}

fn stored_language<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<String, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(crate::i18n::normalize(value.as_str().unwrap_or("en")).into())
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            logging_enabled: true,
            check_updates_on_launch: None,
            language: "en".into(),
            last_game: None,
            last_saves: Vec::new(),
            roots: Vec::new(),
            include_backups: false,
        }
    }
}

pub(super) struct Browser {
    pub auto_load: bool,
    pub candidates: VecDeque<PathBuf>,
    pub missing_save: bool,
    pub catalog: Catalog,
    game: Option<Game>,
    pub request: u64,
    pub scanning: bool,
    profile: usize,
    save: usize,
    preferences: Preferences,
    summary: String,
}

fn preference_path() -> Option<PathBuf> {
    crate::activity::preference_path()
}
impl Browser {
    pub fn check_updates_on_launch(&self) -> Option<bool> {
        self.preferences.check_updates_on_launch
    }
    pub fn set_check_updates_on_launch(&mut self, enabled: bool, persist: bool) -> Result<()> {
        let old = self.preferences.check_updates_on_launch.replace(enabled);
        if persist && let Err(error) = self.persist() {
            self.preferences.check_updates_on_launch = old;
            return Err(error);
        }
        Ok(())
    }
    pub fn language(&self) -> &str {
        &self.preferences.language
    }
    pub fn set_language(&mut self, code: &str, persist: bool) -> Result<()> {
        let old = std::mem::replace(
            &mut self.preferences.language,
            crate::i18n::normalize(code).into(),
        );
        if persist && let Err(error) = self.persist() {
            self.preferences.language = old;
            return Err(error);
        }
        Ok(())
    }
    pub fn logging_enabled(&self) -> bool {
        self.preferences.logging_enabled
    }
    pub fn set_logging(&mut self, enabled: bool, persist: bool) -> Result<()> {
        let old = self.preferences.logging_enabled;
        self.preferences.logging_enabled = enabled;
        if persist && let Err(error) = self.persist() {
            self.preferences.logging_enabled = old;
            return Err(error);
        }
        Ok(())
    }
    pub fn new(isolated: bool) -> Self {
        let preferences = (!isolated)
            .then(preference_path)
            .flatten()
            .and_then(|path| discovery::read_small(&path, 1024 * 1024).ok())
            .and_then(|bytes| serde_json::from_slice::<Preferences>(&bytes).ok())
            .unwrap_or_default();
        Self {
            auto_load: false,
            candidates: VecDeque::new(),
            missing_save: false,
            catalog: Catalog::default(),
            game: None,
            request: 0,
            scanning: false,
            profile: 0,
            save: 0,
            preferences,
            summary: String::new(),
        }
    }
    fn persist(&self) -> Result<()> {
        let path =
            preference_path().ok_or_else(|| Error::Missing("local settings folder".into()))?;
        self.persist_to(&path)
    }
    fn persist_to(&self, path: &std::path::Path) -> Result<()> {
        use std::io::Write;
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent)?;
        let bytes = serde_json::to_vec_pretty(&self.preferences)
            .map_err(|_| Error::InvalidSii("could not encode preferences"))?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(path)
            .map_err(|error| Error::Io(error.error))?;
        Ok(())
    }
    pub fn finish(&mut self, catalog: Catalog) {
        self.summary = format!(
            "{} profile(s) found{}{}",
            catalog.profiles.len(),
            if catalog.unreadable > 0 {
                " · some metadata could not be read"
            } else {
                ""
            },
            if catalog.truncated {
                " · search limit reached; add a more specific folder"
            } else {
                ""
            }
        );
        self.catalog = catalog;
        self.profile = self
            .profile
            .min(self.catalog.profiles.len().saturating_sub(1));
        self.save = 0;
        self.scanning = false;
    }
    pub fn last_game(&self) -> Option<Game> {
        self.preferences.last_game
    }
    pub fn invalidate_game(&mut self) {
        self.game = None;
        self.catalog = Catalog::default();
        self.request = self.request.wrapping_add(1);
        self.scanning = false;
        self.auto_load = false;
        self.candidates.clear();
        self.missing_save = false;
        self.profile = 0;
        self.save = 0;
    }
    pub fn remember_loaded(&mut self, game: Game, path: PathBuf, persist: bool) -> Result<()> {
        if let Some((profile, save)) =
            self.catalog
                .profiles
                .iter()
                .enumerate()
                .find_map(|(index, profile)| {
                    profile
                        .saves
                        .iter()
                        .position(|save| save.path == path)
                        .map(|save| (index, save))
                })
        {
            self.profile = profile;
            self.save = save;
        }
        self.preferences
            .last_saves
            .retain(|(existing, _)| *existing != game);
        self.preferences.last_saves.push((game, path));
        self.preferences.last_game = Some(game);
        self.missing_save = false;
        if persist {
            self.persist()?;
        }
        Ok(())
    }
    pub fn automatic_candidates(&self, game: Game) -> VecDeque<PathBuf> {
        let remembered = self
            .preferences
            .last_saves
            .iter()
            .find(|(saved_game, _)| *saved_game == game)
            .map(|(_, path)| path);
        let mut discovered: Vec<_> = self
            .catalog
            .profiles
            .iter()
            .filter(|profile| {
                !profile.path.components().any(|part| {
                    part.as_os_str()
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .ends_with(".bak")
                })
            })
            .flat_map(|profile| &profile.saves)
            .filter(|save| Some(&save.path) != remembered)
            .collect();
        discovered.sort_unstable_by(|a, b| {
            b.modified
                .cmp(&a.modified)
                .then_with(|| a.path.cmp(&b.path))
        });
        remembered
            .into_iter()
            .chain(discovered.into_iter().map(|save| &save.path))
            .cloned()
            .collect()
    }
}

impl EditorApp {
    pub(super) fn browser_refresh(&mut self, context: &egui::Context) {
        self.begin_discovery(context);
    }
    pub(super) fn discover_if_needed(&mut self, context: &egui::Context) {
        if !self.isolated && self.browser.game != Some(self.game) {
            self.begin_discovery(context);
        }
    }
    fn begin_discovery(&mut self, context: &egui::Context) {
        self.activity.record(
            "INFO",
            &format!("Discovering {} profiles and saves", self.game.name()),
        );
        if self.isolated {
            return;
        }
        if self.browser.game != Some(self.game) {
            self.browser.catalog = Catalog::default();
        }
        self.browser.game = Some(self.game);
        self.browser.auto_load = self.session.is_none() && !self.busy;
        self.browser.missing_save = false;
        self.browser.request = self.browser.request.wrapping_add(1);
        self.browser.scanning = true;
        let request = self.browser.request;
        let game = self.game;
        let include_backups = self.browser.preferences.include_backups;
        let mut roots = self
            .browser
            .preferences
            .roots
            .iter()
            .filter(|(root_game, _)| *root_game == game)
            .map(|(_, path)| path.clone())
            .collect::<Vec<_>>();
        if let Some((_, path)) = self
            .browser
            .preferences
            .last_saves
            .iter()
            .find(|(saved_game, _)| *saved_game == game)
            && let Some(profile) = path
                .parent()
                .and_then(|save| save.parent())
                .and_then(|saves| saves.parent())
        {
            roots.push(profile.to_path_buf());
        }
        let sender = self.tx.clone();
        let context = context.clone();
        thread::spawn(move || {
            let catalog =
                std::panic::catch_unwind(|| discovery::discover(game, &roots, include_backups))
                    .unwrap_or_else(|_| Catalog {
                        unreadable: 1,
                        ..Default::default()
                    });
            let _ = sender.send(Task::Discovered {
                game,
                request,
                catalog,
            });
            context.request_repaint();
        });
    }
    pub(super) fn save_browser(&mut self, ui: &mut egui::Ui) {
        ui.label(tr("If Steam Cloud sync is enabled, it may overwrite edits or cause sync conflicts. Close the game and let sync finish before editing. Editing is still allowed."));
        if self.isolated {
            design::caption(
                ui,
                tr("Automatic profile discovery is off. Open a save file to choose a profile."),
            );
            if ui
                .add_enabled(!self.busy, egui::Button::new(tr("Browse for a save file…")))
                .clicked()
            {
                self.open_dialog(ui.ctx());
            }
            return;
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!self.browser.scanning, egui::Button::new(tr("Refresh")))
                .clicked()
            {
                self.begin_discovery(ui.ctx());
            }
            if ui.button(tr("Add folder…")).clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_title(tr("Select a game home, profiles folder or profile"))
                    .pick_folder()
            {
                if !self
                    .browser
                    .preferences
                    .roots
                    .iter()
                    .any(|(game, existing)| *game == self.game && existing == &path)
                {
                    self.browser.preferences.roots.push((self.game, path));
                    if let Err(error) = self.browser.persist() {
                        self.error = Some(error.to_string());
                    }
                }
                self.begin_discovery(ui.ctx());
            }
        });
        if ui
            .checkbox(
                &mut self.browser.preferences.include_backups,
                tr("Include game backup profiles"),
            )
            .changed()
        {
            if let Err(error) = self.browser.persist() {
                self.error = Some(error.to_string());
            }
            self.begin_discovery(ui.ctx());
        }
        if self.browser.scanning {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(tr("Finding profiles…"));
            });
        } else {
            ui.small(&self.browser.summary);
        }
        if !self.browser.catalog.profiles.is_empty() {
            ui.add_space(6.0);
            design::caption(ui, tr("PROFILE"));
            let mut selected = self.browser.profile;
            design::combo(ui, "discovered_profile", 260.0)
                .selected_text(
                    self.browser
                        .catalog
                        .profiles
                        .get(selected)
                        .map_or("Choose profile", |profile| profile.name.as_str()),
                )
                .show_ui(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    for (index, profile) in self.browser.catalog.profiles.iter().enumerate() {
                        ui.selectable_value(
                            &mut selected,
                            index,
                            format!("{} ({})", profile.name, profile.source),
                        );
                    }
                });
            if selected != self.browser.profile {
                self.browser.profile = selected;
                self.browser.save = 0;
            }
            if let Some(profile) = self.browser.catalog.profiles.get(self.browser.profile) {
                ui.horizontal(|ui| {
                    if let Some(portrait)=self.artwork.game.portrait(ui.ctx(), self.game, &profile.avatar) {
                        ui.image((portrait.id(),egui::vec2(48.0,48.0)));
                    } else {
                    let initial = profile
                        .name
                        .chars()
                        .find(|character| !character.is_whitespace())
                        .unwrap_or('?')
                        .to_uppercase()
                        .to_string();
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 10, design::TINT);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        initial,
                        egui::FontId::proportional(25.0),
                        design::ACCENT,
                    );
                    response.on_hover_text(if profile.avatar.is_empty() {
                        "No avatar is set for this profile."
                    } else {
                        "This portrait is unavailable in the bundled artwork; showing profile initials."
                    });
                    }
                    ui.vertical(|ui| {
                        ui.strong(&profile.name);
                        design::caption(ui, &profile.source);
                    });
                });
                design::caption(ui, tr("SAVE"));
                design::combo(ui, "discovered_save", 260.0)
                    .selected_text(
                        profile
                            .saves
                            .get(self.browser.save)
                            .map_or("No saves found", |save| save.name.as_str()),
                    )
                    .show_ui(ui, |ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                        for (index, save) in profile.saves.iter().enumerate() {
                            ui.selectable_value(&mut self.browser.save, index, &save.name);
                        }
                    });
                let path = profile
                    .saves
                    .get(self.browser.save)
                    .map(|save| save.path.clone());
                if ui
                    .add_enabled(
                        !self.busy && path.is_some(),
                        egui::Button::new(tr("Load selected save"))
                            .min_size([ui.available_width().min(260.0), 40.0].into()),
                    )
                    .clicked()
                    && let Some(path) = path
                {
                    if self.unsaved() {
                        self.pending = Some(Pending::Open(path, self.game));
                    } else {
                        self.begin_load(ui.ctx().clone(), path, self.game);
                    }
                }
            }
        }
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new(tr("Browse for a save file…"))
                    .min_size([ui.available_width().min(260.0), 36.0].into()),
            )
            .clicked()
        {
            self.open_dialog(ui.ctx());
        }
    }

    pub(super) fn missing_save_dialog(&mut self, context: &egui::Context) {
        if !self.browser.missing_save
            || self.busy
            || self.session.is_some()
            || self.pending.is_some()
            || self.error.is_some()
            || self.starter.is_some()
        {
            return;
        }
        let mut open = true;
        egui::Window::new(tr("Choose a save or profile")).id(egui::Id::unique("Choose a save or profile")).open(&mut open).collapsible(false).resizable(false).default_width(500.0).show(context,|ui| {
            ui.label(tr("No save could be loaded automatically. Choose a save file or profile folder to continue."));
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr("Choose save file…")).clicked() {self.open_dialog(context);}
                if ui.button(tr("Choose profile folder…")).clicked() && let Some(path)=rfd::FileDialog::new().set_title(tr("Select a game home, profiles folder or profile")).pick_folder() {
                    if !self.browser.preferences.roots.iter().any(|(game,root)|*game==self.game&&root==&path) {self.browser.preferences.roots.push((self.game,path));}
                    if let Err(error)=self.browser.persist() {self.error=Some(error.to_string());}
                    self.begin_discovery(context);
                }
                if ui.button(tr("Recover from backup…")).clicked() { self.pick_recovery_slot(); }
                self.starter_button(ui);
                if ui.button(tr("Later")).clicked() {self.browser.missing_save=false;}
            });
        });
        if !open {
            self.browser.missing_save = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_check_consent_is_explicit_and_keeps_other_preferences() {
        for input in [
            serde_json::json!({"last_game":"ATS"}),
            serde_json::json!({"last_game":"ATS","check_updates_on_launch":"true"}),
        ] {
            let preferences: Preferences = serde_json::from_value(input).unwrap();
            assert_eq!(preferences.check_updates_on_launch, None);
            assert_eq!(preferences.last_game, Some(Game::Ats));
        }
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("settings.json");
        let mut browser = Browser::new(true);
        assert_eq!(browser.check_updates_on_launch(), None);
        for enabled in [false, true] {
            browser.set_check_updates_on_launch(enabled, false).unwrap();
            browser.persist_to(&path).unwrap();
            let saved: Preferences =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(saved.check_updates_on_launch, Some(enabled));
        }
    }

    #[test]
    fn logging_preference_persists_and_invalid_languages_keep_other_preferences() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("settings.json");
        let mut browser = Browser::new(true);
        assert!(browser.logging_enabled());
        browser.set_logging(false, false).unwrap();
        browser.preferences.last_game = Some(Game::Ats);
        browser.persist_to(&path).unwrap();
        let saved: Preferences = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(!saved.logging_enabled);
        assert_eq!(saved.language, "en");
        assert_eq!(saved.last_game, Some(Game::Ats));
        for language in [
            serde_json::json!("unsupported"),
            serde_json::json!(42),
            serde_json::Value::Null,
        ] {
            let value =
                serde_json::json!({"logging_enabled":false,"last_game":"ATS","language":language});
            let preferences: Preferences = serde_json::from_value(value).unwrap();
            assert_eq!(preferences.language, "en");
            assert!(!preferences.logging_enabled);
            assert_eq!(preferences.last_game, Some(Game::Ats));
        }
        let old: Preferences = serde_json::from_str("{\"last_game\":\"ETS2\"}").unwrap();
        assert!(old.logging_enabled);
        assert_eq!(old.language, "en");
    }

    #[test]
    fn i18n_bundled_language_selection_survives_settings_reload() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("settings.json");
        let mut browser = Browser::new(true);
        for (&code, language) in crate::i18n::languages() {
            browser.set_language(code, false).unwrap();
            browser.persist_to(&path).unwrap();
            let saved: Preferences =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(saved.language, code);
            crate::i18n::select(&saved.language);
            for (source, message) in &language.messages {
                assert_eq!(tr(source), message);
            }
            assert_eq!(tr("Untranslated example"), "Untranslated example");
        }
        crate::i18n::select("en");
    }
    use std::time::{Duration, UNIX_EPOCH};
    use ts_save_core::discovery::{Profile, SaveEntry};

    fn profile(path: &str, saves: &[(&str, u64)]) -> Profile {
        Profile {
            path: path.into(),
            name: "Invented profile".into(),
            source: "Test".into(),
            avatar: String::new(),
            saves: saves
                .iter()
                .map(|(name, time)| SaveEntry {
                    path: PathBuf::from(path).join("save").join(name).join("game.sii"),
                    name: (*name).into(),
                    modified: Some(UNIX_EPOCH + Duration::from_secs(*time)),
                })
                .collect(),
        }
    }

    #[test]
    fn remembers_each_game_and_falls_back_to_newest_non_backup_save() {
        let mut browser = Browser::new(true);
        let ets = profile("ets/profiles/sample", &[("1", 1), ("autosave", 2)]);
        let ats = profile("ats/profiles/sample", &[("1", 3)]);
        let backup = profile("ets/profiles(1.62).bak/sample", &[("1", 10)]);
        browser.finish(Catalog {
            profiles: vec![ets.clone(), backup],
            ..Default::default()
        });
        assert_eq!(
            browser.automatic_candidates(Game::Ets2),
            [ets.saves[1].path.clone(), ets.saves[0].path.clone()]
        );
        browser
            .remember_loaded(Game::Ets2, ets.saves[0].path.clone(), false)
            .unwrap();
        assert_eq!(
            browser.automatic_candidates(Game::Ets2),
            [ets.saves[0].path.clone(), ets.saves[1].path.clone()]
        );
        browser.invalidate_game();
        assert!(browser.catalog.profiles.is_empty());
        browser.finish(Catalog {
            profiles: vec![ats.clone()],
            ..Default::default()
        });
        assert_eq!(
            browser.automatic_candidates(Game::Ats),
            [ats.saves[0].path.clone()]
        );
        browser
            .remember_loaded(Game::Ats, ats.saves[0].path.clone(), false)
            .unwrap();
        browser.finish(Catalog {
            profiles: vec![ets.clone()],
            ..Default::default()
        });
        assert_eq!(
            browser.automatic_candidates(Game::Ets2),
            [ets.saves[0].path.clone(), ets.saves[1].path.clone()]
        );
        assert_eq!(browser.preferences.last_saves.len(), 2);
        assert_eq!(browser.last_game(), Some(Game::Ats));
    }

    #[test]
    fn no_discovered_save_prompts_and_stale_other_game_results_are_ignored() {
        let context = egui::Context::default();
        let mut app = EditorApp::new(&eframe::CreationContext::_new_kittest(context.clone()));
        app.isolated = true;
        app.browser = Browser::new(true);
        app.browser.auto_load = true;
        let request = app.browser.request;
        app.tx
            .send(Task::Discovered {
                game: app.game,
                request,
                catalog: Catalog::default(),
            })
            .unwrap();
        app.poll(&context);
        assert!(app.browser.missing_save);
        assert!(app.session.is_none());
        app.switch_game(Game::Ats);
        app.tx
            .send(Task::Discovered {
                game: Game::Ets2,
                request,
                catalog: Catalog {
                    profiles: vec![profile("old/profile", &[("1", 1)])],
                    ..Default::default()
                },
            })
            .unwrap();
        app.poll(&context);
        assert!(app.browser.catalog.profiles.is_empty());
        assert_eq!(app.game, Game::Ats);
    }

    #[test]
    fn automatic_discovery_never_replaces_a_session_with_unapplied_inputs() {
        let context = egui::Context::default();
        let mut app = EditorApp::new(&eframe::CreationContext::_new_kittest(context.clone()));
        app.isolated = true;
        app.browser = Browser::new(true);
        app.baseline = Some(app.input.clone());
        app.input.company = "Pending company edit".into();
        app.browser.auto_load = true;
        app.tx
            .send(Task::Discovered {
                game: app.game,
                request: app.browser.request,
                catalog: Catalog {
                    profiles: vec![profile("new/profile", &[("1", 1)])],
                    ..Default::default()
                },
            })
            .unwrap();
        app.poll(&context);
        assert!(!app.busy);
        assert_eq!(app.input.company, "Pending company edit");
        assert!(app.has_drafts());
    }

    #[test]
    fn automatically_loads_and_switches_separate_synthetic_saves_without_writing() {
        use std::{fs, time::Duration};
        let root = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app = EditorApp::new(&eframe::CreationContext::_new_kittest(context.clone()));
        app.isolated = true;
        app.browser = Browser::new(true);
        let game_text = "SiiNunit\n{\neconomy : economy.test {\n player: player.test\n bank: bank.test\n experience_points: 42\n}\nplayer : player.test {\n hq_city: \"sample\"\n}\nbank : bank.test {\n money_account: 100\n}\n}\n";
        let mut profiles = Vec::new();
        for (game, name) in [(Game::Ets2, "ETS sample"), (Game::Ats, "ATS sample")] {
            let folder = root.path().join(game.name()).join("profile");
            fs::create_dir_all(folder.join("save/1")).unwrap();
            fs::write(folder.join("profile.sii"),format!("SiiNunit\n{{\nuser_profile : profile.test {{\n profile_name: \"{name}\"\n}}\n}}\n")).unwrap();
            fs::write(folder.join("save/1/info.sii"),"SiiNunit\n{\nsave_container : info.test {\n version: 105\n name: \"Sample save\"\n}\n}\n").unwrap();
            fs::write(folder.join("save/1/game.sii"), game_text).unwrap();
            profiles.push(
                discovery::discover_roots(&[(folder, "Test".into())], false)
                    .profiles
                    .remove(0),
            );
        }
        for (game, index, name) in [
            (Game::Ets2, 0, "ETS sample"),
            (Game::Ats, 1, "ATS sample"),
            (Game::Ets2, 0, "ETS sample"),
        ] {
            app.switch_game(game);
            app.browser.auto_load = true;
            app.tx
                .send(Task::Discovered {
                    game,
                    request: app.browser.request,
                    catalog: Catalog {
                        profiles: vec![profiles[index].clone()],
                        ..Default::default()
                    },
                })
                .unwrap();
            app.poll(&context);
            if app.session.is_none() {
                let task = app.rx.recv_timeout(Duration::from_secs(10)).unwrap();
                assert!(matches!(task, Task::Loaded { .. }));
                app.tx.send(task).unwrap();
                app.poll(&context);
            }
            let session = app.session.as_ref().expect("automatic load completed");
            assert_eq!(session.data.profile_name(), name);
            assert_eq!(session.data.simulator, game);
            assert!(!session.data.is_dirty());
            assert_eq!(
                app.game_data.curve,
                ts_save_core::profile::XpCurve::standard(game)
            );
            assert_eq!(
                fs::read_to_string(session.save_folder().join("game.sii")).unwrap(),
                game_text
            );
        }
    }

    #[test]
    fn automatic_load_skips_invalid_saves_and_a_new_session_resets_save_inputs() {
        use std::fs;
        let root = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app = EditorApp::new(&eframe::CreationContext::_new_kittest(context.clone()));
        app.isolated = true;
        app.browser = Browser::new(true);
        let profile = profile(
            root.path().to_str().unwrap(),
            &[("older", 1), ("newest", 2)],
        );
        let valid = &profile.saves[0].path;
        let invalid = &profile.saves[1].path;
        let missing = root.path().join("save/missing/game.sii");
        fs::write(
            root.path().join("profile.sii"),
            "SiiNunit\n{\nuser_profile : profile.test {\n profile_name: \"Sample\"\n}\n}\n",
        )
        .unwrap();
        for path in [valid, invalid] {
            let folder = path.parent().unwrap();
            fs::create_dir_all(folder).unwrap();
            fs::write(
                folder.join("info.sii"),
                "SiiNunit\n{\nsave_container : info.test {\n version: 105\n}\n}\n",
            )
            .unwrap();
        }
        fs::write(invalid, "unreadable save").unwrap();
        fs::write(valid, "SiiNunit\n{\neconomy : economy.test {\n player: player.test\n bank: bank.test\n}\nplayer : player.test {\n}\nbank : bank.test {\n money_account: 100\n}\n}\n").unwrap();
        app.browser
            .remember_loaded(app.game, missing, false)
            .unwrap();
        app.browser.auto_load = true;
        app.tx
            .send(Task::Discovered {
                game: app.game,
                request: app.browser.request,
                catalog: Catalog {
                    profiles: vec![profile.clone()],
                    ..Default::default()
                },
            })
            .unwrap();
        app.poll(&context);
        while app.busy {
            let task = app.rx.recv_timeout(Duration::from_secs(10)).unwrap();
            app.tx.send(task).unwrap();
            app.poll(&context);
        }
        assert_eq!(
            app.session.as_ref().unwrap().save_folder(),
            valid.parent().unwrap()
        );
        assert!(app.error.is_none());
        assert!(!app.browser.missing_save);

        app.input.pack_json = "old convoy pack".into();
        app.input.cargo_seeds = "old seeds".into();
        app.begin_load(context.clone(), valid.clone(), app.game);
        let task = app.rx.recv_timeout(Duration::from_secs(10)).unwrap();
        app.tx.send(task).unwrap();
        app.poll(&context);
        assert!(app.input.pack_json.is_empty());
        assert!(app.input.cargo_seeds.is_empty());
        assert!(!app.has_drafts());
    }
}
