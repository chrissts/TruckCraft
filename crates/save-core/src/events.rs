//! Preserve unknown event data while updating the economy's ordered queue.
use crate::{
    Error, Result,
    document::{Document, UnitSpec},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct Event {
    pub id: String,
    pub time: u32,
    pub unit_link: String,
    pub param: i64,
}

pub fn entries(doc: &Document, economy: &str) -> Result<(String, Vec<Event>)> {
    let queue = doc.required(economy, "event_queue")?.to_owned();
    if !doc
        .unit(&queue)
        .is_some_and(|unit| unit.kind.as_ref() == "economy_event_queue")
    {
        return Err(Error::InvalidSii("missing economy event queue"));
    }
    let mut seen = BTreeSet::new();
    let events = doc
        .array(&queue, "data")?
        .into_iter()
        .map(|id| {
            if !seen.insert(id)
                || !doc
                    .unit(id)
                    .is_some_and(|unit| unit.kind.as_ref() == "economy_event")
            {
                return Err(Error::InvalidSii(
                    "invalid or duplicate economy event reference",
                ));
            }
            Ok(Event {
                id: id.into(),
                time: doc.integer(id, "time")?,
                unit_link: doc.required(id, "unit_link")?.into(),
                param: doc.integer(id, "param")?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((queue, events))
}

/// Remove matching events, append new ones, then stably order the whole queue.
/// New entries are (unit link, parameter, absolute game minute).
pub fn rewrite(
    doc: &mut Document,
    economy: &str,
    remove: impl Fn(&Event) -> bool,
    add: &[(String, i64, u32)],
) -> Result<()> {
    let (queue, old) = entries(doc, economy)?;
    let removed = old
        .iter()
        .filter(|event| remove(event))
        .map(|event| event.id.clone())
        .collect::<Vec<_>>();
    let mut retained = old
        .into_iter()
        .filter(|event| !remove(event))
        .collect::<Vec<_>>();
    let mut reserved = BTreeSet::new();
    let mut specs = Vec::new();
    for (link, param, time) in add {
        if doc.unit(link).is_none() {
            return Err(Error::Missing("event target".into()));
        }
        let id = doc.fresh_id(&reserved);
        reserved.insert(id.clone());
        specs.push(UnitSpec {
            kind: "economy_event".into(),
            id: id.clone(),
            scalars: BTreeMap::from([
                ("time".into(), time.to_string()),
                ("unit_link".into(), link.clone()),
                ("param".into(), param.to_string()),
            ]),
            ..Default::default()
        });
        retained.push(Event {
            id,
            time: *time,
            unit_link: link.clone(),
            param: *param,
        });
    }
    retained.sort_by_key(|event| event.time);
    doc.transaction(|doc| {
        doc.insert_units(&specs)?;
        doc.set_array(
            &queue,
            "data",
            retained.into_iter().map(|event| event.id).collect(),
        )?;
        doc.remove_units(&removed)
    })
}
