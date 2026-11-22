use super::*;
use ts_save_core::{game_data::Progression, profile::XpCurve};

pub(super) struct GameData {
    pub plate_assets: ts_save_core::game_data::PlateAssets,
    pub curve: XpCurve,
    pub request: u64,
    pub loading: bool,
    pub installed: bool,
}

impl Default for GameData {
    fn default() -> Self {
        Self {
            plate_assets: Default::default(),
            curve: XpCurve::standard(Game::Ets2),
            request: 0,
            loading: false,
            installed: false,
        }
    }
}
impl GameData {
    pub fn reset(&mut self, game: Game) {
        self.plate_assets = Default::default();
        self.curve = XpCurve::standard(game);
        self.request = self.request.wrapping_add(1);
        self.loading = false;
        self.installed = false;
    }
    pub fn accept(&mut self, data: Progression) -> Result<()> {
        if data.truck_ranks != [6; 5]
            || data.adr_ranks != 6
            || data.coach_ranks.is_some_and(|ranks| ranks != [3; 6])
        {
            return Err(Error::InvalidDefinition("This installation uses unsupported skill ranks. The current XP table has been kept.".into()));
        }
        self.curve = data.truck_xp;
        self.plate_assets = data.plate_assets;
        self.installed = true;
        Ok(())
    }
}

impl EditorApp {
    pub(super) fn game_data_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(tr("Game data"));
            design::help(ui, tr("Game data"), tr("TruckCraft uses XP thresholds to calculate driver levels and presets. Loading a game folder also makes its number-plate badges available. Your saved XP stays as it is."));
        });
        design::caption(
            ui,
            tr(if self.game_data.installed {
                "Using data from the game folder"
            } else {
                "Using built-in level thresholds for version 1.62"
            }),
        );
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.game_data.loading && !self.busy,
                    egui::Button::new(tr("Load from game folder…")),
                )
                .clicked()
                && let Some(folder) = rfd::FileDialog::new()
                    .set_title(tr("Select the game installation containing def.scs"))
                    .pick_folder()
            {
                self.begin_game_data(ui.ctx(), folder);
            }
            if self.game_data.installed && ui.button(tr("Restore built-in game data")).clicked() {
                self.game_data.reset(self.game);
            }
            if self.game_data.loading {
                ui.spinner();
            }
        });
    }
    pub(super) fn begin_game_data(&mut self, context: &egui::Context, folder: PathBuf) {
        self.game_data.request = self.game_data.request.wrapping_add(1);
        self.game_data.loading = true;
        let request = self.game_data.request;
        let game = self.game;
        let context = context.clone();
        let sender = self.tx.clone();
        thread::spawn(move || {
            let result = std::panic::catch_unwind(|| Progression::load(game, &folder))
                .unwrap_or_else(|_| Err(Error::InvalidDefinition("Game data loading stopped unexpectedly. The current XP table has been kept.".into())));
            let _ = sender.send(Task::GameDataLoaded {
                game,
                request,
                result,
            });
            context.request_repaint();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom() -> Progression {
        Progression {
            plate_assets: Default::default(),
            truck_xp: XpCurve::from_steps(vec![100, 300]).unwrap(),
            coach_xp: None,
            truck_ranks: [6; 5],
            adr_ranks: 6,
            coach_ranks: None,
            definition_files: 2,
        }
    }

    #[test]
    fn loading_a_curve_preserves_pending_inputs_and_ignores_stale_results() {
        let context = egui::Context::default();
        let mut app = EditorApp::new(&eframe::CreationContext::_new_kittest(context.clone()));
        app.input.xp = 42;
        app.baseline = Some(app.input.clone());
        app.input.xp = 123_456;
        app.input.company = "Unapplied company".into();
        let request = app.game_data.request;
        app.tx
            .send(Task::GameDataLoaded {
                game: app.game,
                request,
                result: Ok(custom()),
            })
            .unwrap();
        app.poll(&context);
        assert!(app.game_data.installed);
        assert_eq!(app.input.xp, 123_456);
        assert_eq!(app.input.company, "Unapplied company");
        assert!(app.has_drafts());
        assert_eq!(app.game_data.curve.xp_for_level(1).unwrap(), 399);
        app.game_data.reset(Game::Ets2);
        app.tx
            .send(Task::GameDataLoaded {
                game: app.game,
                request,
                result: Ok(custom()),
            })
            .unwrap();
        app.poll(&context);
        assert!(!app.game_data.installed);
        assert_eq!(app.game_data.curve, XpCurve::standard(Game::Ets2));
        assert_eq!(app.input.xp, 123_456);
    }

    #[test]
    fn unsupported_rank_definitions_do_not_replace_the_active_curve() {
        let mut data = GameData::default();
        let old = data.curve.clone();
        let mut incoming = custom();
        incoming.truck_ranks[0] = 7;
        assert!(data.accept(incoming).is_err());
        assert_eq!(data.curve, old);
        assert!(!data.installed);
    }
}
