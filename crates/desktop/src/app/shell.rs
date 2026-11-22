use super::*;
use design::{ACCENT, BORDER, CANVAS, MUTED, NAVY, TINT, WHITE};

fn tab_bar(
    ui: &mut egui::Ui,
    tabs: [(&str, &egui::TextureHandle); 8],
    active: usize,
) -> Option<usize> {
    const GAP: f32 = 6.0;
    const ICON_SIZE: f32 = 34.0;
    const PADDING: f32 = 6.0;
    let count = tabs.len() as f32;
    let width = ((ui.available_width() - GAP * (count - 1.0)) / count).floor();
    let captions = tabs.map(|(name, _)| {
        let mut job = egui::text::LayoutJob::simple(
            tr(name).to_owned(),
            egui::FontId::proportional(14.0),
            Color32::PLACEHOLDER,
            (width - 4.0).max(1.0),
        );
        job.halign = egui::Align::Center;
        ui.painter().layout_job(job)
    });
    let caption_height = captions.iter().map(|g| g.size().y).fold(0.0, f32::max);
    let height = ICON_SIZE + caption_height + 3.0 * PADDING;
    let mut clicked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        for (index, galley) in captions.into_iter().enumerate() {
            let (name, icon) = tabs[index];
            let selected = active == index;
            let response = ui.add_sized(
                [width, height],
                egui::Button::selectable(selected, "").fill(if selected {
                    TINT
                } else {
                    Color32::TRANSPARENT
                }),
            );
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::Role::Tab, ui.is_enabled(), selected, tr(name))
            });
            let rect = response.rect;
            let size = icon.size_vec2();
            let center = egui::pos2(rect.center().x, rect.top() + PADDING + ICON_SIZE / 2.0);
            ui.painter().image(
                icon.id(),
                egui::Rect::from_center_size(center, size * (ICON_SIZE / size.max_elem())),
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                WHITE,
            );
            let caption_center = egui::pos2(
                rect.center().x,
                rect.bottom() - PADDING - caption_height / 2.0,
            );
            ui.painter().galley(
                caption_center - galley.rect.center().to_vec2(),
                galley,
                if selected { ACCENT } else { MUTED },
            );
            if response.clicked() && !selected {
                clicked = Some(index);
            }
        }
    });
    clicked
}

