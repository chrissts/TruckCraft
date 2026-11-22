use crate::artwork::Artwork;
use crate::i18n::tr;
use crate::{APP_NAME, display_version};
use eframe::egui::{self, Color32, RichText};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};
use ts_save_core::{
    Error, Result, cargo, coaches, convoy,
    editor::{self, Entity, VehicleKind},
    profile,
    session::{Game, SAVE_FORMAT_TARGET, SaveData, SaveOutcome, SaveSession},
    value,
};
mod browser;
mod car_ui;
mod company_ui;
mod design;
mod freight_ui;
mod game_data_ui;
pub(crate) mod package_check;
mod plate_ui;
mod profile_ui;
mod review;
mod settings_ui;
mod shell;
mod skills_ui;
mod starter_ui;
mod updates;
mod vehicle_ui;
const GITHUB: &str = "https://github.com/chrissts/TruckCraft";
enum Task {
    ReleaseChecked(updates::ReleaseCheck),
    #[cfg(windows)]
    UpdateDownloaded(std::io::Result<crate::windows_update::Prepared>),
    Recovered(Game, Result<PathBuf>),
    StarterCreated(Result<()>),
    GameDataLoaded {
        game: Game,
        request: u64,
        result: Result<ts_save_core::game_data::Progression>,
    },
    ProfileCopied {
        source: PathBuf,
        game: Game,
        operation: ts_save_core::profile_files::Operation,
        slot: Option<PathBuf>,
        result: Result<ts_save_core::profile_files::Outcome>,
    },
    Loaded {
        automatic: bool,
        result: Result<SaveSession>,
    },
    Saved {
        session: SaveSession,
        result: Result<SaveOutcome>,
    },
    Discovered {
        game: Game,
        request: u64,
        catalog: ts_save_core::discovery::Catalog,
    },
}
enum Pending {
    Open(PathBuf, Game),
    SwitchGame(Game),
    Close,
}
#[derive(Default, Clone)]
struct Inputs {
    xp: u32,
    adr: u8,
    skills: [u8; 5],
    money: i64,
    money_entry: String,
    company: String,
    identity: Option<profile::Identity>,
    hq: String,
    coach_xp: u32,
    coach_skills: [u8; 6],
    satisfaction: f32,
    extra_minutes: u32,
    fuel: f32,
    plate: String,
    permanent: bool,
    condition: Vec<ts_save_core::vehicle::Wear>,
    paint: Vec<ts_save_core::vehicle::PaintColor>,
    parts: Vec<ts_save_core::vehicle::Part>,
    vehicle_index: usize,
    cargo_company: usize,
    cargo_filter: String,
    cargo_seeds: String,
    position_name: String,
    pack_json: String,
    position_index: usize,
    colors: Vec<u32>,
    color_share: String,
    color_selected: Vec<bool>,
    offer_index: usize,
    offer_values: [u32; 5],
}
impl Inputs {
    fn default_for_save() -> Self {
        Self {
            satisfaction: 100.0,
            extra_minutes: 60,
            position_name: "Convoy position".into(),
            ..Self::default()
        }
    }
    fn vehicle_draft(&self, base: &Self) -> bool {
        self.fuel != base.fuel
            || self.plate != base.plate
            || self.condition != base.condition
            || self.paint != base.paint
            || self.parts != base.parts
    }
    fn has_drafts(&self, base: &Self) -> bool {
        !self.draft_tabs(base, 2).is_empty()
    }

    fn draft_tabs(&self, base: &Self, vehicle_tab: usize) -> Vec<usize> {
        let mut tabs = Vec::new();
        if self.xp != base.xp
            || self.adr != base.adr
            || self.skills != base.skills
            || self.identity != base.identity
            || self.colors != base.colors
        {
            tabs.push(0);
        }
        if self.money != base.money
            || self.money_entry != base.money_entry
            || self.company != base.company
            || self.hq != base.hq
        {
            tabs.push(1);
        }
        if self.coach_xp != base.coach_xp
            || self.coach_skills != base.coach_skills
            || self.offer_values != base.offer_values
        {
            tabs.push(3);
        }
        if self.vehicle_draft(base) {
            tabs.push(vehicle_tab);
        }
        if self.cargo_seeds != base.cargo_seeds {
            tabs.push(6);
        }
        tabs
    }
    fn retain_drafts(&mut self, previous: &Self, base: &Self) {
        macro_rules! keep {($($field:ident),*)=>{$(if previous.$field!=base.$field{self.$field=previous.$field.clone();})*};}
        keep!(
            identity,
            condition,
            paint,
            parts,
            xp,
            adr,
            skills,
            money,
            money_entry,
            company,
            hq,
            coach_xp,
            coach_skills,
            fuel,
            plate,
            cargo_seeds,
            colors,
            offer_values
        );
    }
}
pub struct EditorApp {
    activity: settings_ui::SettingsUi,
    vehicle_tools: vehicle_ui::VehicleTools,
    colour_sharing: bool,
    plate_tools: plate_ui::PlateTools,
    game_data: game_data_ui::GameData,
    freight: freight_ui::Freight,
    profile_tools: profile_ui::ProfileTools,
    starter: Option<starter_ui::StarterForm>,
    isolated: bool,
    session: Option<SaveSession>,
    game: Game,
    tab: usize,
    status: String,
    error: Option<String>,
    busy: bool,
    saving: bool,
    tx: Sender<Task>,
    rx: Receiver<Task>,
    pending: Option<Pending>,
    allow_close: bool,
    about: bool,
    checking_release: Option<updates::CheckOrigin>,
    release_result: Option<updates::ReleaseCheck>,
    #[cfg(windows)]
    update_state: updates::UpdateState,
    #[cfg(windows)]
    update_confirmation: Option<crate::windows_update::Startup>,
    review_open: bool,
    failed_slot: Option<PathBuf>,
    input: Inputs,
    fleet: Vec<Entity>,
    companies: Vec<String>,
    hq_cities: Vec<String>,
    coach_state: Option<coaches::CoachState>,
    pack: Option<convoy::Pack>,
    cargo_parameters: cargo::Parameters,
    artwork: Artwork,
    baseline: Option<Inputs>,
    offers: Vec<Entity>,
    browser: browser::Browser,
    management: company_ui::Management,
}

