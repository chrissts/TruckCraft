use super::*;
use ts_save_core::vehicle;

#[derive(Default)]
pub(super) struct VehicleTools {
    condition: bool,
    paint: bool,
    parts: bool,
    sharing: String,
}

fn paint_label(field: &str) -> &str {
    match field {
        "base_color" => "Base colour",
        "mask_r_color" => "Design colour 1",
        "mask_g_color" => "Design colour 2",
        "mask_b_color" => "Design colour 3",
        "flake_color" => "Metallic flakes",
        "flip_color" => "Pearlescent colour",
        _ => field,
    }
}

impl EditorApp {
    pub(super) fn vehicle_details(&mut self, ui: &mut egui::Ui, kind: VehicleKind, id: &str) {
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("Component condition…")).clicked() {
                self.vehicle_tools.condition = true;
            }
            if ui.button(tr("Paint colours…")).clicked() {
                self.vehicle_tools.paint = true;
            }
            if ui.button(tr("Parts…")).clicked() {
                self.vehicle_tools.parts = true;
            }
        });
        if self.vehicle_tools.condition {
            let mut open = true;
            egui::Window::new(tr("Component condition")).id(egui::Id::unique("Component condition")).open(&mut open).collapsible(false).default_width(480.0)
                .show(ui.ctx(), |ui| {
                    ui.label(tr("Wear: 0% is undamaged. Changes affect the selected vehicle only; mileage is retained."));
                    if self.input.condition.is_empty() { ui.label(tr("No editable condition values were found in this save.")); }
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        for wear in &mut self.input.condition {
                            let label = if let Some(wheel) = wear.wheel {
                                format!("Wheel {}{}", wheel+1, if wear.field.ends_with("unfixable") { " permanent wear" } else { " wear" })
                            } else { human(&wear.field.replace("_unfixable", " permanent")) };
                            ui.label(label);
                            let mut percent = wear.fraction * 100.0;
                            if design::percent_slider(ui, &mut percent).changed() { wear.fraction = percent/100.0; }
                        }
                    });
                    if ui.add_enabled(!self.input.condition.is_empty(),egui::Button::new(tr("Apply condition"))).clicked() {
                        let edits = self.input.condition.clone();
                        self.edit_named("Change component condition", |data| vehicle::set_condition(data,kind,id,&edits));
                    }
                });
            self.vehicle_tools.condition = open;
        }
        if self.vehicle_tools.paint {
            let mut open = true;
            egui::Window::new(tr("Vehicle paint colours")).id(egui::Id::unique("Vehicle paint colours")).open(&mut open).collapsible(false).default_width(480.0)
                .show(ui.ctx(), |ui| {
                    ui.label(tr("Choose colours for the existing paint design. Paint design, materials and other saved settings are retained."));
                    if self.input.paint.is_empty() { ui.label(tr("No editable paint colours were found in this vehicle's saved accessories.")); }
                    egui::ScrollArea::vertical().max_height(250.0).show(ui, |ui| {
                        for color in &mut self.input.paint {
                            ui.horizontal_wrapped(|ui| {
                                let mut selected = color.linear;
                                if ui.color_edit_button_rgb(&mut selected).changed() { color.linear = selected; }
                                ui.label(paint_label(&color.field));
                            });
                        }
                    });
                    if ui.add_enabled(!self.input.paint.is_empty(),egui::Button::new(tr("Apply paint colours"))).clicked() {
                        let edits = self.input.paint.clone();
                        self.edit_named("Change vehicle paint colours", |data| vehicle::set_paint(data,kind,id,&edits));
                    }
                    ui.separator();
                    ui.label(tr("Share colours with another vehicle"));
                    ui.label(tr("Copy sharing data below, or paste it and choose Import. Import fills these pickers; Apply stages the edit."));
                    ui.add(egui::TextEdit::multiline(&mut self.vehicle_tools.sharing).char_limit(4096).desired_rows(3).desired_width(f32::INFINITY));
                    let accessory = self.input.paint.first().map(|c|c.accessory.clone());
                    ui.horizontal_wrapped(|ui| {
                        if ui.add_enabled(accessory.is_some(),egui::Button::new(tr("Copy paint colours"))).clicked() {
                            match vehicle::export_paint(&self.input.paint,accessory.as_deref().unwrap()) {
                                Ok(text) => { ui.ctx().copy_text(text.clone()); self.vehicle_tools.sharing = text; },
                                Err(error) => self.error = Some(error.to_string()),
                            }
                        }
                        if ui.add_enabled(accessory.is_some(),egui::Button::new(tr("Import paint colours"))).clicked()
                            && let Err(error) = vehicle::import_paint(&self.vehicle_tools.sharing,&mut self.input.paint,accessory.as_deref().unwrap()) {
                                self.error = Some(error.to_string());
                            }
                    });
                });
            self.vehicle_tools.paint = open;
        }
        if self.vehicle_tools.parts {
            let mut open = true;
            egui::Window::new(tr("Saved vehicle parts")).id(egui::Id::unique("Saved vehicle parts")).open(&mut open).collapsible(false).default_width(520.0)
                .show(ui.ctx(), |ui| {
                    ui.label(tr("Addon choices come from saved parts for the same vehicle model and slot. Structural parts are shown for reference; their replacement needs a compatible game-data catalog."));
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        for part in &mut self.input.parts {
                            ui.label(human(part.kind.trim_start_matches("vehicle_").trim_end_matches("_accessory")));
                            let choices = self.session.as_ref().map(|session|vehicle::part_choices(&session.data,part)).unwrap_or_default();
                            if part.kind == "vehicle_addon_accessory" && choices.len() > 1 {
                                design::combo(ui,&part.id,440.0).selected_text(title_words(part.path.rsplit('/').next().unwrap_or(&part.path).trim_end_matches(".sii"))).show_ui(ui, |ui| {
                                    for path in choices {
                                        let label = title_words(path.rsplit('/').next().unwrap_or(&path).trim_end_matches(".sii"));
                                        ui.selectable_value(&mut part.path,path,label);
                                    }
                                });
                            } else { ui.label(part.path.rsplit('/').next().unwrap_or("Unknown part").trim_end_matches(".sii")); }
                            ui.separator();
                        }
                    });
                    if ui.button(tr("Apply parts")).clicked() {
                        let edits = self.input.parts.clone();
                        self.edit_named("Change vehicle addons", |data| vehicle::set_parts(data,kind,id,&edits));
                    }
                });
            self.vehicle_tools.parts = open;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_render_without_editing_and_keep_drafts_across_other_edits() {
        let context = egui::Context::default();
        let mut app = EditorApp::new(&eframe::CreationContext::_new_kittest(context.clone()));
        let temporary = tempfile::tempdir().unwrap();
        let save = temporary.path().join("profile/save/1");
        std::fs::create_dir_all(&save).unwrap();
        std::fs::write(temporary.path().join("profile/profile.sii"),"SiiNunit\n{\nuser_profile : profile.test {\n profile_name: \"Example\"\n face: 1\n logo: logo_0\n male: true\n}\n}\n").unwrap();
        std::fs::write(
            save.join("info.sii"),
            "SiiNunit\n{\nsave_container : info.test {\n version: 105\n}\n}\n",
        )
        .unwrap();
        std::fs::write(save.join("game.sii"),"SiiNunit\n{\neconomy : economy.test {\n player: player.test\n bank: bank.test\n}\nbank : bank.test {\n money_account: 100\n}\nplayer : player.test {\n trucks: 1\n trucks[0]: truck.test\n}\nvehicle : truck.test {\n fuel_relative: 0.5\n engine_wear: 0.125\n accessories: 1\n accessories[0]: paint.test\n future_field: 42\n}\nvehicle_paint_job_accessory : paint.test {\n base_color: (0.1234567, 0.3456789, 0.7654321)\n data_path: \"/def/vehicle/truck/example/paint_job/plain.sii\"\n}\n}\n").unwrap();
        app.session = Some(SaveSession::load(save.join("game.sii"), Game::Ets2).unwrap());
        app.tab = 2;
        app.refresh();
        app.vehicle_tools.condition = true;
        app.vehicle_tools.paint = true;
        app.vehicle_tools.parts = true;
        let original = app
            .session
            .as_ref()
            .unwrap()
            .data
            .game
            .render()
            .unwrap()
            .into_owned();
        let paint = app.input.paint.clone();
        for _ in 0..4 {
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(680.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ui| app.vehicle_details(ui, VehicleKind::Truck, "truck.test"),
            );
            output.textures_delta.clear();
        }
        assert_eq!(
            app.input.paint, paint,
            "displaying paint pickers must preserve channel values exactly"
        );
        assert!(!app.has_drafts());
        assert_eq!(
            app.session.as_ref().unwrap().data.game.render().unwrap(),
            original
        );
        app.input.condition[0].fraction = 0.75;
        app.input.paint[0].linear = [0.1, 0.2, 0.3];
        app.input.identity.as_mut().unwrap().male = false;
        app.edit(|data| data.game.set("truck.test", "future_field", "43"));
        assert_eq!(app.input.condition[0].fraction, 0.75);
        assert_eq!(app.input.paint[0].linear, [0.1, 0.2, 0.3]);
        assert!(!app.input.identity.as_ref().unwrap().male);
        assert!(app.has_drafts());
        assert!(app.vehicle_draft());
    }
}
