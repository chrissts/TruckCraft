use super::*;

pub(super) struct StarterForm {
    game: Game,
    name: String,
    installation: Option<PathBuf>,
    game_home: Option<PathBuf>,
}

fn choose_folder(ui: &mut egui::Ui, label: &str, path: &mut Option<PathBuf>) {
    if ui.button(tr(label)).clicked()
        && let Some(selected) = rfd::FileDialog::new().set_title(tr(label)).pick_folder()
    {
        *path = Some(selected);
    }
    if let Some(path) = path {
        ui.add(egui::Label::new(path.display().to_string()).wrap());
    }
}

impl EditorApp {
    pub(super) fn starter_button(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(!self.busy, egui::Button::new(tr("Create new save…")))
            .clicked()
        {
            self.starter = Some(StarterForm {
                game: self.game,
                name: String::new(),
                installation: None,
                game_home: None,
            });
        }
    }

    pub(super) fn starter_dialog(&mut self, ctx: &egui::Context) {
        let Some(form) = self.starter.as_mut() else {
            return;
        };
        let mut open = true;
        let mut create = false;
        egui::Window::new(tr("Create new save"))
            .id(egui::Id::unique("create_new_save"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(500.0)
            .show(ctx, |ui| {
                ui.heading(form.game.folder());
                ui.label(tr("Create a fresh local profile from your installed game. Close the game before creating a save."));
                ui.label(tr("New profile name"));
                ui.add(egui::TextEdit::singleline(&mut form.name).char_limit(30).desired_width(280.0));
                choose_folder(ui, "Choose game installation…", &mut form.installation);
                choose_folder(ui, "Choose local game data folder…", &mut form.game_home);
                ui.label(tr("Load Fresh starter in the simulator, choose your headquarters city, and save again before editing in TruckCraft."));
                let ready = !self.busy
                    && form.installation.is_some()
                    && form.game_home.is_some()
                    && ts_save_core::profile_files::folder_name(&form.name).is_ok();
                create = design::primary(ui, tr("Create new save"), ready).clicked();
            });
        if create && let Some(form) = self.starter.take() {
            self.begin_starter(ctx, form);
        } else if !open {
            self.starter = None;
        }
    }

    fn begin_starter(&mut self, ctx: &egui::Context, form: StarterForm) {
        let (Some(installation), Some(game_home)) = (form.installation, form.game_home) else {
            return;
        };
        self.busy = true;
        self.status = "Creating starter save…".into();
        let sender = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = std::panic::catch_unwind(|| {
                ts_save_core::starter::create(form.game, &installation, &game_home, &form.name)
                    .map(|_| ())
            })
            .unwrap_or(Err(Error::InvalidSii("starter creation failed")));
            let _ = sender.send(Task::StarterCreated(result));
            ctx.request_repaint();
        });
    }
}
