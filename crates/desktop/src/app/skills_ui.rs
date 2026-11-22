use super::*;
use crate::game_artwork::GameArtwork;

pub(super) fn editor(
    ui: &mut egui::Ui,
    game: Game,
    art: &GameArtwork,
    skills: &mut [u8; 5],
    adr: &mut u8,
    id: &str,
) {
    let art = art.for_game(game);
    ui.push_id(id, |ui| {
        ui.horizontal(|ui| {
            ui.strong(tr("Driver skills"));
            design::help(ui, tr("Driver skills"), tr("Choose a rank. Click the current rank to step down."));
        });
        ui.add_space(4.0);
        let row_width = ui.available_width().min(660.0);
        let padding = egui::Margin::symmetric(14, 8);
        let inner_width = row_width - padding.sum().x;
        let icon_width = if art.is_some() { 40.0 } else { 0.0 };
        let column_gap = 12.0;
        let name_gap = if art.is_some() { column_gap } else { 0.0 };
        let rank_width = 37.0;
        let rank_gap = 8.0;
        let rank_group_width = 6.0 * rank_width + 6.0 * rank_gap + 34.0;
        let name_width = (inner_width - icon_width - name_gap - column_gap - rank_group_width - 2.0)
            .max(112.0);
        let row_height = 44.0;
        let name_group_size = egui::vec2(icon_width + name_gap + name_width, row_height);
        let rank_group_size = egui::vec2(rank_group_width, row_height);
        let centered_row = egui::Layout::left_to_right(egui::Align::Center);
        for (index, label) in [
            "Long distance",
            "High-value cargo",
            "Fragile cargo",
            "Just-in-time delivery",
            "Eco driving",
        ]
        .into_iter()
        .enumerate()
        {
            let label = tr(label);
            egui::Frame::new()
                .fill(design::CANVAS)
                .corner_radius(8)
                .inner_margin(padding)
                .show(ui, |ui| {
                    ui.set_min_width(inner_width);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = column_gap;
                        ui.allocate_ui_with_layout(name_group_size, centered_row, |ui| {
                            ui.set_min_size(name_group_size);
                            if let Some(art) = art {
                                ui.add(
                                    egui::Image::new((
                                        art.skills[index + 1].id(),
                                        egui::Vec2::splat(icon_width),
                                    ))
                                    .uv(egui::Rect::from_min_max(
                                        egui::Pos2::ZERO,
                                        egui::pos2(0.5, 1.0),
                                    )),
                                );
                            }
                            ui.add(egui::Label::new(RichText::new(label).strong()).wrap());
                        });
                        ui.allocate_ui_with_layout(rank_group_size, centered_row, |ui| {
                            ui.set_min_size(rank_group_size);
                            ui.spacing_mut().item_spacing.x = rank_gap;
                            for rank in 1..=6_u8 {
                                let unlocked = rank <= skills[index];
                                let current = rank == skills[index];
                                let response = ui.add_sized(
                                    [rank_width, 36.0],
                                    egui::Button::new("")
                                        .fill(if current {
                                            design::ACCENT
                                        } else if unlocked {
                                            design::TINT
                                        } else {
                                            design::WHITE
                                        })
                                        .stroke(egui::Stroke::new(
                                            1.0,
                                            if current {
                                                design::ACCENT
                                            } else {
                                                design::BORDER
                                            },
                                        )),
                                );
                                response.widget_info(|| {
                                    egui::WidgetInfo::selected(
                                        egui::Role::Button,
                                        ui.is_enabled(),
                                        current,
                                        format!("{label} · {} {rank}", tr("Rank")),
                                    )
                                });
                                let color = if current {
                                    design::WHITE
                                } else if unlocked {
                                    design::ACCENT
                                } else {
                                    design::MUTED
                                };
                                let rank_index = usize::from(rank - 1);
                                if let Some(art) = art {
                                    ui.painter().image(
                                        art.ranks[rank_index].id(),
                                        egui::Rect::from_center_size(
                                            response.rect.center(),
                                            egui::vec2(25.0, 25.0),
                                        ),
                                        egui::Rect::from_min_max(
                                            egui::Pos2::ZERO,
                                            egui::pos2(1.0, 1.0),
                                        ),
                                        color,
                                    );
                                } else {
                                    ui.painter().text(
                                        response.rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        ["I", "II", "III", "IV", "V", "VI"][rank_index],
                                        egui::FontId::proportional(15.0),
                                        color,
                                    );
                                }
                                if response
                                    .on_hover_text(format!("{label} · {} {rank}/6", tr("Rank")))
                                    .clicked()
                                {
                                    skills[index] = if skills[index] == rank {
                                        rank - 1
                                    } else {
                                        rank
                                    };
                                }
                            }
                            ui.small(format!("{} / 6", skills[index]));
                        });
                    });
                });
        }
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            if let Some(art) = art {
                ui.add(
                    egui::Image::new((art.skills[0].id(), egui::vec2(40.0, 40.0))).uv(
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(0.5, 1.0)),
                    ),
                );
            }
            ui.strong(if game == Game::Ets2 {
                tr("ADR / hazardous cargo")
            } else {
                tr("Hazardous cargo")
            });
            design::help(ui, tr("Hazardous cargo"), tr("Select the hazardous cargo classes your driver can carry. Apply progression stages skill and cargo-class changes."));
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for (index, label) in [
                "Explosives",
                "Gases",
                "Flammable liquids",
                "Flammable solids",
                "Toxic",
                "Corrosives",
            ]
            .into_iter()
            .enumerate()
            {
                let label = tr(label);
                let enabled = *adr & (1 << index) != 0;
                let response = ui.add_sized(
                    [94.0, if art.is_some() { 86.0 } else { 54.0 }],
                    egui::Button::new("")
                        .fill(if enabled { design::TINT } else { design::WHITE })
                        .stroke(egui::Stroke::new(
                            if enabled { 2.0 } else { 1.0 },
                            if enabled {
                                design::ACCENT
                            } else {
                                design::BORDER
                            },
                        )),
                );
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::Role::Button,
                        ui.is_enabled(),
                        enabled,
                        label,
                    )
                });
                if let Some(art) = art {
                    let image = if enabled {
                        &art.adr[index]
                    } else {
                        &art.adr_grey[index]
                    };
                    ui.painter().image(
                        image.id(),
                        egui::Rect::from_center_size(
                            egui::pos2(response.rect.center().x, response.rect.top() + 26.0),
                            egui::vec2(40.0, 40.0),
                        ),
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        design::WHITE,
                    );
                }
                let mut job = egui::text::LayoutJob::simple(
                    label.into(),
                    egui::FontId::proportional(12.0),
                    design::INK,
                    response.rect.width() - 8.0,
                );
                job.halign = egui::Align::Center;
                let text = ui.painter().layout_job(job);
                let center = if art.is_some() {
                    egui::pos2(response.rect.center().x, response.rect.top() + 65.0)
                } else {
                    response.rect.center()
                };
                ui.painter().galley(
                    center - text.rect.center().to_vec2(),
                    text,
                    design::INK,
                );
                if response.on_hover_text(label).clicked() {
                    *adr ^= 1 << index;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("Unlock all skills")).clicked() {
                *skills = [6; 5];
                *adr = 63;
            }
            if ui.button(tr("Clear skills")).clicked() {
                *skills = [0; 5];
                *adr = 0;
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_labels_and_rank_buttons_align() {
        let context = egui::Context::default();
        design::configure(&context);
        context.set_pixels_per_point(1.25);
        let art = GameArtwork::load(&context);
        crate::i18n::select("en");
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(680.0, 720.0),
                )),
                ..Default::default()
            },
            |ui| editor(ui, Game::Ets2, &art, &mut [0; 5], &mut 0, "skills"),
        );
        output.textures_delta.clear();
        let label = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "Just-in-time delivery" => {
                    Some(&text.galley.job)
                }
                _ => None,
            })
            .expect("skill label is visible");
        assert!(!label.justify);
        assert_eq!(label.halign, egui::Align::Min);

        let first_rank = art.for_game(Game::Ets2).map(|art| art.ranks[0].id());
        let rank_centers: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) if Some(mesh.texture_id) == first_rank => {
                    Some(mesh.calc_bounds().center())
                }
                egui::Shape::Text(text) if first_rank.is_none() && text.galley.text() == "I" => {
                    Some(text.pos + text.galley.rect.center().to_vec2())
                }
                _ => None,
            })
            .collect();
        assert_eq!(rank_centers.len(), 5);
        assert!(
            rank_centers
                .iter()
                .all(|center| (center.x - rank_centers[0].x).abs() < 0.5)
        );

        let rows: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect) if rect.fill == design::CANVAS => Some(rect.rect),
                _ => None,
            })
            .collect();
        assert_eq!(rows.len(), 5);
        assert!(
            rows.iter()
                .all(|row| (row.width() - rows[0].width()).abs() < 0.5)
        );
        assert!(
            rows.iter()
                .zip(rank_centers)
                .all(|(row, rank)| (row.center().y - rank.y).abs() < 1.0)
        );
    }
}