impl EditorApp {
    fn header(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("menu")
            .frame(
                egui::Frame::new()
                    .fill(NAVY)
                    .inner_margin(egui::Margin::symmetric(22, 12)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.image((self.artwork.mark.id(), egui::vec2(48.0, 48.0)));
                    ui.label(RichText::new(APP_NAME).size(26.0).strong().color(WHITE));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new(tr("About")).color(WHITE))
                                    .fill(Color32::from_rgb(37, 60, 81)),
                            )
                            .clicked()
                        {
                            self.about = true;
                        }
                        if ui
                            .add(
                                egui::Button::new(RichText::new(tr("Settings")).color(WHITE))
                                    .fill(Color32::from_rgb(37, 60, 81)),
                            )
                            .clicked()
                        {
                            self.activity.open = true;
                        }
                        let checking_manually =
                            self.checking_release == Some(updates::CheckOrigin::Manual);
                        if ui
                            .add_enabled(
                                !checking_manually,
                                egui::Button::new(
                                    RichText::new(tr(if checking_manually {
                                        "Checking for updates…"
                                    } else {
                                        "Check for updates"
                                    }))
                                    .color(WHITE),
                                )
                                .fill(Color32::from_rgb(37, 60, 81)),
                            )
                            .clicked()
                        {
                            self.check_release(ui.ctx());
                        }
                        ui.label(
                            RichText::new(display_version())
                                .color(Color32::from_rgb(181, 203, 221))
                                .size(14.0),
                        );
                    });
                });
            });
    }

    pub(super) fn tabs(&mut self, ui: &mut egui::Ui) {
        if let Some(index) = tab_bar(ui, self.artwork.tabs(self.game), self.tab) {
            if self.vehicle_draft() {
                self.error =
                    Some("Apply or revert the vehicle inputs before switching tabs.".into());
            } else {
                self.tab = index;
                self.input.vehicle_index = 0;
                self.refresh_vehicle();
            }
        }
        ui.add_space(12.0);
    }

    pub(super) fn empty(&mut self, ui: &mut egui::Ui) {
        design::card(ui, |ui| {
            let journey_art = match self.game {
                Game::Ets2 => &self.artwork.coach,
                Game::Ats => &self.artwork.car,
            };
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.image((self.artwork.truck.id(), egui::vec2(160.0, 150.0)));
                ui.image((journey_art.id(), egui::vec2(180.0, 150.0)));
                ui.image((self.artwork.trailer.id(), egui::vec2(180.0, 120.0)));
            });
            ui.add_space(12.0);
            ui.heading(tr("Choose a save to start editing"));
            ui.add_space(12.0);
            if self.isolated {
                ui.label(tr("Choose ETS2 or ATS, then open a game.sii save."));
            } else {
                ui.label(tr("1. Choose ETS2 or ATS on the right."));
                ui.label(tr(
                    "2. Select a profile and save, then choose Load selected save.",
                ));
                ui.label(tr("3. Apply your edits and save when you are ready."));
            }
            ui.add_space(12.0);
            if design::primary(ui, tr("Open a save file…"), true).clicked() {
                self.open_dialog(ui.ctx());
            }
            if ui.button(tr("Recover from backup…")).clicked() {
                self.pick_recovery_slot();
            }
            self.starter_button(ui);
            ui.add_space(14.0);
        });
        ui.add_space(16.0);
        design::notice(
            ui,
            tr("Made for your local saves"),
            tr(
                "Close the game before editing. Save creates a recovery copy, and you should also keep an independent backup.",
            ),
        );
    }

    fn dialogs(&mut self, context: &egui::Context) {
        self.release_dialog(context);
        self.review_dialog(context);
        if let Some(message) = self.error.clone() {
            egui::Window::new(tr("Message"))
                .id(egui::Id::unique("Message"))
                .collapsible(false)
                .resizable(false)
                .default_width(520.0)
                .show(context, |ui| {
                    ui.add_space(8.0);
                    ui.label(message);
                    if let Some(slot) = self.failed_slot.clone()
                        && ui.button(tr("Recover from backup…")).clicked()
                    {
                        self.error = None;
                        self.show_copy_recovery(slot);
                    }
                    ui.add_space(12.0);
                    if design::primary(ui, tr("OK"), true).clicked() {
                        self.error = None;
                    }
                });
        }
        if self.pending.is_some() && !self.busy {
            let response =
                egui::Modal::new(egui::Id::unique("Unsaved changes")).show(context, |ui| {
                    ui.set_width(440.0);
                    ui.heading(tr("Unsaved changes"));
                    ui.label(tr("You have staged changes or unapplied inputs."));
                    design::caption(ui, tr("Discard them to continue, or keep editing."));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if design::primary(ui, tr("Keep editing"), true).clicked() {
                            self.pending = None;
                        }
                        if ui.button(tr("Discard changes")).clicked()
                            && let Some(action) = self.pending.take()
                        {
                            match action {
                                Pending::Open(path, game) => {
                                    self.begin_load(context.clone(), path, game)
                                }
                                Pending::SwitchGame(game) => self.switch_game(game),
                                Pending::Close => {
                                    self.allow_close = true;
                                    context.send_viewport_cmd(egui::ViewportCommand::Close);
                                }
                            }
                        }
                    });
                });
            if response.should_close() {
                self.pending = None;
            }
        }
        if self.about {
            egui::Window::new(tr("About TruckCraft")).id(egui::Id::unique("About TruckCraft"))
                .open(&mut self.about)
                .collapsible(false)
                .resizable(false)
                .default_width(560.0)
                .show(context, |ui| {
                    ui.horizontal(|ui| {
                        ui.image((self.artwork.mark.id(), egui::vec2(84.0, 84.0)));
                        ui.vertical(|ui| {
                            ui.heading(APP_NAME);
                            design::badge(ui, display_version(), ACCENT, TINT);
                        });
                    });
                    ui.add_space(8.0);
                    ui.label(tr("For ETS2 and ATS 1.62.x"));
                    let format = self.session.as_ref().and_then(|session| session.data.format_version().ok());
                    ui.label(match format {
                        Some(version) => format!("{} {version}", tr("Loaded save format")),
                        None => format!("{} {SAVE_FORMAT_TARGET} · {}", tr("Target save format"), tr("No save loaded")),
                    });
                    ui.add_space(12.0);
                    ui.strong(tr("Community support"));
                    ui.hyperlink_to(tr("Raise an issue or submit a PR with a fix"), GITHUB);
                    ui.hyperlink_to(tr("Original TS SE Tool by LIPtoH"), "https://github.com/LIPtoH/TS-SE-Tool");
                    ui.add_space(12.0);
                    ui.separator();
                    design::caption(ui, tr("TruckCraft is a Rust remodel of TS SE Tool. The application uses Apache-2.0. The adapted scs_tools decoder retains its MIT and Apache-2.0 notices."));
                    design::caption(ui, tr("ETS2 and ATS names and logos belong to SCS Software. TruckCraft is an independent community project."));
                });
        }
    }
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        crate::i18n::select(self.browser.language());
        let context = ui.ctx().clone();
        self.poll(&context);
        self.discover_if_needed(&context);
        if context.input(|input| input.viewport().close_requested()) && !self.allow_close {
            if self.saving || self.profile_tools.running {
                context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.status = "Finishing the file operation before closing…".into();
            } else if self.unsaved() {
                context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending = Some(Pending::Close);
            }
        }
        self.header(ui);
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(WHITE)
                    .stroke(egui::Stroke::new(1.0, BORDER))
                    .inner_margin(egui::Margin::symmetric(22, 9)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("●").color(if self.unsaved() {
                        Color32::from_rgb(170, 111, 28)
                    } else {
                        ACCENT
                    }));
                    ui.label(RichText::new(tr(&self.status)).size(13.0).color(MUTED));
                });
            });
        egui::Panel::right("save_browser")
            .default_size(314.0)
            .min_size(290.0)
            .max_size(380.0)
            .resizable(true)
            .frame(egui::Frame::new().fill(WHITE).inner_margin(20))
            .show(ui, |ui| {
                egui::Panel::bottom("save_actions")
                    .frame(egui::Frame::new().fill(WHITE).inner_margin(egui::Margin {
                        top: 12,
                        ..Default::default()
                    }))
                    .show(ui, |ui| {
                        self.save_actions(ui);
                    });
                egui::ScrollArea::vertical()
                    .id_salt("sidebar_scroll")
                    .show(ui, |ui| {
                        self.sidebar(ui);
                    });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(CANVAS).inner_margin(22))
            .show(ui, |ui| {
                self.tabs(ui);
                if self.busy {
                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.spinner();
                            ui.label(tr(if self.saving {
                                "Saving your changes…"
                            } else if self.profile_tools.running {
                                "Copying profile files…"
                            } else {
                                "Loading your save…"
                            }));
                        });
                    });
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt((
                        "editor_scroll",
                        self.game.name(),
                        self.tab,
                        self.session.as_ref().map(SaveSession::save_folder),
                    ))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.session.is_none() {
                            self.empty(ui);
                            return;
                        }
                        design::card(ui, |ui| match self.tab {
                            0 => self.profile(ui),
                            1 => self.company(ui),
                            2 | 4 => self.vehicle(ui),
                            3 if self.game == Game::Ats => self.car(ui),
                            3 => self.coach(ui),
                            5 => self.freight(ui),
                            6 => self.cargo(ui),
                            7 => self.convoy(ui),
                            _ => {}
                        });
                        ui.add_space(16.0);
                        design::caption(
                            ui,
                            tr("ETS2 and ATS 1.62.x · Keep a backup before editing."),
                        );
                    });
            });
        self.dialogs(&context);
        self.profile_tools_dialog(&context);
        self.starter_dialog(&context);
        self.settings_dialog(&context);
        self.missing_save_dialog(&context);
        self.application_settings(&context);
        self.activity.observe(&self.status, &self.error);
        #[cfg(windows)]
        if let Some(confirmation) = self.update_confirmation.take() {
            confirmation.confirm();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translated_tab_captions_fit_below_their_icons() {
        let context = egui::Context::default();
        design::configure(&context);
        let art = Artwork::load(&context);
        for scale in [1.0, 1.5] {
            context.set_pixels_per_point(scale);
            for game in [Game::Ets2, Game::Ats] {
                let tabs = art.tabs(game);
                for &locale in crate::i18n::languages().keys() {
                    crate::i18n::select(locale);
                    let mut row = egui::Rect::NOTHING;
                    let mut output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(680.0, 720.0),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            row = ui.scope(|ui| tab_bar(ui, tabs, 3)).response.rect;
                        },
                    );
                    output.textures_delta.clear();
                    for (name, icon) in tabs {
                        let caption = output
                            .shapes
                            .iter()
                            .find_map(|shape| match &shape.shape {
                                egui::Shape::Text(text) if text.galley.text() == tr(name) => {
                                    Some(text)
                                }
                                _ => None,
                            })
                            .expect("tab caption");
                        let image = output
                            .shapes
                            .iter()
                            .find_map(|shape| match &shape.shape {
                                egui::Shape::Mesh(mesh) if mesh.texture_id == icon.id() => {
                                    Some(mesh.calc_bounds())
                                }
                                _ => None,
                            })
                            .expect("tab icon");
                        let bounds = caption.galley.rect.translate(caption.pos.to_vec2());
                        assert!(row.contains_rect(bounds), "{locale} {name} outside row");
                        assert!(
                            bounds.top() >= image.bottom(),
                            "{locale} {name} overlaps icon"
                        );
                        assert!((bounds.center().x - image.center().x).abs() < 1.0);
                        assert!(bounds.width() <= row.width() / tabs.len() as f32);
                        assert!(caption.galley.job.sections[0].format.font_id.size >= 14.0);
                    }
                }
            }
        }
        crate::i18n::select("en");
    }
}
