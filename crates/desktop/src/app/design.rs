use super::*;

pub(super) const INK: Color32 = Color32::from_rgb(27, 42, 59);
pub(super) const MUTED: Color32 = Color32::from_rgb(91, 108, 126);
pub(super) const NAVY: Color32 = Color32::from_rgb(20, 39, 59);
pub(super) const ACCENT: Color32 = Color32::from_rgb(0, 105, 124);
pub(super) const TINT: Color32 = Color32::from_rgb(229, 244, 247);
pub(super) const CANVAS: Color32 = Color32::from_rgb(241, 245, 249);
pub(super) const BORDER: Color32 = Color32::from_rgb(216, 225, 234);
pub(super) const WHITE: Color32 = Color32::WHITE;

pub(super) fn configure(context: &egui::Context) {
    // Use the installed Windows UI font; never redistribute an OS font file.
    let mut fonts = egui::FontDefinitions::default();
    if let Some(windows) = std::env::var_os("WINDIR")
        && let Ok(bytes) = ts_save_core::discovery::read_small(
            &PathBuf::from(windows).join("Fonts/segoeui.ttf"),
            8 * 1024 * 1024,
        )
    {
        fonts.font_data.insert(
            "system-ui".into(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "system-ui".into());
    }
    context.set_fonts(fonts);
    context.set_theme(egui::Theme::Light);
    context.style_mut_of(egui::Theme::Light, |style| {
        for (kind, size) in [
            (egui::TextStyle::Body, 16.0),
            (egui::TextStyle::Button, 15.0),
            (egui::TextStyle::Small, 13.0),
            (egui::TextStyle::Heading, 25.0),
            (egui::TextStyle::Monospace, 15.0),
        ] {
            let family = if kind == egui::TextStyle::Monospace {
                egui::FontFamily::Monospace
            } else {
                egui::FontFamily::Proportional
            };
            style
                .text_styles
                .insert(kind, egui::FontId::new(size, family));
        }
        style.spacing.item_spacing = egui::vec2(12.0, 10.0);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        style.spacing.interact_size = egui::vec2(42.0, 34.0);
        style.spacing.slider_rail_height = 6.0;
        style.spacing.text_edit_width = 280.0;
        style.spacing.combo_width = 260.0;
        let visuals = &mut style.visuals;
        visuals.override_text_color = Some(INK);
        visuals.weak_text_color = Some(MUTED);
        visuals.panel_fill = CANVAS;
        visuals.window_fill = WHITE;
        visuals.window_stroke = egui::Stroke::new(1.0, BORDER);
        visuals.window_corner_radius = egui::CornerRadius::same(12);
        visuals.extreme_bg_color = WHITE;
        visuals.faint_bg_color = Color32::from_rgb(245, 248, 251);
        visuals.hyperlink_color = ACCENT;
        visuals.selection.bg_fill = TINT;
        visuals.selection.stroke = egui::Stroke::new(1.5, ACCENT);
        for widget in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.noninteractive,
        ] {
            widget.bg_fill = Color32::from_rgb(247, 249, 252);
            widget.weak_bg_fill = Color32::from_rgb(247, 249, 252);
            widget.bg_stroke = egui::Stroke::new(1.0, BORDER);
            widget.corner_radius = egui::CornerRadius::same(7);
            widget.fg_stroke = egui::Stroke::new(1.0, INK);
        }
        visuals.widgets.hovered.bg_fill = TINT;
        visuals.widgets.hovered.weak_bg_fill = TINT;
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT);
        visuals.widgets.active.bg_fill = Color32::from_rgb(204, 233, 239);
        visuals.widgets.active.weak_bg_fill = Color32::from_rgb(204, 233, 239);
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, ACCENT);
    });
}

pub(super) fn card(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    let width = ui.available_width();
    let frame = egui::Frame::new()
        .fill(WHITE)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(12)
        .inner_margin(22);
    let inner_width = (width - frame.total_margin().sum().x).max(0.0);
    frame.show(ui, |ui| {
        ui.set_min_width(inner_width);
        content(ui);
    });
}

/// ComboBox widths are minimums in egui. Bound them explicitly so long game
/// names cannot enlarge the page or sidebar. The popup keeps the full label.
pub(super) fn combo<'a>(ui: &egui::Ui, id: &str, preferred: f32) -> egui::ComboBox<'a> {
    egui::ComboBox::from_id_salt(id)
        .width(preferred.min(ui.available_width()).max(0.0))
        .truncate()
}

pub(super) fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(13.0).color(MUTED));
}

pub(super) fn help(ui: &mut egui::Ui, title: &str, text: &str) {
    let button = ui
        .add(
            egui::Button::new(RichText::new("i").strong().color(ACCENT))
                .small()
                .min_size(egui::vec2(24.0, 24.0))
                .corner_radius(12)
                .fill(TINT),
        )
        .on_hover_text(title);
    button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), title));
    egui::Popup::from_toggle_button_response(&button)
        .gap(6.0)
        .show(|ui| {
            ui.set_width(300.0);
            ui.label(egui::RichText::new(title).strong());
            ui.add(egui::Label::new(text).wrap());
        });
}

pub(super) fn percent_slider(ui: &mut egui::Ui, value: &mut f32) -> egui::Response {
    ui.scope(|ui| {
        ui.spacing_mut().slider_width = (ui.available_width() - 88.0).clamp(80.0, 220.0);
        ui.add(
            egui::Slider::new(value, 0.0..=100.0)
                .suffix("%")
                .trailing_fill(true),
        )
    })
    .inner
}

pub(super) fn primary(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(label).color(WHITE).strong())
            .fill(ACCENT)
            .wrap()
            .min_size(egui::vec2(114.0, 36.0)),
    )
}

pub(super) fn badge(ui: &mut egui::Ui, text: &str, color: Color32, background: Color32) {
    egui::Frame::new()
        .fill(background)
        .corner_radius(12)
        .inner_margin(egui::Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(13.0).color(color).strong());
        });
}

pub(super) fn notice(ui: &mut egui::Ui, title: &str, text: &str) {
    egui::Frame::new()
        .fill(TINT)
        .corner_radius(8)
        .inner_margin(14)
        .show(ui, |ui| {
            ui.set_min_width((ui.available_width() - 2.0).max(0.0));
            ui.label(RichText::new(title).color(ACCENT).strong());
            ui.label(RichText::new(text).size(14.0).color(INK));
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn information_button_opens_help_on_click() {
        let context = egui::Context::default();
        let text = "Example help for a control.";
        let frame = |events| {
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(680.0, 480.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| help(ui, "Example", text),
            );
            output.textures_delta.clear();
            output
        };
        let initial = frame(vec![]);
        let icon = initial
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(label) if label.galley.job.text == "i" => {
                    Some(label.pos + label.galley.rect.center().to_vec2())
                }
                _ => None,
            })
            .expect("information icon is visible");
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(icon),
                egui::Event::PointerButton {
                    pos: icon,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        let opened = frame(vec![]);
        assert!(opened.shapes.iter().any(|shape| matches!(
            &shape.shape,
            egui::Shape::Text(label) if label.galley.job.text == text,
        )));
    }
}
