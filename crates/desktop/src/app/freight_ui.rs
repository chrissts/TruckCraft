use super::*;
use ts_save_core::freight::{self, Offer, Terms};

#[derive(Default)]
pub(super) struct Freight {
    companies: Vec<String>,
    offers: Vec<Offer>,
    source: String,
    selected: String,
    template: String,
    base_template: String,
    draft: Option<Terms>,
    base: Option<Terms>,
    error: Option<String>,
}

impl Freight {
    pub fn has_drafts(&self) -> bool {
        self.draft != self.base || self.template != self.base_template
    }
    pub fn revert(&mut self) {
        self.draft = self.base.clone();
        self.template = self.base_template.clone();
    }
    pub fn refresh(&mut self, data: &SaveData) {
        let result = (|| {
            let companies = freight::companies(data)?;
            let mut offers = Vec::new();
            for company in &companies {
                offers.extend(freight::offers(data, company)?);
            }
            Ok::<_, Error>((companies, offers))
        })();
        match result {
            Ok((companies, offers)) => {
                self.error = None;
                self.companies = companies;
                self.offers = offers;
                if !self.has_drafts() {
                    if !self.companies.contains(&self.source) {
                        self.source = self.companies.first().cloned().unwrap_or_default();
                    }
                    if !self
                        .offers
                        .iter()
                        .any(|offer| offer.id == self.selected && offer.source == self.source)
                    {
                        self.selected = self
                            .offers
                            .iter()
                            .find(|offer| offer.source == self.source)
                            .map(|offer| offer.id.clone())
                            .unwrap_or_default();
                    }
                    self.select(self.selected.clone());
                }
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
    fn select(&mut self, id: String) {
        self.selected = id;
        self.template = self.selected.clone();
        self.base_template = self.template.clone();
        self.base = self
            .offers
            .iter()
            .find(|offer| offer.id == self.selected)
            .map(|offer| offer.terms.clone());
        self.draft = self.base.clone();
    }
}

pub(super) fn company_label(id: &str) -> String {
    let id = id.strip_prefix("company.volatile.").unwrap_or(id);
    let Some((company, city)) = id.rsplit_once('.') else {
        return title_words(id);
    };
    let company = match company {
        "aaa" => "AAA".into(),
        "cesare_smar" => "Cesare Supermercato".into(),
        _ => title_words(company),
    };
    format!("{company} · {}", city_label(city))
}

fn cargo_label(id: &str) -> String {
    let name = freight::cargo_name(id);
    match name.as_str() {
        "tyres" => tr("Tyres").into(),
        _ => title_words(&name),
    }
}

fn trailer_label(id: &str, prefix: &str) -> String {
    let id = id.trim_matches('"');
    match id {
        "trailer_def.scs.box.single_3.curtain" => return tr("SCS 3-axle curtainsider").into(),
        "trailer.scs_curt_s" => return tr("Steering axle").into(),
        _ => {}
    }
    let id = id.strip_prefix(prefix).unwrap_or(id);
    if let Some(name) = id.strip_prefix("scs.") {
        format!("SCS {}", title_words(name))
    } else if let Some(name) = id.strip_prefix("scs_") {
        format!("SCS {}", title_words(name))
    } else {
        title_words(id)
    }
}

fn combination_label(offer: &Offer) -> String {
    format!(
        "{} · {} · {}",
        cargo_label(&offer.cargo),
        trailer_label(&offer.trailer, "trailer_def."),
        trailer_label(&offer.variant, "trailer.")
    )
}

fn offer_label(offer: &Offer) -> String {
    format!(
        "{} → {} · {}",
        cargo_label(&offer.cargo),
        company_label(&offer.terms.target),
        offer.terms.expiration
    )
}

impl EditorApp {
    fn freight_edit(
        &mut self,
        description: &str,
        operation: impl FnOnce(&mut SaveData) -> Result<()>,
    ) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        match session.edit(operation) {
            Ok(()) => {
                self.freight.revert();
                self.refresh();
                self.activity.record("EDIT", description);
                self.status = "Freight changes staged. Save to write them, or use Undo.".into();
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
    pub(super) fn freight(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(tr("Freight Market"));
            design::help(ui, tr("Freight Market"), tr("Apply offer updates the selected offer. Add as new offer leaves it in place and creates another one. Clear removes every offer at the selected source depot. Save changes writes the edits to disk."));
        });
        ui.label(tr("Edit offers or add one using a saved cargo, truck and trailer combination. Changes include the corresponding expiration events."));
        if let Some(error) = &self.freight.error {
            design::notice(ui, tr("Freight data unavailable"), error);
            return;
        }
        if self.freight.companies.is_empty() {
            ui.label(tr("No Freight Market companies are present in this save."));
            return;
        }
        let pending = self.freight.has_drafts();
        let mut source = self.freight.source.clone();
        ui.label(tr("Source company / city"));
        ui.add_enabled_ui(!pending, |ui| {
            design::combo(ui, "freight_source", 520.0)
                .selected_text(company_label(&source))
                .show_ui(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    for id in &self.freight.companies {
                        ui.selectable_value(&mut source, id.clone(), company_label(id));
                    }
                });
        });
        if source != self.freight.source {
            self.freight.source = source;
            let first = self
                .freight
                .offers
                .iter()
                .find(|offer| offer.source == self.freight.source)
                .map(|offer| offer.id.clone())
                .unwrap_or_default();
            self.freight.select(first);
        }
        let mut selected = self.freight.selected.clone();
        ui.label(tr("Existing offer"));
        ui.add_enabled_ui(!pending, |ui| {
            design::combo(ui, "freight_offer", 520.0)
                .selected_text(
                    self.freight
                        .offers
                        .iter()
                        .find(|offer| offer.id == selected)
                        .map_or_else(|| tr("Choose an offer").into(), offer_label),
                )
                .show_ui(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    for offer in self
                        .freight
                        .offers
                        .iter()
                        .filter(|offer| offer.source == self.freight.source)
                    {
                        ui.selectable_value(&mut selected, offer.id.clone(), offer_label(offer));
                    }
                });
        });
        if selected != self.freight.selected {
            self.freight.select(selected);
        }
        let mut template = self.freight.template.clone();
        ui.label(tr("Cargo / trailer / variant combination"));
        design::combo(ui, "freight_template", 520.0)
            .selected_text(
                self.freight
                    .offers
                    .iter()
                    .find(|offer| offer.id == template)
                    .map_or_else(
                        || tr("Choose a saved combination").into(),
                        combination_label,
                    ),
            )
            .show_ui(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                for offer in &self.freight.offers {
                    ui.selectable_value(
                        &mut template,
                        offer.id.clone(),
                        format!(
                            "{} · {}: {} · {}",
                            combination_label(offer),
                            tr("Units"),
                            offer.units,
                            truck_label(&offer.truck)
                        ),
                    );
                }
            });
        if template != self.freight.template {
            self.freight.template = template.clone();
            if self.freight.draft.is_none() {
                self.freight.draft = self
                    .freight
                    .offers
                    .iter()
                    .find(|offer| offer.id == template)
                    .map(|offer| offer.terms.clone());
            }
        }
        if let Some(draft) = &mut self.freight.draft {
            ui.label(tr("Destination company / city"));
            design::combo(ui, "freight_target", 520.0)
                .selected_text(company_label(&draft.target))
                .show_ui(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    for id in &self.freight.companies {
                        if id != &self.freight.source
                            && let Ok(target) = freight::target(id)
                        {
                            ui.selectable_value(
                                &mut draft.target,
                                target.to_owned(),
                                company_label(id),
                            );
                        }
                    }
                });
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("Urgency"));
                for (level, label) in [(0, "Normal"), (1, "Important"), (2, "Urgent")] {
                    ui.selectable_value(&mut draft.urgency, level, tr(label));
                }
            });
            for (label, number) in [
                ("Expiration (absolute game minute)", &mut draft.expiration),
                ("Distance (km)", &mut draft.distance),
                ("Ferry time (minutes)", &mut draft.ferry_time),
                ("Ferry price", &mut draft.ferry_price),
            ] {
                ui.horizontal_wrapped(|ui| {
                    ui.label(tr(label));
                    ui.add(egui::DragValue::new(number));
                });
            }
            if let Some(session) = &self.session
                && let Ok(now) = session
                    .data
                    .game
                    .integer::<u32>(session.data.economy_id().unwrap_or_default(), "game_time")
            {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("{}: {now}", tr("Current game minute")));
                    ui.label(tr("Set an expiration later than the current game minute."));
                });
            }
        }
        let draft = self.freight.draft.clone();
        let source = self.freight.source.clone();
        let selected = self.freight.selected.clone();
        let template = self.freight.template.clone();
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.freight.has_drafts() && !selected.is_empty(),
                    egui::Button::new(tr("Apply offer")),
                )
                .on_hover_text(tr(
                    "Stage edits to the selected offer and its expiration event.",
                ))
                .clicked()
                && let Some(terms) = draft.clone()
            {
                self.freight_edit("Edit freight offer", |data| {
                    freight::configure(data, &source, &selected, &template, &terms)
                });
            }
            if ui
                .add_enabled(draft.is_some(), egui::Button::new(tr("Add as new offer")))
                .on_hover_text(tr(
                    "Stage a new offer using the selected cargo, trailer and truck combination.",
                ))
                .clicked()
                && let Some(terms) = draft
            {
                self.freight_edit("Edit freight offer", |data| {
                    freight::create(data, &source, &template, &terms).map(|_| ())
                });
            }
            if ui
                .add_enabled(
                    !pending && !selected.is_empty(),
                    egui::Button::new(tr("Remove selected offer")),
                )
                .on_hover_text(tr("Stage removal of the selected offer only."))
                .clicked()
            {
                self.freight_edit("Edit freight offer", |data| {
                    freight::remove(data, &source, std::slice::from_ref(&selected))
                });
            }
            if ui
                .add_enabled(
                    !pending,
                    egui::Button::new(tr("Clear source company's offers")),
                )
                .on_hover_text(tr(
                    "Stage removal of every freight offer at the selected source depot.",
                ))
                .clicked()
            {
                self.freight_edit("Clear company freight offers", |data| {
                    freight::clear(data, &source)
                });
            }
            if ui
                .add_enabled(
                    self.freight.has_drafts(),
                    egui::Button::new(tr("Revert offer inputs")),
                )
                .on_hover_text(tr("Restore this form to the last staged offer."))
                .clicked()
            {
                self.freight.revert();
            }
        });
        ui.label(tr("Available combinations come from this save. Special transports with positioned trailers cannot be cloned. New game-definition catalogs and route-distance calculation are still being migrated."));
    }
}

