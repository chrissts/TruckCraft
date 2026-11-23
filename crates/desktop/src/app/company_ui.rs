use super::*;
use std::collections::BTreeSet;
use ts_save_core::company::{self, DriverStatus, MoveKind};

#[derive(Clone, PartialEq, Eq)]
struct DriverDraft {
    xp: u32,
    adr: u8,
    skills: [u8; 5],
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    fn fits(
        context: &egui::Context,
        label: &str,
        width: f32,
        mut content: impl FnMut(&mut egui::Ui),
    ) {
        for frame in 0..4 {
            let mut used = 0.0_f32;
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 600.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ui, |ui| {
                            let output =
                                egui::ScrollArea::vertical().id_salt(label).show(ui, |ui| {
                                    content(ui);
                                });
                            used = output.content_size.x;
                        });
                },
            );
            output.textures_delta.clear();
            if frame >= 2 {
                assert!(
                    used <= width + 1.0,
                    "{label}: {used:.1}px of content exceeds {width:.1}px viewport"
                );
            }
        }
    }

    fn app(context: &egui::Context) -> EditorApp {
        EditorApp::new(&eframe::CreationContext::_new_kittest(context.clone()))
    }

    #[test]
    fn editor_and_sidebar_fit_without_horizontal_scroll() {
        let context = egui::Context::default();
        let mut app = app(&context);
        app.input.money_entry = balance_text(i64::MAX);
        app.input.xp = i32::MAX as u32;
        app.input.colors = vec![u32::MAX; 16];
        app.fleet = vec![Entity {
            id: "vehicle.example".into(),
            name: "A very long vehicle name ".repeat(12),
        }];
        app.companies = vec!["company.volatile.long_company_name.long_city_name".repeat(8)];
        app.hq_cities = vec!["a_long_headquarters_city_name".repeat(8)];
        app.browser.finish(ts_save_core::discovery::Catalog {
            profiles: vec![ts_save_core::discovery::Profile {
                path: PathBuf::new(),
                name: "A very long profile name ".repeat(10),
                source: "Custom profiles".into(),
                avatar: String::new(),
                saves: vec![ts_save_core::discovery::SaveEntry {
                    path: PathBuf::new(),
                    name: "A very long save name ".repeat(10),
                    modified: None,
                }],
            }],
            ..Default::default()
        });
        // The narrowest editor occurs with the sidebar at its widest setting.
        for width in [680.0, 1000.0] {
            fits(&context, "tabs", width, |ui| app.tabs(ui));
            fits(&context, "empty", width, |ui| app.empty(ui));
            fits(&context, "profile", width, |ui| app.profile(ui));
            assert_eq!(
                app.input.xp,
                i32::MAX as u32,
                "rendering the level control must preserve existing XP above level 150"
            );
            fits(&context, "company", width, |ui| app.company_details(ui));
            fits(&context, "vehicle", width, |ui| app.vehicle(ui));
            fits(&context, "cargo", width, |ui| app.cargo(ui));
            fits(&context, "freight", width, |ui| app.freight(ui));
            fits(&context, "convoy", width, |ui| app.convoy(ui));
        }
        for &locale in crate::i18n::languages().keys() {
            crate::i18n::select(locale);
            fits(&context, &format!("{locale} tabs"), 680.0, |ui| {
                app.tabs(ui)
            });
            fits(&context, &format!("{locale} cargo"), 680.0, |ui| {
                app.cargo(ui)
            });
            fits(&context, &format!("{locale} profile"), 680.0, |ui| {
                app.profile(ui)
            });
            fits(&context, &format!("{locale} company"), 680.0, |ui| {
                app.company_details(ui)
            });
            for width in [230.0, 274.0, 340.0] {
                fits(&context, &format!("{locale} sidebar"), width, |ui| {
                    app.sidebar(ui)
                });
                fits(&context, &format!("{locale} save actions"), width, |ui| {
                    app.save_actions(ui)
                });
            }
        }
        crate::i18n::select("en");
    }

    #[test]
    #[ignore = "requires a private ATS Road Trip save fixture"]
    fn active_car_page_fits_at_minimum_window_width() {
        let root = std::env::var_os("TSSET_CAR_FIXTURE").expect("TSSET_CAR_FIXTURE");
        let context = egui::Context::default();
        let mut app = app(&context);
        app.game = Game::Ats;
        app.game_data.reset(Game::Ats);
        app.session = Some(
            SaveSession::load(PathBuf::from(root).join("save/1/game.sii"), Game::Ats).unwrap(),
        );
        app.tab = 3;
        app.refresh();
        assert!(
            ts_save_core::cars::detect(&app.session.as_ref().unwrap().data)
                .unwrap()
                .ready()
        );
        fits(&context, "active_car_journey", 680.0, |ui| app.car(ui));
    }

    #[test]
    #[ignore = "requires private ETS2/ATS fixture files"]
    fn real_save_pages_fit_at_minimum_window_width() {
        let root = std::env::var_os("TSSET_FIXTURE_ROOT").expect("TSSET_FIXTURE_ROOT");
        let context = egui::Context::default();
        let mut app = app(&context);
        for game in [Game::Ets2, Game::Ats] {
            app.game = game;
            app.game_data.reset(game);
            app.session = Some(
                SaveSession::load(
                    PathBuf::from(&root)
                        .join(game.name())
                        .join("profile/save/1/game.sii"),
                    game,
                )
                .unwrap(),
            );
            app.refresh();
            if let Some(garage) = app
                .management
                .garages
                .iter()
                .find(|garage| garage.status != 0)
            {
                app.management.garage_selection.insert(garage.id.clone());
            }
            for page in 0..4 {
                app.management.page = page;
                fits(&context, &format!("company_{page}"), 680.0, |ui| {
                    app.company(ui)
                });
            }
            for tab in [0, 2, 3, 4, 5, 6, 7] {
                app.tab = tab;
                app.refresh_vehicle();
                fits(&context, &format!("page_{tab}"), 680.0, |ui| match tab {
                    0 => app.profile(ui),
                    2 | 4 => app.vehicle(ui),
                    3 if game == Game::Ats => app.car(ui),
                    3 => app.coach(ui),
                    5 => app.freight(ui),
                    6 => app.cargo(ui),
                    7 => app.convoy(ui),
                    _ => unreachable!(),
                });
            }
            for &locale in crate::i18n::languages().keys() {
                crate::i18n::select(locale);
                fits(&context, &format!("{locale} freight"), 680.0, |ui| {
                    app.freight(ui)
                });
                fits(&context, &format!("{locale} cargo"), 680.0, |ui| {
                    app.cargo(ui)
                });
            }
            crate::i18n::select("en");
        }
        let coach_root = std::env::var_os("TSSET_COACH_FIXTURE").expect("TSSET_COACH_FIXTURE");
        let temporary = tempfile::tempdir().unwrap();
        let profile = temporary.path().join("profile");
        let save = profile.join("save/1");
        std::fs::create_dir_all(&save).unwrap();
        std::fs::copy(
            PathBuf::from(&root).join("ETS2/profile/profile.sii"),
            profile.join("profile.sii"),
        )
        .unwrap();
        for name in ["game.sii", "info.sii"] {
            std::fs::copy(PathBuf::from(&coach_root).join(name), save.join(name)).unwrap();
        }
        app.game = Game::Ets2;
        app.session = Some(SaveSession::load(save.join("game.sii"), Game::Ets2).unwrap());
        app.tab = 3;
        app.refresh();
        assert!(
            app.coach_state
                .as_ref()
                .is_some_and(coaches::CoachState::ready)
        );
        fits(&context, "active_coach_journey", 680.0, |ui| app.coach(ui));
    }
}
impl From<&company::Driver> for DriverDraft {
    fn from(driver: &company::Driver) -> Self {
        Self {
            xp: driver.xp,
            adr: driver.adr,
            skills: driver.skills,
        }
    }
}
#[derive(Default)]
pub(super) struct Management {
    pub(super) page: usize,
    garages: Vec<company::Garage>,
    drivers: Vec<company::Driver>,
    cities: Vec<(String, u32)>,
    error: Option<String>,
    garage_filter: String,
    garage_selection: BTreeSet<String>,
    target: String,
    source_slot: usize,
    target_slot: usize,
    move_kind: usize,
    driver_filter: String,
    driver_id: String,
    driver_draft: Option<DriverDraft>,
    driver_base: Option<DriverDraft>,
    hire_garage: String,
    city_filter: String,
    city_selection: BTreeSet<String>,
}
impl Management {
    pub fn has_drafts(&self) -> bool {
        self.driver_draft != self.driver_base
    }
    pub fn revert(&mut self) {
        self.driver_draft = self.driver_base.clone();
    }
    pub fn refresh(&mut self, data: &SaveData) {
        let result = (|| -> Result<_> {
            Ok((
                company::garages(data)?,
                company::drivers(data)?,
                company::cities(data)?,
            ))
        })();
        match result {
            Ok((garages, drivers, cities)) => {
                self.garages = garages;
                self.drivers = drivers;
                self.cities = cities;
                self.error = None;
                self.garage_selection
                    .retain(|id| self.garages.iter().any(|garage| &garage.id == id));
                self.city_selection
                    .retain(|id| self.cities.iter().any(|(city, _)| city == id));
                if !self
                    .drivers
                    .iter()
                    .any(|driver| driver.id == self.driver_id)
                {
                    self.driver_id = self
                        .drivers
                        .iter()
                        .find(|driver| driver.status != DriverStatus::Player)
                        .map(|driver| driver.id.clone())
                        .unwrap_or_default();
                    self.driver_base = None;
                    self.driver_draft = None;
                }
                let preserve = self.has_drafts();
                self.driver_base = self
                    .drivers
                    .iter()
                    .find(|driver| driver.id == self.driver_id)
                    .map(DriverDraft::from);
                if !preserve {
                    self.driver_draft = self.driver_base.clone();
                }
                if !self
                    .garages
                    .iter()
                    .any(|garage| garage.id == self.target && garage.status != 0)
                {
                    self.target = self
                        .garages
                        .iter()
                        .find(|garage| garage.status != 0)
                        .map(|garage| garage.id.clone())
                        .unwrap_or_default();
                }
                if !self.garages.iter().any(|garage| {
                    garage.id == self.hire_garage
                        && garage.status != 0
                        && garage.drivers.iter().any(|id| value::is_null(id))
                }) {
                    self.hire_garage = self
                        .garages
                        .iter()
                        .find(|garage| {
                            garage.status != 0 && garage.drivers.iter().any(|id| value::is_null(id))
                        })
                        .map(|garage| garage.id.clone())
                        .unwrap_or_default();
                }
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
    fn select_driver(&mut self, id: String) {
        self.driver_id = id;
        self.driver_base = self
            .drivers
            .iter()
            .find(|driver| driver.id == self.driver_id)
            .map(DriverDraft::from);
        self.driver_draft = self.driver_base.clone();
    }
}
fn driver_name(raw: &str) -> String {
    if value::is_null(raw) {
        "—".into()
    } else {
        raw.strip_prefix("driver.")
            .map_or_else(|| title_words(raw), |id| format!("Driver {id}"))
    }
}

impl EditorApp {
    pub(super) fn company(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (index, label) in ["Details", "Garages", "Drivers", "Visited cities"]
                .iter()
                .enumerate()
            {
                ui.selectable_value(&mut self.management.page, index, tr(label));
            }
        });
        ui.add_space(10.0);
        if self.management.page == 0 {
            self.company_details(ui);
            return;
        }
        if let Some(error) = &self.management.error {
            design::notice(ui, tr("Company data unavailable"), error);
            return;
        }
        match self.management.page {
            1 => self.garage_management(ui),
            2 => self.driver_management(ui),
            3 => self.city_management(ui),
            _ => {}
        }
    }
    fn garage_management(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr("Garages"));
        design::caption(
            ui,
            tr(
                "Buy, resize or sell garages. Occupants that no longer fit move to available slots; owned trailers return to headquarters.",
            ),
        );
        ui.horizontal(|ui| {
            ui.label(tr("Find city"));
            ui.text_edit_singleline(&mut self.management.garage_filter);
        });
        let filter = self.management.garage_filter.to_lowercase();
        let rows = self
            .management
            .garages
            .iter()
            .filter(|garage| city_matches(&garage.city, &filter))
            .cloned()
            .collect::<Vec<_>>();
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("Select listed")).clicked() {
                self.management
                    .garage_selection
                    .extend(rows.iter().map(|garage| garage.id.clone()));
            }
            if ui.button(tr("Clear selection")).clicked() {
                self.management.garage_selection.clear();
            }
            ui.label(format!(
                "{} selected",
                self.management.garage_selection.len()
            ));
        });
        ui.horizontal(|ui| {
            ui.add_space(30.0);
            for (width, label) in [
                (160.0, "City"),
                (115.0, "Size"),
                (76.0, "Drivers"),
                (76.0, "Vehicles"),
                (66.0, "Trailers"),
            ] {
                ui.add_sized(
                    [width, 24.0],
                    egui::Label::new(RichText::new(label).strong()),
                );
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("garage_list")
            .max_height(280.0)
            .show_rows(ui, 36.0, rows.len(), |ui, range| {
                for garage in &rows[range] {
                    ui.horizontal(|ui| {
                        let mut selected = self.management.garage_selection.contains(&garage.id);
                        if ui.checkbox(&mut selected, "").changed() {
                            if selected {
                                self.management.garage_selection.insert(garage.id.clone());
                            } else {
                                self.management.garage_selection.remove(&garage.id);
                            }
                        }
                        ui.add_sized(
                            [160.0, 32.0],
                            egui::Label::new(city_label(&garage.city)).truncate(),
                        )
                        .on_hover_text(city_label(&garage.city));
                        ui.add_sized([115.0, 32.0], egui::Label::new(garage.label()));
                        for (width, text) in [
                            (
                                76.0,
                                format!(
                                    "{} / {}",
                                    garage
                                        .drivers
                                        .iter()
                                        .filter(|id| !value::is_null(id))
                                        .count(),
                                    garage.drivers.len()
                                ),
                            ),
                            (
                                76.0,
                                format!(
                                    "{} / {}",
                                    garage
                                        .vehicles
                                        .iter()
                                        .filter(|id| !value::is_null(id))
                                        .count(),
                                    garage.vehicles.len()
                                ),
                            ),
                            (66.0, garage.trailers.len().to_string()),
                        ] {
                            ui.add_sized([width, 32.0], egui::Label::new(text));
                        }
                    });
                }
            });
        let selected = self
            .management
            .garage_selection
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        ui.add_enabled_ui(!selected.is_empty(), |ui| {
            ui.horizontal_wrapped(|ui| {
                for (label, status) in [
                    ("Buy / make small", 2),
                    ("Make large", 3),
                    ("Starting size (HQ)", 6),
                    ("Sell selected", 0),
                ] {
                    if ui.button(label).clicked() {
                        let ids = selected.clone();
                        self.edit_named("Resize garages", move |data| {
                            company::resize_garages(data, &ids, status)
                        });
                    }
                }
            });
        });
        ui.add_space(12.0);
        ui.separator();
        ui.heading(tr("Move occupants"));
        if selected.len() != 1 {
            design::caption(
                ui,
                tr("Select one garage to move or swap its drivers, vehicles and trailers."),
            );
            return;
        }
        let Some(source) = self
            .management
            .garages
            .iter()
            .find(|garage| garage.id == selected[0])
            .cloned()
        else {
            return;
        };
        if source.status == 0 {
            design::caption(ui, tr("Buy this garage before assigning occupants."));
            return;
        }
        if let Some(session) = &self.session {
            egui::Grid::new("garage_slots")
                .striped(true)
                .max_col_width(((ui.available_width() - 24.0) / 3.0).max(40.0))
                .show(ui, |ui| {
                    for label in ["Slot", "Driver", "Vehicle"] {
                        ui.strong(label);
                    }
                    ui.end_row();
                    for (index, (driver, vehicle)) in
                        source.drivers.iter().zip(&source.vehicles).enumerate()
                    {
                        ui.label((index + 1).to_string());
                        ui.label(
                            if session
                                .data
                                .game
                                .unit(driver)
                                .is_some_and(|unit| unit.kind.as_ref() == "driver_player")
                            {
                                session.data.profile_name()
                            } else {
                                driver_name(driver)
                            },
                        );
                        ui.label(if value::is_null(vehicle) {
                            "—".into()
                        } else {
                            editor::vehicle_name(&session.data.game, vehicle, VehicleKind::Truck)
                        });
                        ui.end_row();
                    }
                });
        }
        ui.label(tr("Destination garage"));
        design::combo(ui, "move_garage_target", 260.0)
            .selected_text(city_label(
                self.management.target.trim_start_matches("garage."),
            ))
            .show_ui(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                for garage in self
                    .management
                    .garages
                    .iter()
                    .filter(|garage| garage.status != 0)
                {
                    ui.selectable_value(
                        &mut self.management.target,
                        garage.id.clone(),
                        city_label(&garage.city),
                    );
                }
            });
        let target_capacity = self
            .management
            .garages
            .iter()
            .find(|garage| garage.id == self.management.target)
            .map_or(0, |garage| garage.drivers.len());
        let mut from = self.management.source_slot + 1;
        let mut to = self.management.target_slot + 1;
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("From slot"));
            ui.add(egui::DragValue::new(&mut from).range(1..=source.drivers.len().max(1)));
            ui.label(tr("To slot"));
            ui.add(egui::DragValue::new(&mut to).range(1..=target_capacity.max(1)));
        });
        self.management.source_slot = from.saturating_sub(1);
        self.management.target_slot = to.saturating_sub(1);
        ui.horizontal_wrapped(|ui| {
            for (index, label) in ["Driver + vehicle", "Driver only", "Vehicle only"]
                .iter()
                .enumerate()
            {
                ui.selectable_value(&mut self.management.move_kind, index, *label);
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("Move / swap slot")).clicked() {
                let source = source.id.clone();
                let target = self.management.target.clone();
                let from = self.management.source_slot;
                let to = self.management.target_slot;
                let kind = match self.management.move_kind {
                    1 => MoveKind::Driver,
                    2 => MoveKind::Vehicle,
                    _ => MoveKind::Pair,
                };
                self.edit_named("Move garage occupants", move |data| {
                    company::move_slot(data, &source, from, &target, to, kind)
                });
            }
            if ui
                .add_enabled(
                    !source.trailers.is_empty(),
                    egui::Button::new(format!("Move all {} trailers", source.trailers.len())),
                )
                .clicked()
            {
                let source = source.id.clone();
                let target = self.management.target.clone();
                self.edit_named("Move garage trailers", move |data| {
                    company::move_trailers(data, &source, &target).map(|_| ())
                });
            }
        });
    }
    fn driver_management(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr("Drivers"));
        design::caption(
            ui,
            tr(
                "Hire available drivers into an owned garage, edit their progression or dismiss them.",
            ),
        );
        if ui.add_enabled(
            !self.management.has_drafts()
                && self.management.drivers.iter().any(|driver| driver.status == DriverStatus::Hired),
            egui::Button::new(tr("Max out hired drivers")),
        ).on_hover_text(tr("Set hired employees to level 150, all truck skills and ADR. Higher existing XP is retained. Apply or revert pending driver inputs first.")).clicked() {
            match self.game_data.curve.xp_for_level(150) {
                Ok(target) => self.edit_named("Maximise hired drivers", move |data| company::maximise_hired_drivers_to(data, target).map(|_| ())),
                Err(error) => self.error = Some(error.to_string()),
            }
        }
        ui.horizontal(|ui| {
            ui.label(tr("Find driver / city"));
            ui.text_edit_singleline(&mut self.management.driver_filter);
        });
        let filter = self.management.driver_filter.to_lowercase();
        let rows = self
            .management
            .drivers
            .iter()
            .filter(|driver| {
                format!("{} {}", driver.id, driver.hometown)
                    .to_lowercase()
                    .contains(&filter)
                    || city_matches(&driver.hometown, &filter)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut selected = self.management.driver_id.clone();
        egui::ScrollArea::vertical()
            .id_salt("driver_list")
            .max_height(210.0)
            .show_rows(ui, 44.0, rows.len(), |ui, range| {
                for driver in &rows[range] {
                    ui.horizontal(|ui| {
                        let label = if driver.status == DriverStatus::Player {
                            self.session
                                .as_ref()
                                .map(|session| session.data.profile_name())
                                .unwrap_or_else(|| "You".into())
                        } else {
                            driver_name(&driver.id)
                        };
                        let face =
                            if driver.status == DriverStatus::Player {
                                self.session
                                    .as_ref()
                                    .and_then(|session| {
                                        session.data.profile_id().ok().and_then(|id| {
                                            session.data.profile.string(id, "face").ok()
                                        })
                                    })
                                    .unwrap_or_default()
                            } else {
                                driver.id.clone()
                            };
                        let portrait = self.artwork.game.portrait(ui.ctx(), self.game, &face);
                        let button = if let Some(portrait) = portrait {
                            egui::Button::new((
                                egui::Image::new((portrait.id(), egui::vec2(24.0, 24.0))),
                                label,
                            ))
                            .selected(selected == driver.id)
                        } else {
                            egui::Button::selectable(selected == driver.id, label)
                        };
                        if ui.add_sized([210.0, 42.0], button).clicked() {
                            selected = driver.id.clone();
                        }
                        ui.add_sized(
                            [150.0, 32.0],
                            egui::Label::new(match driver.status {
                                DriverStatus::Player => "Your driver",
                                DriverStatus::Hired => "Hired",
                                DriverStatus::Dismissed => "Dismissal pending",
                                DriverStatus::Available => "Available",
                            }),
                        );
                        ui.label(city_label(&driver.hometown));
                    });
                }
            });
        if selected != self.management.driver_id {
            if self.management.has_drafts() {
                self.error = Some(
                    "Apply or revert this driver's inputs before selecting another driver.".into(),
                );
            } else {
                self.management.select_driver(selected);
            }
        }
        let Some(driver) = self
            .management
            .drivers
            .iter()
            .find(|driver| driver.id == self.management.driver_id)
            .cloned()
        else {
            return;
        };
        if driver.status == DriverStatus::Player {
            ui.label(tr("Edit your driver's skills in Profile."));
            return;
        }
        ui.separator();
        ui.strong(driver_name(&driver.id));
        if let Some(draft) = &mut self.management.driver_draft {
            ui.horizontal(|ui| {
                ui.label(tr("Experience points"));
                ui.add(egui::DragValue::new(&mut draft.xp).range(0..=i32::MAX as u32));
                ui.label(format!(
                    "Level {}",
                    self.game_data.curve.level_for_xp(draft.xp)
                ));
            });
            skills_ui::editor(
                ui,
                self.game,
                &self.artwork.game,
                &mut draft.skills,
                &mut draft.adr,
                "employee_skills",
            );
            if ui.button(tr("Max out driver"))
                .on_hover_text(tr("Fill level 150, all truck skills and ADR, then choose Apply driver progression."))
                .clicked() {
                if let Ok(target) = self.game_data.curve.xp_for_level(150) {
                    draft.xp = draft.xp.max(target);
                }
                draft.skills = [6; 5];
                draft.adr = 63;
            }
        }
        if design::primary(
            ui,
            tr("Apply driver progression"),
            self.management.has_drafts(),
        )
        .clicked()
            && let Some(draft) = self.management.driver_draft.clone()
        {
            let id = driver.id.clone();
            self.edit(move |data| {
                company::set_driver(data, &id, draft.xp, draft.adr, draft.skills, None)
            });
        }
        ui.add_space(8.0);
        if driver.status == DriverStatus::Hired {
            design::caption(
                ui,
                tr(
                    "Dismissal preserves an active delivery until it finishes. The assigned vehicle stays in your garage.",
                ),
            );
            if ui.button(tr("Dismiss driver")).clicked() {
                let id = driver.id.clone();
                self.edit_named("Dismiss driver", move |data| company::fire(data, &id));
            }
        } else {
            ui.label(tr("Hire into garage"));
            design::combo(ui, "hire_into_garage", 260.0)
                .selected_text(city_label(
                    self.management.hire_garage.trim_start_matches("garage."),
                ))
                .show_ui(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    for garage in self.management.garages.iter().filter(|garage| {
                        garage.status != 0 && garage.drivers.iter().any(|id| value::is_null(id))
                    }) {
                        ui.selectable_value(
                            &mut self.management.hire_garage,
                            garage.id.clone(),
                            city_label(&garage.city),
                        );
                    }
                });
            if ui
                .add_enabled(
                    !self.management.hire_garage.is_empty(),
                    egui::Button::new(if driver.status == DriverStatus::Dismissed {
                        "Cancel dismissal / rehire"
                    } else {
                        "Hire driver"
                    }),
                )
                .clicked()
            {
                let id = driver.id;
                let target = self.management.hire_garage.clone();
                self.edit_named("Hire driver", move |data| company::hire(data, &id, &target));
            }
        }
    }
    fn city_management(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr("Visited cities"));
        design::caption(
            ui,
            tr("Cities referenced by this save. Visit counts for other cities are retained."),
        );
        ui.horizontal(|ui| {
            ui.label(tr("Find city"));
            ui.text_edit_singleline(&mut self.management.city_filter);
        });
        let filter = self.management.city_filter.to_lowercase();
        let rows = self
            .management
            .cities
            .iter()
            .filter(|(city, _)| city_matches(city, &filter))
            .cloned()
            .collect::<Vec<_>>();
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("Select listed")).clicked() {
                self.management
                    .city_selection
                    .extend(rows.iter().map(|(city, _)| city.clone()));
            }
            if ui.button(tr("Clear selection")).clicked() {
                self.management.city_selection.clear();
            }
            ui.label(format!("{} selected", self.management.city_selection.len()));
        });
        egui::ScrollArea::vertical()
            .id_salt("visited_city_list")
            .max_height(360.0)
            .show_rows(ui, 34.0, rows.len(), |ui, range| {
                for (city, count) in &rows[range] {
                    ui.horizontal(|ui| {
                        let mut selected = self.management.city_selection.contains(city);
                        if ui.checkbox(&mut selected, "").changed() {
                            if selected {
                                self.management.city_selection.insert(city.clone());
                            } else {
                                self.management.city_selection.remove(city);
                            }
                        }
                        ui.add_sized([250.0, 30.0], egui::Label::new(city_label(city)));
                        ui.label(if *count == 0 {
                            "Not visited".into()
                        } else {
                            format!("Visited · {count} visits")
                        });
                    });
                }
            });
        let ids = self
            .management
            .city_selection
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        ui.add_enabled_ui(!ids.is_empty(), |ui| {
            ui.horizontal(|ui| {
                for (label, visited) in [("Mark visited", true), ("Mark unvisited", false)] {
                    if ui.button(label).clicked() {
                        let ids = ids.clone();
                        self.edit_named("Change visited cities", move |data| {
                            company::set_visited(data, &ids, visited)
                        });
                    }
                }
            });
        });
    }
}