impl EditorApp {
    pub fn new(context: &eframe::CreationContext<'_>) -> Self {
        design::configure(&context.egui_ctx);
        let isolated = crate::isolated_launch();
        let browser = browser::Browser::new(isolated);
        let activity = settings_ui::SettingsUi::new(isolated, browser.logging_enabled());
        let (tx, rx) = mpsc::channel();
        let mut app = Self {
            activity,
            vehicle_tools: Default::default(),
            colour_sharing: false,
            plate_tools: Default::default(),
            game_data: Default::default(),
            freight: Default::default(),
            profile_tools: Default::default(),
            starter: None,
            isolated,
            session: None,
            game: Game::Ets2,
            tab: 0,
            status: "Open a game.sii save to begin.".into(),
            error: None,
            busy: false,
            saving: false,
            tx,
            rx,
            pending: None,
            allow_close: false,
            about: false,
            checking_release: None,
            release_result: None,
            #[cfg(windows)]
            update_state: updates::UpdateState::Idle,
            #[cfg(windows)]
            update_confirmation: None,
            review_open: false,
            failed_slot: None,
            input: Inputs::default_for_save(),
            fleet: Vec::new(),
            companies: Vec::new(),
            hq_cities: Vec::new(),
            coach_state: None,
            pack: None,
            cargo_parameters: cargo::Parameters {
                count: 10,
                valid_minutes: 180,
                spacing_minutes: 60,
            },
            artwork: Artwork::load(&context.egui_ctx),
            baseline: None,
            offers: Vec::new(),
            browser,
            management: company_ui::Management::default(),
        };
        let mut path = None;
        if !isolated && let Some(game) = app.browser.last_game() {
            app.game = game;
        }
        let mut args = std::env::args_os().skip(1);
        while let Some(arg) = args.next() {
            if arg == "--ats" {
                app.game = Game::Ats;
            } else if arg == "--open" {
                path = args.next().map(PathBuf::from);
            }
        }
        app.game_data.reset(app.game);
        if let Some(path) = path {
            app.begin_load(context.egui_ctx.clone(), path, app.game);
        }
        app
    }
    fn has_drafts(&self) -> bool {
        self.freight.has_drafts()
            || self.management.has_drafts()
            || self
                .baseline
                .as_ref()
                .is_some_and(|base| self.input.has_drafts(base))
    }
    fn vehicle_draft(&self) -> bool {
        self.baseline
            .as_ref()
            .is_some_and(|base| self.input.vehicle_draft(base))
    }
    fn unsaved(&self) -> bool {
        self.has_drafts()
            || self
                .session
                .as_ref()
                .is_some_and(|session| session.is_dirty())
    }
    fn edit(&mut self, operation: impl FnOnce(&mut SaveData) -> Result<()>) {
        self.edit_named("Edit save", operation);
    }
    fn edit_named(
        &mut self,
        description: &str,
        operation: impl FnOnce(&mut SaveData) -> Result<()>,
    ) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        match session.edit(operation) {
            Ok(()) => {
                let label = self.artwork.tabs(self.game)[self.tab].0;
                self.activity
                    .record("EDIT", &format!("{label}: {description} staged"));
                self.status =
                    "Changes staged. Save writes them with a complete recovery snapshot.".into();
                self.refresh();
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
    fn refresh(&mut self) {
        let previous = self.input.clone();
        let baseline = self.baseline.clone();
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let data = &session.data;
        if let Ok(economy) = data.economy_id() {
            self.input.xp = data.game.integer(economy, "experience_points").unwrap_or(0);
            self.input.adr = data.game.integer(economy, "adr").unwrap_or(0);
            for (index, field) in editor::TRUCK_SKILLS.iter().enumerate() {
                self.input.skills[index] = data.game.integer(economy, field).unwrap_or(0);
            }
            self.input.coach_xp = data
                .game
                .integer(economy, "bus_experience_points")
                .unwrap_or(0);
            for (index, field) in coaches::SKILLS.iter().enumerate() {
                self.input.coach_skills[index] = data.game.integer(economy, field).unwrap_or(0);
            }
        }
        if let Ok(bank) = data.bank_id() {
            self.input.money = data.game.integer(bank, "money_account").unwrap_or(0);
            self.input.money_entry = balance_text(self.input.money);
        }
        if let Ok(profile) = data.profile_id() {
            self.input.company = data
                .profile
                .string(profile, "company_name")
                .unwrap_or_default();
        }
        self.input.identity = profile::identity(data).ok();
        if let Ok(player) = data.player_id() {
            self.input.hq = data.game.string(player, "hq_city").unwrap_or_default();
        }
        self.hq_cities = data
            .game
            .units_of_kind("garage")
            .filter(|unit| {
                data.game
                    .integer::<u32>(&unit.id, "status")
                    .is_ok_and(|status| status != 0)
            })
            .filter_map(|unit| unit.id.strip_prefix("garage.").map(str::to_owned))
            .collect();
        self.hq_cities.sort_unstable();
        self.companies = cargo::companies(data);
        self.coach_state = coaches::detect(data).ok();
        self.input.colors = profile::user_colors(data).unwrap_or_default();
        self.offers = data
            .game
            .units_of_kind("line_offer_data")
            .map(|unit| {
                let source = data
                    .game
                    .value(&unit.id, "source")
                    .unwrap_or("Unknown stop");
                let route = data
                    .game
                    .array_or_empty(&unit.id, "saved_targets")
                    .unwrap_or_default()
                    .iter()
                    .map(|raw| coach_stop_label(raw))
                    .collect::<Vec<_>>()
                    .join(" → ");
                Entity {
                    id: unit.id.to_string(),
                    name: format!("{} → {}", coach_stop_label(source), route),
                }
            })
            .collect();
        self.management.refresh(data);
        self.freight.refresh(data);
        self.refresh_vehicle();
        self.refresh_seeds();
        self.refresh_offer();
        self.baseline = Some(self.input.clone());
        if let Some(base) = baseline {
            self.input.retain_drafts(&previous, &base);
        }
    }
    fn vehicle_kind(&self) -> VehicleKind {
        match self.tab {
            3 if self.game == Game::Ats => VehicleKind::Car,
            3 => VehicleKind::Coach,
            4 => VehicleKind::Trailer,
            _ => VehicleKind::Truck,
        }
    }
    fn refresh_vehicle(&mut self) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        self.fleet = editor::fleet(&session.data, self.vehicle_kind()).unwrap_or_default();
        self.input.vehicle_index = self
            .input
            .vehicle_index
            .min(self.fleet.len().saturating_sub(1));
        if let Some(entity) = self.fleet.get(self.input.vehicle_index) {
            self.input.fuel = editor::fuel(&session.data, &entity.id).unwrap_or(0.0);
            self.input.plate = session
                .data
                .game
                .string(&entity.id, "license_plate")
                .unwrap_or_default();
            self.input.condition =
                ts_save_core::vehicle::condition(&session.data, self.vehicle_kind(), &entity.id)
                    .unwrap_or_default();
            self.input.paint =
                ts_save_core::vehicle::paint(&session.data, self.vehicle_kind(), &entity.id)
                    .unwrap_or_default();
            self.input.parts =
                ts_save_core::vehicle::parts(&session.data, self.vehicle_kind(), &entity.id)
                    .unwrap_or_default();
        } else {
            self.input.condition.clear();
            self.input.paint.clear();
            self.input.parts.clear();
        }
        if let Some(base) = self.baseline.as_mut() {
            base.fuel = self.input.fuel;
            base.plate = self.input.plate.clone();
            base.condition = self.input.condition.clone();
            base.paint = self.input.paint.clone();
            base.parts = self.input.parts.clone();
        }
    }
    fn refresh_seeds(&mut self) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        self.input.cargo_company = self
            .input
            .cargo_company
            .min(self.companies.len().saturating_sub(1));
        if let Some(id) = self.companies.get(self.input.cargo_company) {
            self.input.cargo_seeds = session
                .data
                .game
                .array(id, "cargo_offer_seeds")
                .unwrap_or_default()
                .join("\n");
        }
        if let Some(base) = self.baseline.as_mut() {
            base.cargo_seeds = self.input.cargo_seeds.clone();
        }
    }
    fn refresh_offer(&mut self) {
        self.input.offer_index = self
            .input
            .offer_index
            .min(self.offers.len().saturating_sub(1));
        if let (Some(session), Some(offer)) =
            (&self.session, self.offers.get(self.input.offer_index))
        {
            for (index, field) in coaches::OFFER_FIELDS.iter().enumerate() {
                self.input.offer_values[index] =
                    session.data.game.integer(&offer.id, field).unwrap_or(0);
            }
        }
        if let Some(base) = self.baseline.as_mut() {
            base.offer_values = self.input.offer_values;
        }
    }
    fn begin_load(&mut self, context: egui::Context, path: PathBuf, game: Game) {
        if self.busy {
            return;
        }
        self.browser.candidates.clear();
        self.load_save(context, path, game, false);
    }
    fn load_next_automatic_save(&mut self, context: &egui::Context) -> bool {
        let Some(path) = self.browser.candidates.pop_front() else {
            return false;
        };
        self.load_save(context.clone(), path, self.game, true);
        true
    }
    fn load_save(&mut self, context: egui::Context, path: PathBuf, game: Game, automatic: bool) {
        self.activity
            .record("INFO", &format!("Loading {} save", game.name()));
        self.busy = true;
        self.status = "Loading and validating save…".into();
        self.failed_slot = path.parent().map(PathBuf::from);
        let sender = self.tx.clone();
        thread::spawn(move || {
            let result = std::panic::catch_unwind(|| SaveSession::load(path, game))
                .unwrap_or(Err(Error::InvalidSii("save loader failed safely")));
            let _ = sender.send(Task::Loaded { automatic, result });
            context.request_repaint();
        });
    }
    fn begin_save(&mut self, context: egui::Context) {
        let Some(mut session) = self.session.take() else {
            return;
        };
        self.activity.record(
            "INFO",
            "Saving staged changes and creating a recovery snapshot",
        );
        self.busy = true;
        self.saving = true;
        self.status = "Creating recovery snapshot and saving…".into();
        let sender = self.tx.clone();
        thread::spawn(move || {
            let result =
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session.save())) {
                    Ok(result) => result,
                    Err(_) => {
                        session.require_reload();
                        Err(Error::InvalidSii(
                            "save writer failed; reload before continuing",
                        ))
                    }
                };
            let _ = sender.send(Task::Saved { session, result });
            context.request_repaint();
        });
    }
    fn poll(&mut self, context: &egui::Context) {
        while let Ok(task) = self.rx.try_recv() {
            if matches!(task, Task::Loaded { .. } | Task::Saved { .. }) {
                self.busy = false;
                self.saving = false;
            }
            match task {
                Task::ReleaseChecked(result) => {
                    self.release_result = self
                        .checking_release
                        .take()
                        .and_then(|origin| result.visible(origin));
                }
                #[cfg(windows)]
                Task::UpdateDownloaded(result) => {
                    self.update_state = match result {
                        Ok(update) => updates::UpdateState::Ready(update),
                        Err(error) => updates::UpdateState::Failed(error.to_string()),
                    };
                }
                Task::Recovered(game, result) => {
                    self.busy = false;
                    self.profile_tools.running = false;
                    match result {
                        Ok(path) if self.unsaved() => {
                            self.pending = Some(Pending::Open(path, game))
                        }
                        Ok(path) => self.begin_load(context.clone(), path, game),
                        Err(error) => self.error = Some(error.to_string()),
                    }
                }
                Task::StarterCreated(result) => {
                    self.busy = false;
                    match result {
                        Ok(()) => {
                            self.status = "Starter save created. Load Fresh starter in the simulator, choose your headquarters city, and save again.".into();
                            self.browser.missing_save = false;
                            self.browser_refresh(context);
                            self.browser.auto_load = false;
                        }
                        Err(error) => self.error = Some(error.to_string()),
                    }
                }
                Task::GameDataLoaded {
                    game,
                    request,
                    result,
                } => {
                    if game == self.game && request == self.game_data.request {
                        self.game_data.loading = false;
                        match result.and_then(|data| self.game_data.accept(data)) {
                            Ok(()) => {
                                self.plate_tools.clear_preview();
                                self.status = "Game data loaded. XP presets and plate artwork choices updated; your edits are unchanged.".into();
                            }
                            Err(error) => self.error = Some(error.to_string()),
                        }
                    }
                }
                Task::ProfileCopied {
                    source,
                    game,
                    operation,
                    slot,
                    result,
                } => self.finish_profile_copy(context, source, game, operation, slot, result),
                Task::Discovered {
                    game,
                    request,
                    catalog,
                } => {
                    if game == self.game && request == self.browser.request {
                        self.browser.finish(catalog);
                        let auto_load = std::mem::take(&mut self.browser.auto_load);
                        if auto_load && self.session.is_none() && !self.busy && !self.unsaved() {
                            self.browser.candidates = self.browser.automatic_candidates(game);
                            if !self.load_next_automatic_save(context) {
                                self.browser.missing_save = true;
                                self.status = "Choose a save or profile folder.".into();
                            }
                        }
                    }
                }
                Task::Loaded { automatic, result } => match result {
                    Ok(session) => {
                        self.failed_slot = None;
                        self.review_open = false;
                        self.browser.candidates.clear();
                        self.profile_tools = Default::default();
                        self.vehicle_tools = Default::default();
                        self.plate_tools = Default::default();
                        self.colour_sharing = false;
                        self.game = session.data.simulator;
                        let path = session.save_folder().join("game.sii");
                        if let Err(error) =
                            self.browser
                                .remember_loaded(self.game, path, !self.isolated)
                        {
                            self.error = Some(error.to_string());
                        }
                        let version = session.data.format_version().unwrap_or(0);
                        self.status = "Save loaded. Ready to edit.".into();
                        if version > SAVE_FORMAT_TARGET {
                            self.error = Some(format!(
                                "This save uses format {version}, newer than format {SAVE_FORMAT_TARGET}. Editing may cause issues. Loading continues; keep an independent backup."
                            ));
                        }
                        self.session = Some(session);
                        self.pack = None;
                        self.baseline = None;
                        self.input = Inputs::default_for_save();
                        self.management = company_ui::Management::default();
                        self.freight = freight_ui::Freight::default();
                        self.refresh();
                    }
                    Err(error) => {
                        if automatic {
                            if self.load_next_automatic_save(context) {
                                continue;
                            }
                            self.browser.missing_save = true;
                            self.status = "Choose a save or profile folder.".into();
                        } else {
                            self.status = "Could not load the selected save.".into();
                        }
                        self.error = Some(error.to_string());
                        if self.session.is_none() && !self.isolated {
                            self.browser.missing_save = true;
                        }
                    }
                },
                Task::Saved { session, result } => {
                    match result {
                        Ok(SaveOutcome::Unchanged) => self.status = "No changes to save.".into(),
                        Ok(SaveOutcome::Saved { changed_files, .. }) => {
                            self.status = format!(
                                "Saved {changed_files} file(s). A full recovery snapshot is in this save's tsset-backups folder."
                            )
                        }
                        Err(error) => {
                            self.status = if session.reload_required() {
                                "Save did not finish. Reload before trying again."
                            } else {
                                "Save did not finish. Review the message before trying again."
                            }
                            .into();
                            self.error = Some(error.to_string());
                        }
                    }
                    self.session = Some(session);
                    self.refresh();
                }
            }
        }
    }
    fn open_dialog(&mut self, context: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title(tr("Open game.sii from a profile save folder"))
            .add_filter("SCS save", &["sii"])
            .pick_file()
        {
            if self.unsaved() {
                self.pending = Some(Pending::Open(path, self.game));
            } else {
                self.begin_load(context.clone(), path, self.game);
            }
        }
    }
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr("Profiles & saves"));
        design::caption(ui, tr("Choose where your next edit begins."));
        ui.add_space(8.0);
        let mut selected = self.game;
        ui.add_enabled_ui(!self.busy, |ui| {
            ui.horizontal(|ui| {
                let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                for (game, label, logo) in [
                    (Game::Ets2, "ETS2", &self.artwork.ets2_logo),
                    (Game::Ats, "ATS", &self.artwork.ats_logo),
                ] {
                    let response = ui.add_sized(
                        [width, 84.0],
                        egui::Button::new("").fill(if selected == game {
                            design::TINT
                        } else {
                            design::WHITE
                        }),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label)
                    });
                    if let Some(logo) = logo {
                        let size = logo.size_vec2();
                        let scale = ((width - 14.0) / size.x).min(52.0 / size.y);
                        ui.painter().image(
                            logo.id(),
                            egui::Rect::from_center_size(
                                egui::pos2(response.rect.center().x, response.rect.top() + 30.0),
                                size * scale,
                            ),
                            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                            design::WHITE,
                        );
                    }
                    ui.painter().text(
                        if logo.is_some() {
                            egui::pos2(response.rect.center().x, response.rect.top() + 67.0)
                        } else {
                            response.rect.center()
                        },
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::proportional(if logo.is_some() { 14.0 } else { 20.0 }),
                        design::INK,
                    );
                    if response.clicked() {
                        selected = game;
                    }
                }
            });
        });
        if selected != self.game {
            if self.unsaved() {
                self.pending = Some(Pending::SwitchGame(selected));
            } else {
                self.switch_game(selected);
            }
        }
        ui.add_space(8.0);
        self.save_browser(ui);
        self.profile_tools_button(ui);
        if let Some(session) = self.session.as_ref() {
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);
            egui::Frame::new()
                .fill(design::CANVAS)
                .corner_radius(8)
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.set_min_width((ui.available_width() - 2.0).max(0.0));
                    design::caption(ui, tr("Current save"));
                    ui.strong(session.data.profile_name());
                    let name = session.data.save_name();
                    ui.label(if name.is_empty() {
                        tr("Unnamed save")
                    } else {
                        &name
                    });
                    design::badge(
                        ui,
                        if session.is_dirty() {
                            tr("Changes staged")
                        } else {
                            tr("No staged changes")
                        },
                        design::ACCENT,
                        design::TINT,
                    );
                });
        }
    }
    fn save_actions(&mut self, ui: &mut egui::Ui) {
        if self
            .session
            .as_ref()
            .is_some_and(SaveSession::reload_required)
        {
            design::notice(
                ui,
                tr("Reload required"),
                tr(
                    "The save on disk changed or the previous write could not be verified. Reload this save before editing again.",
                ),
            );
            if ui.button(tr("Reload this save")).clicked()
                && let Some(session) = &self.session
            {
                let path = session.save_folder().join("game.sii");
                if self.unsaved() {
                    self.pending = Some(Pending::Open(path, self.game));
                } else {
                    self.begin_load(ui.ctx().clone(), path, self.game);
                }
            }
        }
        if self.has_drafts() {
            design::notice(
                ui,
                tr("Inputs to apply"),
                tr("Use the Apply buttons before saving."),
            );
            self.draft_links(ui);
            if ui.button(tr("Revert unapplied inputs")).clicked()
                && let Some(base) = &self.baseline
            {
                self.input = base.clone();
                self.management.revert();
                self.freight.revert();
            }
        }
        if self.session.as_ref().is_some_and(SaveSession::is_dirty)
            && ui.button(tr("Review changes…")).clicked()
        {
            self.review_open = true;
        }
        let can_save = self.session.as_ref().is_some_and(SaveSession::can_save)
            && !self.busy
            && !self.has_drafts();
        if ui
            .add_enabled(
                can_save,
                egui::Button::new((
                    egui::Atom::grow(),
                    RichText::new(tr("Save changes"))
                        .color(design::WHITE)
                        .strong()
                        .size(17.0),
                    egui::Atom::grow(),
                ))
                .fill(design::ACCENT)
                .wrap()
                .min_size([ui.available_width(), 50.0].into()),
            )
            .clicked()
        {
            self.begin_save(ui.ctx().clone());
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().button_padding.x = 10.0;
            let font = egui::TextStyle::Button.resolve(ui.style());
            let label_width = [tr("Undo"), tr("Redo")]
                .into_iter()
                .map(|label| {
                    ui.painter()
                        .layout_no_wrap(label.into(), font.clone(), design::INK)
                        .size()
                        .x
                })
                .fold(0.0_f32, f32::max);
            let gap = ui.spacing().item_spacing.x;
            let button_width = (label_width.ceil() + 2.0 * ui.spacing().button_padding.x + 2.0)
                .max(86.0)
                .min((ui.available_width() - gap) / 2.0);
            let pair_width = 2.0 * button_width + gap;
            ui.add_space(((ui.available_width() - pair_width) / 2.0).max(0.0));
            if ui
                .add_enabled(
                    !self.busy && self.session.as_ref().is_some_and(SaveSession::can_undo),
                    egui::Button::new(tr("Undo"))
                        .wrap()
                        .min_size(egui::vec2(button_width, 36.0)),
                )
                .clicked()
            {
                if let Some(session) = self.session.as_mut() {
                    session.undo();
                }
                self.activity.record("EDIT", "Undo");
                self.refresh();
                self.status = if self
                    .session
                    .as_ref()
                    .is_some_and(|session| session.is_dirty())
                {
                    "Changes staged. Save writes them with a complete recovery snapshot."
                } else {
                    "No staged changes. Ready to edit."
                }
                .into();
            }
            if ui
                .add_enabled(
                    !self.busy && self.session.as_ref().is_some_and(SaveSession::can_redo),
                    egui::Button::new(tr("Redo"))
                        .wrap()
                        .min_size(egui::vec2(button_width, 36.0)),
                )
                .clicked()
            {
                if let Some(session) = self.session.as_mut() {
                    session.redo();
                }
                self.activity.record("EDIT", "Redo");
                self.refresh();
                self.status = if self
                    .session
                    .as_ref()
                    .is_some_and(|session| session.is_dirty())
                {
                    "Changes staged. Save writes them with a complete recovery snapshot."
                } else {
                    "No staged changes. Ready to edit."
                }
                .into();
            }
        });
        ui.add_space(6.0);
        design::caption(
            ui,
            tr(
                "A recovery copy is created when you save. Close the game before editing and keep an independent backup.",
            ),
        );
        if self.busy {
            ui.spinner();
        }
    }
    fn profile(&mut self, ui: &mut egui::Ui) {
        self.identity_controls(ui);
        ui.heading(tr("Player progression"));
        ui.horizontal(|ui| {
            ui.strong(tr("Experience points"));
            ui.add(egui::DragValue::new(&mut self.input.xp).range(0..=i32::MAX as u32));
            design::help(ui, tr("Experience and level"), tr("XP is stored in your save. Changing the level fills in the minimum XP for that level. To use thresholds from an installed game, open Settings → Game data."));
        });
        ui.horizontal_wrapped(|ui| {
            let mut level = self.game_data.curve.level_for_xp(self.input.xp);
            ui.horizontal(|ui| {
                ui.strong(tr("Level"));
                if ui
                    .add(
                        egui::DragValue::new(&mut level)
                            .range(0..=150)
                            .clamp_existing_to_range(false),
                    )
                    .changed()
                    && let Ok(xp) = self.game_data.curve.xp_for_level(level)
                {
                    self.input.xp = xp;
                }
            });
            for (label, delta) in [("−10", -10), ("−1", -1), ("+1", 1), ("+10", 10)] {
                if ui.button(label).clicked() {
                    let level = (level as i32 + delta).clamp(0, 150) as u32;
                    if let Ok(xp) = self.game_data.curve.xp_for_level(level) {
                        self.input.xp = xp;
                    }
                }
            }
            if ui
                .button(tr("Set level 150"))
                .on_hover_text(tr(
                    "Fill the level-150 preset, then choose Apply progression.",
                ))
                .clicked()
                && let Ok(target) = self.game_data.curve.xp_for_level(150)
            {
                self.input.xp = self.input.xp.max(target);
            }
        });
        ui.add_space(10.0);
        skills_ui::editor(
            ui,
            self.game,
            &self.artwork.game,
            &mut self.input.skills,
            &mut self.input.adr,
            "player_skills",
        );
        if design::primary(ui, tr("Apply progression"), true).clicked() {
            let (xp, adr, skills) = (self.input.xp, self.input.adr, self.input.skills);
            self.edit_named("Change player progression", move |data| {
                editor::set_progress(data, xp, adr, skills)
            });
        }
        ui.separator();
        self.user_colors(ui);
    }
    fn company_details(&mut self, ui: &mut egui::Ui) {
        let logo = self
            .session
            .as_ref()
            .and_then(|session| {
                let id = session.data.profile_id().ok()?;
                session.data.profile.string(id, "logo").ok()
            })
            .and_then(|raw| self.artwork.game.company_logo(ui.ctx(), self.game, &raw));
        ui.horizontal(|ui| {
            if let Some(logo) = logo {
                ui.image((logo.id(), egui::vec2(64.0, 64.0)));
            }
            ui.vertical(|ui| {
                ui.heading(tr("Company"));
                design::caption(ui, tr("Set your company details, balance and home base."));
            });
        });
        ui.add_space(12.0);
        ui.strong(tr("Company name"));
        ui.horizontal_wrapped(|ui| {
            let width = 280.0_f32.min(ui.available_width());
            ui.add(
                egui::TextEdit::singleline(&mut self.input.company)
                    .char_limit(20)
                    .desired_width(width)
                    .margin(egui::Margin::symmetric(12, 10)),
            );
            let changed = self
                .baseline
                .as_ref()
                .is_some_and(|base| self.input.company != base.company);
            if ui
                .add_enabled(changed, egui::Button::new(tr("Apply name")))
                .clicked()
            {
                let name = self.input.company.clone();
                self.edit_named("Change company name", move |data| {
                    editor::set_company_name(data, &name)
                });
            }
        });
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.strong(tr("Headquarters"));
            design::help(
                ui,
                tr("Headquarters"),
                tr("Move to an owned garage. Your driver and vehicle move together."),
            );
        });
        ui.horizontal_wrapped(|ui| {
            design::combo(ui, "hq", 280.0)
                .selected_text(city_label(&self.input.hq))
                .show_ui(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    for city in &self.hq_cities {
                        ui.selectable_value(&mut self.input.hq, city.clone(), city_label(city));
                    }
                });
            let changed = self
                .baseline
                .as_ref()
                .is_some_and(|base| self.input.hq != base.hq);
            if ui
                .add_enabled(changed, egui::Button::new(tr("Move headquarters")))
                .clicked()
            {
                let city = self.input.hq.clone();
                self.edit_named("Move headquarters", move |data| {
                    editor::move_headquarters(data, &city)
                });
            }
        });
        ui.add_space(16.0);
        ui.separator();
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.strong(tr("Account balance"));
            design::help(ui, tr("Account balance"), tr("Set balance replaces the amount in the field. Add to balance increases it. Apply balance stages that amount, and Save changes writes it to disk."));
        });
        let currency = if self.game == Game::Ets2 { "€" } else { "$" };
        ui.horizontal(|ui| {
            ui.label(RichText::new(currency).size(28.0).color(design::MUTED));
            let width = (ui.available_width() - 12.0).clamp(180.0, 410.0);
            if ui
                .add(
                    egui::TextEdit::singleline(&mut self.input.money_entry)
                        .font(egui::FontId::proportional(28.0))
                        .desired_width(width)
                        .char_limit(32)
                        .margin(egui::Margin::symmetric(14, 12))
                        .hint_text("1,000,000"),
                )
                .changed()
                && let Ok(money) = parse_balance(&self.input.money_entry)
            {
                self.input.money = money;
            }
        });
        let valid = parse_balance(&self.input.money_entry);
        if let Err(message) = &valid {
            ui.colored_label(Color32::from_rgb(172, 55, 43), message);
        }
        design::caption(ui, tr("Set balance"));
        ui.horizontal_wrapped(|ui| {
            for amount in [100_000, 1_000_000, 10_000_000, 100_000_000] {
                if ui
                    .button(format!("{currency}{}", balance_text(amount)))
                    .clicked()
                {
                    self.input.money = amount;
                    self.input.money_entry = balance_text(amount);
                }
            }
        });
        design::caption(ui, tr("Add to balance"));
        ui.horizontal_wrapped(|ui| {
            for (label, amount) in [
                ("10,000", 10_000),
                ("100,000", 100_000),
                ("1,000,000", 1_000_000),
            ] {
                if ui
                    .add_enabled(
                        valid.is_ok(),
                        egui::Button::new(format!("+ {currency}{label}")).fill(design::TINT),
                    )
                    .clicked()
                {
                    if let Some(money) = self.input.money.checked_add(amount) {
                        self.input.money = money;
                        self.input.money_entry = balance_text(money);
                    } else {
                        self.error = Some(
                            "The balance would exceed the supported whole-number range.".into(),
                        );
                    }
                }
            }
        });
        ui.add_space(8.0);
        let changed = self
            .baseline
            .as_ref()
            .is_some_and(|base| self.input.money_entry != base.money_entry);
        if design::primary(ui, tr("Apply balance"), changed && valid.is_ok()).clicked()
            && let Ok(money) = valid
        {
            self.input.money = money;
            self.input.money_entry = balance_text(money);
            self.edit_named("Change account balance", move |data| {
                editor::set_balance(data, money)
            });
        }
    }
    fn user_colors(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr("Saved paint colours"));
        ui.label(tr(
            "Click a colour swatch to choose a colour. Each saved slot contains four colours.",
        ));
        self.input
            .color_selected
            .resize(self.input.colors.len() / 4, false);
        let mut remove = None;
        egui::Grid::new("paint_colors")
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                for (slot, colors) in self
                    .input
                    .colors
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .enumerate()
                {
                    ui.checkbox(
                        &mut self.input.color_selected[slot],
                        format!("Slot {}", slot + 1),
                    );
                    for (index, raw) in colors.iter_mut().enumerate() {
                        let [r, g, b, a] = profile::rgba(*raw);
                        let mut color = Color32::from_rgba_unmultiplied(r, g, b, a);
                        if ui
                            .color_edit_button_srgba(&mut color)
                            .on_hover_text(format!("Choose colour {}", index + 1))
                            .changed()
                        {
                            *raw = profile::color(color.to_srgba_unmultiplied());
                        }
                    }
                    if ui.button(tr("Remove slot")).clicked() {
                        remove = Some(slot);
                    }
                    ui.end_row();
                }
            });
        if let Some(slot) = remove {
            self.input.colors.drain(slot * 4..slot * 4 + 4);
            self.input.color_selected.remove(slot);
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.input.colors.len() < 160,
                    egui::Button::new(tr("Add colour slot")),
                )
                .clicked()
            {
                self.input
                    .colors
                    .extend([profile::color([255, 255, 255, 255]); 4]);
            }
            if ui.button(tr("Apply colours")).clicked() {
                let colors = self.input.colors.clone();
                self.edit_named("Change saved colour presets", move |data| {
                    profile::set_user_colors(data, &colors)
                });
            }
            if ui.button(tr("Copy selected / all")).clicked() {
                let selected = self.input.color_selected.iter().any(|selected| *selected);
                let colors = self
                    .input
                    .colors
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| !selected || self.input.color_selected[*index])
                    .flat_map(|(_, colors)| colors.iter().copied())
                    .collect::<Vec<_>>();
                match profile::export_colors(&colors) {
                    Ok(text) => {
                        ui.ctx().copy_text(text.clone());
                        self.input.color_share = text;
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
        });
        if ui.button(tr("Import or share colours…")).clicked() {
            self.colour_sharing = true;
        }
        if !self.colour_sharing {
            return;
        }
        let mut open = true;
        egui::Window::new(tr("Import or share saved colours")).id(egui::Id::unique("Import or share saved colours")).open(&mut open).collapsible(false).resizable(true).default_width(540.0).show(ui.ctx(), |ui| {
        ui.label(tr("Paste a colour-sharing code below. Choose Append to add slots, or replace selected slots. Use the colour swatches in Profile to choose colours visually."));
        ui.add(
            egui::TextEdit::multiline(&mut self.input.color_share)
                .desired_width(f32::INFINITY)
                .desired_rows(2)
                .char_limit(64 * 1024)
                .hint_text(tr("Paste colour sharing data here")),
        );
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("Append shared colours")).clicked() {
                match profile::import_colors(&self.input.color_share) {
                    Ok(colors) if self.input.colors.len() + colors.len() <= 160 => {
                        self.input.colors.extend(colors)
                    }
                    Ok(_) => {
                        self.error = Some("The imported colours would exceed 40 slots.".into())
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
            if ui.button(tr("Replace selected slots")).clicked() {
                match profile::import_colors(&self.input.color_share) {
                    Ok(colors) => {
                        let indices = self
                            .input
                            .color_selected
                            .iter()
                            .enumerate()
                            .filter_map(|(index, selected)| selected.then_some(index))
                            .collect::<Vec<_>>();
                        if colors.len() != indices.len() * 4 || indices.is_empty() {
                            self.error = Some(
                                "Select the same number of slots as the imported colours contain."
                                    .into(),
                            );
                        } else {
                            for (slot, colors) in indices.into_iter().zip(colors.as_chunks::<4>().0)
                            {
                                self.input.colors[slot * 4..slot * 4 + 4].copy_from_slice(colors);
                            }
                        }
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
        });
        });
        self.colour_sharing = open;
    }
    fn vehicle(&mut self, ui: &mut egui::Ui) {
        let kind = self.vehicle_kind();
        let icon = match kind {
            VehicleKind::Truck => &self.artwork.truck,
            VehicleKind::Coach => &self.artwork.coach,
            VehicleKind::Car => &self.artwork.car,
            VehicleKind::Trailer => &self.artwork.trailer,
        };
        ui.horizontal(|ui| {
            ui.image((icon.id(), egui::vec2(80.0, 64.0)));
            ui.heading(match kind {
                VehicleKind::Truck => "Truck",
                VehicleKind::Coach => "Coach condition",
                VehicleKind::Car => "Car",
                VehicleKind::Trailer => "Trailer",
            });
        });
        if self.fleet.is_empty() {
            ui.label(tr("No owned or active-job vehicle of this type was found."));
            return;
        }
        let mut selected = self.input.vehicle_index;
        ui.add_enabled_ui(!self.vehicle_draft(), |ui| {
            design::combo(ui, "fleet", 440.0)
                .selected_text(self.fleet.get(selected).map_or_else(
                    || tr("Select vehicle").into(),
                    |entity| title_words(&entity.name),
                ))
                .show_ui(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    for (index, entity) in self.fleet.iter().enumerate() {
                        ui.selectable_value(&mut selected, index, title_words(&entity.name));
                    }
                });
        });
        if selected != self.input.vehicle_index {
            self.input.vehicle_index = selected;
            self.refresh_vehicle();
        }
        let Some(id) = self
            .fleet
            .get(self.input.vehicle_index)
            .map(|entity| entity.id.clone())
        else {
            return;
        };
        if kind != VehicleKind::Trailer {
            ui.horizontal(|ui| {
                ui.strong(tr("Fuel"));
                design::help(ui, tr("Fuel"), tr("Set the selected vehicle’s fuel as a percentage of its tank capacity. Apply fuel stages the change."));
            });
            ui.horizontal_wrapped(|ui| {
                design::percent_slider(ui, &mut self.input.fuel);
                if design::primary(
                    ui,
                    tr("Apply fuel"),
                    self.baseline
                        .as_ref()
                        .is_some_and(|base| base.fuel != self.input.fuel),
                )
                .clicked()
                {
                    let percent = self.input.fuel;
                    let id = id.clone();
                    self.edit_named("Change vehicle fuel", move |data| {
                        editor::set_fuel(data, kind, &id, percent)
                    });
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("Repair vehicle")).clicked() {
                let id = id.clone();
                let permanent = self.input.permanent;
                self.edit_named("Repair vehicle", move |data| {
                    editor::repair(data, kind, &id, permanent).map(|_| ())
                });
            }
            ui.checkbox(&mut self.input.permanent, tr("Include permanent wear"));
            design::help(ui, tr("Repair vehicle"), tr("Repair stages a reset of damage and repairable wear on the selected vehicle. Include permanent wear also clears age-related wear while preserving mileage."));
        });
        ui.separator();
        self.plate_controls(ui, kind);
        if ui.button(tr("Apply plate")).clicked() {
            let plate = self.input.plate.clone();
            let id = id.clone();
            self.edit_named("Change number plate", move |data| {
                editor::set_license_plate(data, kind, &id, &plate)
            });
        }
        self.vehicle_details(ui, kind, &id);
    }
    fn coach(&mut self, ui: &mut egui::Ui) {
        let state = self
            .coach_state
            .clone()
            .unwrap_or(coaches::CoachState::MissingData);
        ui.label(state.message());
        if !state.ready() {
            return;
        }
        ui.vertical(|ui| {
            ui.heading(tr("Coach progression"));
            ui.horizontal(|ui| {
                ui.label(tr("Experience"));
                ui.add(egui::DragValue::new(&mut self.input.coach_xp).range(0..=i32::MAX as u32));
            });
            ui.horizontal(|ui| {
                ui.strong(tr("Driver skills"));
                design::help(ui, tr("Coach progression"), tr("Choose a rank from 0 to 3 for each coach skill. Apply coach progression stages the XP and skill changes."));
            });
            egui::Grid::new("coach_ranks").striped(true).spacing([24.0, 6.0]).show(ui, |ui| {
                for (index, label) in [
                    "Long duty",
                    "Tight schedule",
                    "Satisfaction loss resistance",
                    "Low acceleration",
                    "Satisfaction gain",
                    "Eco driving",
                ]
                .iter()
                .enumerate()
                {
                    ui.label(tr(label));
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for rank in 0..=3 {
                            let selected = self.input.coach_skills[index] == rank;
                            if ui.add(
                                egui::Button::new(rank.to_string())
                                    .selected(selected)
                                    .fill(if selected { design::TINT } else { design::WHITE })
                                    .min_size(egui::vec2(34.0, 32.0))
                            ).clicked() {
                                self.input.coach_skills[index] = rank;
                            }
                        }
                    });
                    ui.end_row();
                }
            });
            ui.horizontal_wrapped(|ui| {
                if design::primary(ui, tr("Apply coach progression"), true).clicked() {
                    let xp = self.input.coach_xp;
                    let ranks = self.input.coach_skills;
                    self.edit_named("Change coach progression", move |data| {
                        coaches::set_progress(data, xp, ranks)
                    });
                }
                if ui.button(tr("Discover all coach stations")).clicked() {
                    self.edit_named("Discover coach stations", |data| {
                        coaches::discover_stations(data).map(|_| ())
                    });
                }
            });
            ui.separator();
            self.vehicle(ui);
            ui.separator();
            ui.heading(tr("Passenger journey"));
            if let coaches::CoachState::Ready {
                stages, passengers, ..
            } = &state
            {
                ui.label(format!(
                    "{} stages · {} passengers",
                    stages.len(),
                    passengers.len()
                ));
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("Satisfaction"));
                design::percent_slider(ui, &mut self.input.satisfaction);
                if ui.button(tr("Apply to passengers")).clicked() {
                    let percent = self.input.satisfaction;
                    self.edit_named("Change passenger satisfaction", move |data| {
                        coaches::set_satisfaction(data, percent).map(|_| ())
                    });
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("Extra minutes"));
                ui.add(egui::DragValue::new(&mut self.input.extra_minutes).range(1..=1440));
                if ui.button(tr("Extend remaining stages")).clicked() {
                    let minutes = self.input.extra_minutes;
                    self.edit_named("Extend coach timetable", move |data| {
                        coaches::extend_trip(data, minutes)
                    });
                }
            });
            self.coach_details(ui, &state);
        });
    }
    fn coach_details(&mut self, ui: &mut egui::Ui, state: &coaches::CoachState) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let doc = &session.data.game;
        let coaches::CoachState::Ready {
            job,
            stages,
            passengers,
        } = state
        else {
            return;
        };
        let current = doc.integer::<usize>(job, "stage_idx").unwrap_or(0);
        let text = |id: &str, field: &str| -> String {
            let raw = doc.value(id, field).unwrap_or("—");
            if raw == "4294967295" || value::is_null(raw) {
                "—".into()
            } else if matches!(field, "source_bus_stop" | "target_bus_stop") {
                coach_stop_label(raw)
            } else {
                value::unquote(raw).unwrap_or_else(|_| raw.into())
            }
        };
        ui.separator();
        ui.heading(tr("Journey stages"));
        ui.scope(|ui| {
            let column_width = ((ui.available_width() - 72.0) / 7.0).max(40.0);
            egui::Grid::new("coach_stages")
                .striped(true)
                .min_col_width(40.0)
                .max_col_width(column_width)
                .show(ui, |ui| {
                    for header in [
                        "Stage",
                        "From",
                        "To",
                        "Distance (km)",
                        "Duration (min)",
                        "Departure",
                        "Arrival",
                    ] {
                        ui.strong(header);
                    }
                    ui.end_row();
                    for (index, id) in stages.iter().enumerate() {
                        ui.label(format!(
                            "{}{}",
                            index + 1,
                            if index == current { " (current)" } else { "" }
                        ));
                        for field in [
                            "source_bus_stop",
                            "target_bus_stop",
                            "planned_distance",
                            "planned_stage_time",
                            "planned_departure_time",
                            "planned_arrival_time",
                        ] {
                            ui.label(text(id, field));
                        }
                        ui.end_row();
                    }
                });
        });
        ui.heading(tr("Passengers"));
        egui::ScrollArea::vertical()
            .id_salt("coach_passengers_scroll")
            .max_height(240.0)
            .show(ui, |ui| {
                egui::Grid::new("coach_passengers")
                    .striped(true)
                    .max_col_width(((ui.available_width() - 36.0) / 4.0).max(40.0))
                    .show(ui, |ui| {
                        for header in ["Seat", "From", "To", "Satisfaction"] {
                            ui.strong(header);
                        }
                        ui.end_row();
                        for id in passengers {
                            for field in ["seat_idx", "source_bus_stop", "target_bus_stop"] {
                                ui.label(text(id, field));
                            }
                            let satisfaction = doc
                                .value(id, "satisfaction")
                                .and_then(|raw| value::float(raw).ok())
                                .map(|number| format!("{:.1}%", number * 100.0))
                                .unwrap_or_else(|| "—".into());
                            ui.label(satisfaction);
                            ui.end_row();
                        }
                    });
            });
        ui.separator();
        ui.heading(tr("Existing passenger offers"));
        if self.offers.is_empty() {
            ui.label(tr("No passenger offers were found in this save."));
            return;
        }
        let mut selected = self.input.offer_index;
        design::combo(ui, "coach_offer", 650.0)
            .selected_text(
                self.offers
                    .get(selected)
                    .map_or(tr("Select offer"), |offer| offer.name.as_str()),
            )
            .show_ui(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                for (index, offer) in self.offers.iter().enumerate() {
                    ui.selectable_value(&mut selected, index, &offer.name);
                }
            });
        if selected != self.input.offer_index {
            if self
                .baseline
                .as_ref()
                .is_some_and(|base| self.input.offer_values != base.offer_values)
            {
                self.error =
                    Some("Apply or revert the offer inputs before selecting another offer.".into());
            } else {
                self.input.offer_index = selected;
                self.refresh_offer();
            }
        }
        egui::Grid::new("coach_offer_values").show(ui, |ui| {
            for (index, label) in [
                "Departure (minute of day)",
                "Expiration (game minute)",
                "Tight schedule rank",
                "Satisfaction loss resistance rank",
                "Low acceleration rank",
            ]
            .iter()
            .enumerate()
            {
                ui.label(*label);
                let max = match index {
                    0 => 1439,
                    1 => i32::MAX as u32,
                    _ => 3,
                };
                ui.add(egui::DragValue::new(&mut self.input.offer_values[index]).range(0..=max));
                ui.end_row();
            }
        });
        if ui.button(tr("Apply passenger offer")).clicked()
            && let Some(id) = self
                .offers
                .get(self.input.offer_index)
                .map(|offer| offer.id.clone())
        {
            let values = self.input.offer_values;
            self.edit(move |data| {
                for (field, value) in coaches::OFFER_FIELDS.iter().zip(values) {
                    coaches::set_offer(data, &id, field, value)?;
                }
                Ok(())
            });
        }
    }
    fn cargo(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(tr("Cargo Market"));
            design::help(ui, tr("Cargo Market"), tr("Regenerate replaces the saved offer seeds. Reset clears them. Company affects the selected depot, while City affects all depots in its city. Save changes writes the result to disk."));
        });
        ui.label(tr("Regenerate or edit offer seeds. The game chooses the resulting cargo and destinations."));
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("Find company / city"));
            ui.text_edit_singleline(&mut self.input.cargo_filter);
        });
        let mut selected = self.input.cargo_company;
        let filter = self.input.cargo_filter.to_lowercase();
        design::combo(ui, "cargo_company", 600.0)
            .selected_text(self.companies.get(selected).map_or_else(
                || tr("Select company").into(),
                |id| freight_ui::company_label(id),
            ))
            .show_ui(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                for (index, id) in self.companies.iter().enumerate() {
                    let label = freight_ui::company_label(id);
                    if label.to_lowercase().contains(&filter) || id.to_lowercase().contains(&filter)
                    {
                        ui.selectable_value(&mut selected, index, label);
                    }
                }
            });
        if selected != self.input.cargo_company {
            if self
                .baseline
                .as_ref()
                .is_some_and(|base| self.input.cargo_seeds != base.cargo_seeds)
            {
                self.error = Some(
                    "Apply or revert the edited seeds before choosing another company.".into(),
                );
            } else {
                self.input.cargo_company = selected;
                self.refresh_seeds();
            }
        }
        let Some(id) = self.companies.get(self.input.cargo_company).cloned() else {
            ui.label(tr(
                "No Cargo Market seed fields are available in this save.",
            ));
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("Offers"));
            ui.add(egui::DragValue::new(&mut self.cargo_parameters.count).range(1..=64));
            ui.label(tr("Valid minutes"));
            ui.add(egui::DragValue::new(&mut self.cargo_parameters.valid_minutes).range(1..=10080));
            ui.label(tr("Spacing (minutes)"));
            ui.add(
                egui::DragValue::new(&mut self.cargo_parameters.spacing_minutes).range(1..=1440),
            );
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(tr("Regenerate company"))
                .on_hover_text(tr(
                    "Replace the selected depot’s seed list using the values above.",
                ))
                .clicked()
            {
                let id = id.clone();
                let parameters = self.cargo_parameters;
                self.edit_named("Regenerate company cargo market", move |data| {
                    cargo::regenerate(data, &id, parameters)
                });
            }
            if ui
                .button(tr("Regenerate city"))
                .on_hover_text(tr(
                    "Replace seed lists at every Cargo Market depot in the selected city.",
                ))
                .clicked()
            {
                let id = id.clone();
                let parameters = self.cargo_parameters;
                self.edit_named("Change city cargo market", move |data| {
                    cargo::city(data, &id, Some(parameters)).map(|_| ())
                });
            }
            if ui
                .button(tr("Reset company"))
                .on_hover_text(tr("Clear the saved offer seeds at the selected depot."))
                .clicked()
            {
                let id = id.clone();
                self.edit_named("Change cargo market offers", move |data| {
                    cargo::replace(data, &id, &[])
                });
            }
            if ui
                .button(tr("Reset city"))
                .on_hover_text(tr(
                    "Clear the saved offer seeds at every Cargo Market depot in the selected city.",
                ))
                .clicked()
            {
                let id = id.clone();
                self.edit_named("Change city cargo market", move |data| {
                    cargo::city(data, &id, None).map(|_| ())
                });
            }
        });
        ui.add_space(12.0);
        ui.label(tr("Offer seeds (absolute game minutes), one per line:"));
        ui.add(
            egui::TextEdit::multiline(&mut self.input.cargo_seeds)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY)
                .desired_rows(13),
        );
        if ui.button(tr("Apply edited seeds"))
            .on_hover_text(tr("Stage the seed list below for the selected depot. Use Save changes to write it to disk."))
            .clicked() {
            let seeds = self
                .input
                .cargo_seeds
                .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
                .filter(|token| !token.is_empty())
                .map(str::parse::<u32>)
                .collect::<std::result::Result<Vec<_>, _>>();
            match seeds {
                Ok(seeds) => self.edit_named("Change cargo market offers", move |data| {
                    cargo::replace(data, &id, &seeds)
                }),
                Err(_) => self.error = Some("Offer seeds must be unsigned integers.".into()),
            }
        }
    }
    fn convoy(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr("Convoy Tools"));
        ui.label(tr("Share vehicle positions and GPS routes between saves for the same game. Use compatible maps, DLC and mods."));
        ui.horizontal(|ui| {
            ui.label(tr("Position name"));
            ui.add(egui::TextEdit::singleline(&mut self.input.position_name).char_limit(100));
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("Add current position")).clicked() {
                let result = self
                    .session
                    .as_ref()
                    .map(|session| convoy::capture(&session.data, &self.input.position_name));
                if let Some(result) = result {
                    match result {
                        Ok(captured) => {
                            if let Some(pack) = self.pack.as_mut() {
                                if pack.positions.len() < 128 {
                                    pack.positions.push(captured.positions[0].clone());
                                } else {
                                    self.error =
                                        Some("A pack can contain at most 128 positions.".into());
                                }
                            } else {
                                self.pack = Some(captured);
                            }
                            self.sync_pack_json();
                        }
                        Err(error) => self.error = Some(error.to_string()),
                    }
                }
            }
            if ui.button(tr("Capture current GPS")).clicked()
                && let Some(session) = self.session.as_ref()
            {
                match convoy::capture(&session.data, &self.input.position_name) {
                    Ok(captured) => {
                        if let Some(pack) = self.pack.as_mut() {
                            pack.behind = captured.behind;
                            pack.ahead = captured.ahead;
                            pack.avoid = captured.avoid;
                        } else {
                            self.pack = Some(captured);
                        }
                        self.sync_pack_json();
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
            if ui.button(tr("Copy pack")).clicked()
                && let Some(pack) = &self.pack
            {
                match convoy::export(pack) {
                    Ok(json) => ui.ctx().copy_text(json),
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
        });
        if let Some(pack) = &self.pack {
            ui.label(format!(
                "{} positions · GPS: {} behind, {} ahead, {} avoid",
                pack.positions.len(),
                pack.behind.len(),
                pack.ahead.len(),
                pack.avoid.len()
            ));
            let mut selected = self.input.position_index;
            design::combo(ui, "convoy_position", 500.0)
                .selected_text(
                    pack.positions
                        .get(selected)
                        .map_or("Select position", |position| position.name.as_str()),
                )
                .show_ui(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    for (index, position) in pack.positions.iter().enumerate() {
                        ui.selectable_value(&mut selected, index, &position.name);
                    }
                });
            self.input.position_index = selected;
        }
        ui.horizontal_wrapped(|ui| {
            let has_position = self
                .pack
                .as_ref()
                .is_some_and(|pack| pack.positions.get(self.input.position_index).is_some());
            if ui
                .add_enabled(
                    has_position,
                    egui::Button::new(tr("Remove selected position from pack")),
                )
                .clicked()
                && let Some(pack) = self.pack.as_mut()
            {
                pack.positions.remove(self.input.position_index);
                self.input.position_index = self
                    .input
                    .position_index
                    .min(pack.positions.len().saturating_sub(1));
                self.sync_pack_json();
                self.status =
                    "Position removed from sharing pack. The loaded save is unchanged.".into();
            }
            if ui
                .add_enabled(
                    has_position,
                    egui::Button::new(tr("Apply selected position")),
                )
                .clicked()
                && let Some(pack) = self.pack.clone()
            {
                let index = self.input.position_index;
                self.edit_named("Apply convoy position", move |data| {
                    convoy::apply_position(data, &pack, index)
                });
            }
            if ui.button(tr("Apply GPS route")).clicked()
                && let Some(pack) = self.pack.clone()
            {
                self.edit_named("Apply GPS route", move |data| {
                    convoy::apply_route(data, &pack)
                });
            }
            if ui.button(tr("Clear loaded save GPS route")).clicked() {
                self.edit_named("Clear GPS route", convoy::clear_route);
            }
            if ui.button(tr("Import file…")).clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Convoy JSON", &["json"])
                    .pick_file()
            {
                match std::fs::metadata(&path).and_then(|meta| {
                    if meta.len() <= 512 * 1024 {
                        std::fs::read_to_string(&path)
                    } else {
                        Err(std::io::Error::other("Convoy file exceeds 512 KiB."))
                    }
                }) {
                    Ok(json) => {
                        self.input.pack_json = json;
                        self.import_pack();
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
            if ui.button(tr("Export file…")).clicked()
                && let Some(pack) = &self.pack
            {
                match convoy::export(pack) {
                    Ok(json) => {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Convoy JSON", &["json"])
                            .set_file_name("convoy.json")
                            .save_file()
                            && let Err(error) = std::fs::write(path, json)
                        {
                            self.error = Some(error.to_string());
                        }
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
        });
        ui.add_space(10.0);
        ui.label(tr(
            "Paste or review convoy JSON (importing alone does not change your save):",
        ));
        ui.add(
            egui::TextEdit::multiline(&mut self.input.pack_json)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY)
                .desired_rows(12)
                .char_limit(512 * 1024),
        );
        if ui.button(tr("Import pasted pack")).clicked() {
            self.import_pack();
        }
    }
    fn sync_pack_json(&mut self) {
        if let Some(pack) = &self.pack {
            match convoy::export(pack) {
                Ok(json) => self.input.pack_json = json,
                Err(error) => self.error = Some(error.to_string()),
            }
        }
    }
    fn switch_game(&mut self, game: Game) {
        self.game = game;
        self.browser.invalidate_game();
        self.game_data.reset(game);
        self.session = None;
        self.management = company_ui::Management::default();
        self.freight = freight_ui::Freight::default();
        self.profile_tools = Default::default();
        self.vehicle_tools = Default::default();
        self.baseline = None;
        self.pack = None;
        self.fleet.clear();
        self.companies.clear();
        self.hq_cities.clear();
        self.coach_state = None;
        self.status = format!("Open a {} profile save.", game.name());
    }
    fn import_pack(&mut self) {
        let game = self
            .session
            .as_ref()
            .map_or(self.game, |session| session.data.simulator);
        match convoy::import(&self.input.pack_json, game) {
            Ok(pack) => {
                self.pack = Some(pack);
                self.input.position_index = 0;
                self.status =
                    "Convoy pack imported. Apply a position or route to stage a save change."
                        .into();
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
}

fn human(raw: &str) -> String {
    raw.replace(['_', '.'], " ")
}

fn title_words(id: &str) -> String {
    human(id)
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            let first = chars.next().expect("word is nonempty");
            format!("{}{}", first.to_uppercase(), chars.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn coach_stop_label(raw: &str) -> String {
    let stop = value::unquote(raw).unwrap_or_else(|_| raw.into());
    city_label(stop.strip_prefix("bus_stop.city.").unwrap_or(&stop))
}

fn city_label(id: &str) -> String {
    match id {
        // ATS uses this shortened token in save and definition references.
        "san_francisc" => "San Francisco".into(),
        _ => title_words(id),
    }
}

fn city_matches(id: &str, filter: &str) -> bool {
    id.to_lowercase().contains(filter) || city_label(id).to_lowercase().contains(filter)
}

fn balance_text(money: i64) -> String {
    let raw = money.unsigned_abs().to_string();
    let mut result = String::new();
    if money < 0 {
        result.push('-');
    }
    for (index, character) in raw.chars().enumerate() {
        if index > 0 && (raw.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(character);
    }
    result
}
fn parse_balance(text: &str) -> std::result::Result<i64, String> {
    let digits = text
        .chars()
        .filter(|character| !matches!(character, ',' | '_' | ' '))
        .collect::<String>();
    digits
        .parse::<i64>()
        .map_err(|_| "Enter a whole-number balance within the supported range.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coach_stop_labels_format_bare_and_quoted_city_references() {
        assert_eq!(
            coach_stop_label("bus_stop.city.san_sebastian"),
            "San Sebastian"
        );
        assert_eq!(coach_stop_label("\"bus_stop.city.hamburg\""), "Hamburg");
        assert_eq!(coach_stop_label("\"le_havre\""), "Le Havre");
    }

    #[test]
    fn full_city_name_is_displayed_and_searchable_for_the_ats_token() {
        assert_eq!(city_label("san_francisc"), "San Francisco");
        assert!(city_matches("san_francisc", "san francisco"));
        assert!(city_matches("san_francisc", "san_francisc"));
    }

    #[test]
    fn balance_entry_preserves_large_integers_and_rejects_invalid_input() {
        for money in [
            0,
            -1000,
            1_000_000,
            i64::MIN,
            i64::MAX,
            9_007_199_254_740_993,
        ] {
            assert_eq!(parse_balance(&balance_text(money)).unwrap(), money);
        }
        for text in ["", "1.5", "1e6", "9223372036854775808", "--10"] {
            assert!(parse_balance(text).is_err());
        }
    }
    #[test]
    fn applying_one_group_retains_other_unapplied_inputs() {
        let baseline = Inputs {
            xp: 10,
            money: 100,
            ..Default::default()
        };
        let previous = Inputs {
            xp: 20,
            money: 200,
            condition: vec![ts_save_core::vehicle::Wear {
                field: "engine_wear".into(),
                wheel: None,
                fraction: 0.125,
            }],
            paint: vec![ts_save_core::vehicle::PaintColor {
                accessory: "paint.test".into(),
                field: "base_color".into(),
                linear: [0.1, 0.2, 0.3],
            }],
            identity: Some(profile::Identity {
                face: 1,
                logo: "logo_0".into(),
                male: true,
            }),
            ..baseline.clone()
        };
        let mut refreshed = Inputs {
            xp: 20,
            ..baseline.clone()
        };
        let saved_baseline = refreshed.clone();
        refreshed.retain_drafts(&previous, &baseline);
        assert_eq!(refreshed.xp, 20);
        assert_eq!(refreshed.money, 200);
        assert_eq!(refreshed.condition, previous.condition);
        assert_eq!(refreshed.paint, previous.paint);
        assert_eq!(refreshed.identity, previous.identity);
        assert!(refreshed.has_drafts(&saved_baseline));
        let saved_baseline = refreshed.clone();
        assert!(!refreshed.has_drafts(&saved_baseline));
    }

    #[test]
    fn applying_balance_keeps_the_company_name_draft_and_undoes_only_the_balance() {
        let temporary = tempfile::tempdir().unwrap();
        let save = temporary.path().join("save/1");
        std::fs::create_dir_all(&save).unwrap();
        std::fs::write(
            temporary.path().join("profile.sii"),
            "SiiNunit\n{\nuser_profile : profile.test {\n company_name: \"Original\"\n}\n}\n",
        )
        .unwrap();
        std::fs::write(
            save.join("info.sii"),
            "SiiNunit\n{\nsave_container : info.test {\n version: 105\n}\n}\n",
        )
        .unwrap();
        let game_path = save.join("game.sii");
        let original = "SiiNunit\n{\neconomy : economy.test {\n player: player.test\n bank: bank.test\n}\nplayer : player.test {\n hq_city: \"sample\"\n}\nbank : bank.test {\n money_account: 100\n}\n}\n";
        std::fs::write(&game_path, original).unwrap();
        let mut app = EditorApp::new(&eframe::CreationContext::_new_kittest(
            egui::Context::default(),
        ));
        app.session = Some(SaveSession::load(game_path.clone(), Game::Ets2).unwrap());
        app.refresh();
        app.input.company = "Pending".into();
        app.input.money = 9_007_199_254_740_993;
        app.input.money_entry = balance_text(app.input.money);
        let money = app.input.money;
        app.edit_named("Change account balance", |data| {
            editor::set_balance(data, money)
        });

        let baseline = app.baseline.as_ref().unwrap();
        assert_eq!(baseline.company, "Original");
        assert_eq!(baseline.money, money);
        assert_eq!(app.input.company, "Pending");
        assert!(app.session.as_mut().unwrap().undo());
        app.refresh();
        assert_eq!(app.input.money, 100);
        assert_eq!(app.input.company, "Pending");
        assert_eq!(std::fs::read_to_string(game_path).unwrap(), original);
    }
}