fn truck_label(id: &str) -> String {
    let path = id.strip_prefix("/def/vehicle/truck/").unwrap_or(id);
    let path = path.strip_suffix(".sii").unwrap_or(path);
    path.split('/')
        .filter(|segment| !matches!(*segment, "company" | ""))
        .map(title_words)
        .collect::<Vec<_>>()
        .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn market_labels_remove_save_prefixes_and_separate_depots_from_cities() {
        assert_eq!(
            company_label("company.volatile.aaa.cagliari"),
            "AAA · Cagliari"
        );
        assert_eq!(
            company_label("cesare_smar.pescara"),
            "Cesare Supermercato · Pescara"
        );
        assert_eq!(company_label("depot.san_francisc"), "Depot · San Francisco");
        crate::i18n::select("en");
        assert_eq!(cargo_label("cargo.tyres"), "Tyres");
        crate::i18n::select("es");
        assert_eq!(cargo_label("cargo.tyres"), "Neumáticos");
        crate::i18n::select("en");
        assert_eq!(
            trailer_label("trailer.scs_curt_s", "trailer."),
            "Steering axle"
        );
        assert_eq!(
            trailer_label("trailer_def.scs.box.single_3.curtain", "trailer_def."),
            "SCS 3-axle curtainsider"
        );
        assert_eq!(
            trailer_label("trailer.scs_flat_b", "trailer."),
            "SCS Flat B"
        );
        assert_eq!(
            truck_label("/def/vehicle/truck/volvo.fh16/company/stock.sii"),
            "Volvo Fh16 · Stock"
        );
    }
}
