use super::*;
use std::collections::BTreeSet;
use ts_save_core::plates::{Part, Plate, Tag};

#[derive(Default)]
pub(super) struct PlateTools {
    layout: bool,
    tool: usize,
    before: usize,
    background: Option<((u64, String, VehicleKind), egui::TextureHandle)>,
}

impl PlateTools {
    pub(super) fn clear_preview(&mut self) {
        self.background = None;
    }
}

fn label(raw: &str) -> String {
    raw.split('/')
        .filter(|part| !part.is_empty())
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|part| title_words(part.trim_end_matches(".mat")))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn region_label(region: &str) -> String {
    match region {
        "uk" => "United Kingdom".into(),
        _ => title_words(region),
    }
}

fn preview(
    ui: &mut egui::Ui,
    plate: &Plate,
    texture: Option<&egui::TextureHandle>,
    game_data_loaded: bool,
) {
    let region = plate.country.as_deref().unwrap_or_default();
    let germany = region == "germany";
    let number = plate.text();
    let [r, g, b, a] = plate.first_color();
    let color = Color32::from_rgba_unmultiplied(r, g, b, a);
    let caption = if texture.is_some() {
        tr("Game plate background. Text layout is approximate.")
    } else if germany {
        tr("Simplified German plate preview.")
    } else if game_data_loaded {
        tr("Text preview. The game applies its own plate artwork and layout.")
    } else {
        tr("Text preview. Load game data to see regional plate backgrounds.")
    };
    let aspect = texture.map_or(4.0, |image| image.aspect_ratio());
    let width = ui.available_width().min(288.0);
    let height = (width / aspect).clamp(48.0, 134.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter();
    if let Some(image) = texture {
        painter.image(
            image.id(),
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        painter.rect_filled(rect, 4, Color32::from_rgb(250, 250, 246));
        if germany {
            let band = egui::Rect::from_min_size(rect.min, egui::vec2(width * 0.13, height));
            painter.rect_filled(
                band,
                egui::CornerRadius {
                    nw: 4,
                    sw: 4,
                    ..Default::default()
                },
                Color32::from_rgb(0, 54, 142),
            );
            painter.text(
                egui::pos2(band.center().x, band.min.y + height * 0.68),
                egui::Align2::CENTER_CENTER,
                "D",
                egui::FontId::proportional(17.0),
                Color32::WHITE,
            );
        }
        painter.rect_stroke(
            rect,
            4,
            egui::Stroke::new(1.0, design::INK),
            egui::StrokeKind::Inside,
        );
    }
    let mut text_area = rect.shrink2(egui::vec2(10.0, 3.0));
    if germany {
        text_area.min.x = text_area.min.x.max(rect.min.x + width * 0.16);
    }
    let mut job = egui::text::LayoutJob::simple(
        number.clone(),
        egui::FontId::monospace((height * 0.42).clamp(14.0, 28.0)),
        color,
        text_area.width(),
    );
    job.halign = egui::Align::Center;
    job.wrap.max_rows = 1;
    let text = painter.layout_job(job);
    painter.with_clip_rect(text_area).galley(
        text_area.center() - text.rect.center().to_vec2(),
        text,
        color,
    );
    response.on_hover_text(number);
    ui.small(caption);
}

fn numeric(
    ui: &mut egui::Ui,
    tag: &mut Tag,
    key: &str,
    title: &str,
    default: f32,
    range: std::ops::RangeInclusive<f32>,
) -> Result<()> {
    let mut number = tag.number(key, default);
    ui.label(title);
    if ui
        .add(
            egui::DragValue::new(&mut number)
                .speed(0.05)
                .range(range)
                .clamp_existing_to_range(false),
        )
        .changed()
    {
        tag.set(key, &number.to_string())?;
    }
    Ok(())
}

fn controls(ui: &mut egui::Ui, tag: &mut Tag, badges: &[String]) -> Result<()> {
    let name = tag.name().to_owned();
    match name.as_str() {
        "color" => {
            ui.label(tr("Text colour"));
            let [r, g, b, a] = tag.color().unwrap_or([0, 0, 0, 255]);
            let mut color = Color32::from_rgba_unmultiplied(r, g, b, a);
            if ui
                .color_edit_button_srgba(&mut color)
                .on_hover_text(tr("Choose text colour"))
                .changed()
            {
                tag.set(
                    "value",
                    &format!("{:08X}", u32::from_le_bytes(color.to_srgba_unmultiplied())),
                )?;
            }
        }
        "font" => {
            ui.strong(tr("Font scale"));
            numeric(ui, tag, "xscale", "Width", 1.0, 0.1..=4.0)?;
            numeric(ui, tag, "yscale", "Height", 1.0, 0.1..=4.0)?;
        }
        "offset" => {
            ui.strong(tr("Position"));
            numeric(ui, tag, "hshift", "Horizontal", 0.0, -4096.0..=4096.0)?;
            numeric(ui, tag, "vshift", "Vertical", 0.0, -4096.0..=4096.0)?;
        }
        "img" => {
            ui.strong(tr("Badge"));
            let mut source = tag.get("src").unwrap_or_default().to_owned();
            design::combo(ui, "plate_badge", 220.0)
                .selected_text(label(&source))
                .show_ui(ui, |ui| {
                    for badge in badges {
                        ui.selectable_value(&mut source, badge.clone(), label(badge));
                    }
                });
            if Some(source.as_str()) != tag.get("src") {
                tag.set("src", &source)?;
            }
            numeric(ui, tag, "width", "Width", 10.0, 1.0..=128.0)?;
            numeric(ui, tag, "height", "Height", 15.0, 1.0..=128.0)?;
        }
        "align" => {
            // Preserve the existing side and any additional alignment attributes.
            let side = if tag.get("right").is_some() {
                "right"
            } else {
                "left"
            };
            ui.strong(if side == "left" {
                "Align from left"
            } else {
                "Align from right"
            });
            numeric(ui, tag, side, "Inset", 0.0, -4096.0..=4096.0)?;
        }
        "/font" => {
            ui.label(tr("End font scale"));
        }
        "/align" => {
            ui.label(tr("End alignment"));
        }
        "sup" => {
            ui.label(tr("Start raised text"));
        }
        "/sup" => {
            ui.label(tr("End raised text"));
        }
        "sub" => {
            ui.label(tr("Start lowered text"));
        }
        "/sub" => {
            ui.label(tr("End lowered text"));
        }
        "ret" => {
            ui.label(tr("Restart text position / next column"));
        }
        _ => {
            ui.label(tr("Additional game formatting (retained)"));
        }
    }
    Ok(())
}

fn insert(plate: &mut Plate, at: usize, tool: usize, badges: &[String]) -> Result<()> {
    let mut tag = Tag::create(match tool {
        0 => "color",
        1 => "font",
        2 => "offset",
        3 => "align",
        4 => "align",
        5 => "sup",
        6 => "sub",
        7 => "ret",
        8 => "img",
        10 => "/align",
        _ => {
            return Err(Error::InvalidEdit(
                "Choose a plate formatting control.".into(),
            ));
        }
    })?;
    match tool {
        0 => tag.set("value", "FF000000")?,
        1 => {
            tag.set("xscale", "1")?;
            tag.set("yscale", "1")?;
        }
        2 => {
            tag.set("hshift", "0")?;
            tag.set("vshift", "0")?;
        }
        3 => tag.set("left", "0")?,
        4 => tag.set("right", "0")?,
        8 => {
            tag.set(
                "src",
                badges.first().ok_or_else(|| {
                    Error::InvalidEdit(
                        "Load installed game data or a plate with a badge to choose an image."
                            .into(),
                    )
                })?,
            )?;
            tag.set("width", "10")?;
            tag.set("height", "15")?;
        }
        _ => {}
    }
    let at = at.min(plate.parts.len());
    if matches!(tool, 1 | 5 | 6) {
        let close = Tag::create(match tool {
            1 => "/font",
            5 => "/sup",
            _ => "/sub",
        })?;
        if let Some(text) = plate
            .parts
            .iter()
            .enumerate()
            .skip(at)
            .find_map(|(i, part)| matches!(part, Part::Text(_)).then_some(i))
        {
            plate.parts.insert(text + 1, Part::Tag(close));
        } else {
            plate.parts.push(Part::Text("Text".into()));
            plate.parts.push(Part::Tag(close));
        }
    }
    plate.parts.insert(at, Part::Tag(tag));
    Ok(())
}

impl EditorApp {
    pub(super) fn plate_controls(&mut self, ui: &mut egui::Ui, kind: VehicleKind) {
        ui.heading(tr("Number plate"));
        let mut plate = match Plate::parse(&self.input.plate) {
            Ok(plate) => plate,
            Err(error) => {
                ui.label(error.to_string());
                return;
            }
        };
        let before = plate.clone();
        let mut number = plate.text();
        let sections = plate
            .parts
            .iter()
            .filter(|part| matches!(part, Part::Text(_)))
            .count();
        ui.label(tr("Plate text"));
        if ui
            .add_enabled(
                sections <= 1,
                egui::TextEdit::singleline(&mut number)
                    .char_limit(128)
                    .desired_width(350.0),
            )
            .changed()
            && let Err(error) = plate.set_text(&number)
        {
            self.error = Some(error.to_string());
        }
        if sections > 1 {
            ui.small(tr(
                "This plate has several text sections. Edit them in Layout and badges.",
            ));
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("Text colour"));
            let [r, g, b, a] = plate.first_color();
            let mut color = Color32::from_rgba_unmultiplied(r, g, b, a);
            if ui
                .color_edit_button_srgba(&mut color)
                .on_hover_text(tr("Choose plate text colour"))
                .changed()
                && let Err(error) = plate.set_color(color.to_srgba_unmultiplied())
            {
                self.error = Some(error.to_string());
            }
            if ui.button(tr("Layout and badges…")).clicked() {
                self.plate_tools.layout = true;
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("Country / state"));
            let mut regions = self
                .game_data
                .plate_assets
                .regions
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>();
            if let Some(country) = &plate.country {
                regions.insert(country.clone());
            }
            design::combo(ui, "plate_region", 240.0)
                .selected_text(
                    plate
                        .country
                        .as_deref()
                        .map_or_else(|| tr("Game default").into(), region_label),
                )
                .show_ui(ui, |ui| {
                    if plate.country.is_none() {
                        ui.selectable_value(&mut plate.country, None, tr("Game default"));
                    }
                    for region in regions {
                        ui.selectable_value(
                            &mut plate.country,
                            Some(region.clone()),
                            region_label(&region),
                        );
                    }
                });
        });
        let region = plate.country.as_deref().unwrap_or_default();
        let request = self.game_data.request;
        if !self
            .plate_tools
            .background
            .as_ref()
            .is_some_and(|((loaded, name, vehicle), _)| {
                *loaded == request && name == region && *vehicle == kind
            })
        {
            self.plate_tools.background =
                self.game_data
                    .plate_assets
                    .background(region, kind)
                    .map(|background| {
                        let texture = ui.ctx().load_texture(
                            "regional-plate-background",
                            egui::ColorImage::from_rgba_unmultiplied(
                                background.size,
                                &background.rgba,
                            ),
                            egui::TextureOptions::LINEAR,
                        );
                        ((request, region.to_owned(), kind), texture)
                    });
        }
        preview(
            ui,
            &plate,
            self.plate_tools
                .background
                .as_ref()
                .map(|(_, texture)| texture),
            self.game_data.installed,
        );
        if self.game_data.plate_assets.regions.is_empty()
            && ui
                .add_enabled(
                    !self.game_data.loading,
                    egui::Button::new(tr("Load game data…")),
                )
                .clicked()
            && let Some(folder) = rfd::FileDialog::new()
                .set_title(tr(
                    "Select the game installation containing def.scs and base.scs",
                ))
                .pick_folder()
        {
            self.begin_game_data(ui.ctx(), folder);
        }
        if self.plate_tools.layout {
            let mut choices = self
                .game_data
                .plate_assets
                .badges
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>();
            if let Some(session) = &self.session {
                for unit in session.data.game.units() {
                    if let Ok(raw) = session.data.game.string(&unit.id, "license_plate")
                        && let Ok(saved) = Plate::parse(&raw)
                    {
                        for part in saved.parts {
                            if let Part::Tag(tag) = part
                                && tag.name() == "img"
                                && let Some(path) = tag.get("src")
                            {
                                choices.insert(path.to_owned());
                            }
                        }
                    }
                }
            }
            for part in &plate.parts {
                if let Part::Tag(tag) = part
                    && tag.name() == "img"
                    && let Some(path) = tag.get("src")
                {
                    choices.insert(path.to_owned());
                }
            }
            let badges = choices.into_iter().collect::<Vec<_>>();
            let mut open = true;
            egui::Window::new(tr("Plate layout and badges")).id(egui::Id::unique("Plate layout and badges")).open(&mut open).collapsible(false).resizable(true).default_width(620.0).show(ui.ctx(),|ui| {
                ui.label(tr("Choose a section, then add formatting before it. Controls stay in plate order. Edits are staged with Apply plate."));
                let mut remove=None; let mut movement=None;
                egui::ScrollArea::vertical().max_height(380.0).show(ui,|ui| {
                    for (index,part) in plate.parts.iter_mut().enumerate() {
                        ui.push_id(index,|ui| {
                            egui::Frame::group(ui.style()).show(ui,|ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.radio_value(&mut self.plate_tools.before,index,format!("Section {}",index+1));
                                    match part {
                                        Part::Text(text)=>{ ui.label(tr("Text")); ui.add(egui::TextEdit::singleline(text).char_limit(128).desired_width(260.0)); },
                                        Part::Tag(tag)=>{ if let Err(error)=controls(ui,tag,&badges) { self.error=Some(error.to_string()); } },
                                    }
                                });
                                ui.horizontal_wrapped(|ui| {
                                    if ui.button(tr("Move up")).clicked() && index>0 { movement=Some((index,index-1)); }
                                    if ui.button(tr("Move down")).clicked() { movement=Some((index,index+1)); }
                                    if ui.button(tr("Remove section")).clicked() { remove=Some(index); }
                                });
                            });
                        });
                    }
                });
                if let Some((from,to))=movement && to<plate.parts.len() { plate.parts.swap(from,to); }
                if let Some(index)=remove { plate.parts.remove(index); }
                self.plate_tools.before=self.plate_tools.before.min(plate.parts.len());
                ui.horizontal_wrapped(|ui| {
                    design::combo(ui,"plate_tool",220.0).selected_text(["Text colour","Font scale","Position","Align left","Align right","Raised text","Lowered text","Restart position","Badge","Text section","End alignment"][self.plate_tools.tool]).show_ui(ui,|ui| {
                        for (index,title) in ["Text colour","Font scale","Position","Align left","Align right","Raised text","Lowered text","Restart position","Badge","Text section","End alignment"].iter().enumerate() { ui.selectable_value(&mut self.plate_tools.tool,index,*title); }
                    });
                    if ui.add_enabled(plate.parts.len()+3<ts_save_core::plates::MAX_PARTS,egui::Button::new(tr("Add before selected section"))).clicked() {
                        if self.plate_tools.tool==9 {plate.parts.insert(self.plate_tools.before,Part::Text("Text".into()));}
                        else if let Err(error)=insert(&mut plate,self.plate_tools.before,self.plate_tools.tool,&badges) {self.error=Some(error.to_string());}
                    }
                });
                if badges.is_empty() { ui.small(tr("Load game data to choose installed plate badges. Existing badge formatting is always retained.")); }
                if ui.add_enabled(!self.game_data.loading,egui::Button::new(tr("Load game data…"))).clicked()
                    && let Some(folder)=rfd::FileDialog::new().set_title(tr("Select the game installation containing def.scs and base.scs")).pick_folder() {
                    self.begin_game_data(ui.ctx(),folder);
                }
                if ui.button(tr("Done")).clicked() {self.plate_tools.layout=false;}
            });
            if !open {
                self.plate_tools.layout = false;
            }
        }
        if plate != before {
            match plate.render() {
                Ok(raw) => self.input.plate = raw,
                Err(error) => self.error = Some(error.to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_in(shape: &egui::epaint::Shape, output: &mut Vec<String>) {
        match shape {
            egui::epaint::Shape::Text(text) => output.push(text.galley.text().to_owned()),
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    text_in(shape, output)
                }
            }
            _ => {}
        }
    }

    #[test]
    fn plate_views_show_readable_controls_and_do_not_modify_saved_markup() {
        let context = egui::Context::default();
        let mut app = EditorApp::new(&eframe::CreationContext::_new_kittest(context.clone()));
        let raw = "<font xscale=0.8 yscale=1><color value=FF000000>ABC 123</font>|germany";
        app.input.plate = raw.into();
        for layout in [false, true] {
            app.plate_tools.layout = layout;
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(680.0, 720.0),
                    )),
                    ..Default::default()
                },
                |ui| app.plate_controls(ui, VehicleKind::Truck),
            );
            output.textures_delta.clear();
            let mut labels = Vec::new();
            for clipped in output.shapes {
                text_in(&clipped.shape, &mut labels);
            }
            assert!(
                !labels
                    .iter()
                    .any(|text| text.contains("<font") || text.contains("<color"))
            );
            assert!(labels.iter().any(|text| text == "Germany"));
            assert!(
                labels
                    .iter()
                    .any(|text| text == "Simplified German plate preview.")
            );
            assert_eq!(
                app.input.plate, raw,
                "displaying a plate must retain every saved byte"
            );
        }
    }

    #[test]
    fn formatting_buttons_build_all_supported_controls_without_markup_entry() {
        for tool in 0..=8 {
            let mut plate = Plate::parse("ABC|germany").unwrap();
            insert(
                &mut plate,
                0,
                tool,
                &["/material/ui/lp/germany/flag.mat".into()],
            )
            .unwrap();
            let raw = plate.render().unwrap();
            assert_eq!(Plate::parse(&raw).unwrap().text(), "ABC");
            assert_eq!(plate.country.as_deref(), Some("germany"));
            assert!(matches!(plate.parts[0], Part::Tag(_)));
        }
    }
}
