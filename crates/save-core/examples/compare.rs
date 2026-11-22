//! Compare two decoded save documents without printing any saved values or IDs.
use std::{env, fs};
use ts_save_core::{decode_document, value};
fn same(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if a.starts_with('"') || b.starts_with('"') {
        return matches!((value::unquote(a),value::unquote(b)),(Ok(a),Ok(b)) if a==b);
    }
    if a.starts_with('&') || b.starts_with('&') {
        return matches!((value::float(a),value::float(b)),(Ok(a),Ok(b)) if a.to_bits()==b.to_bits());
    }
    match (a.parse::<i128>(), b.parse::<i128>()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("Usage: compare <reference.sii> <candidate.sii>".into());
    }
    let reference = decode_document(&fs::read(&args[0])?)?;
    let candidate = decode_document(&fs::read(&args[1])?)?;
    if reference.unit_count() != candidate.unit_count() {
        return Err("unit counts differ".into());
    }
    let mut fields = 0;
    let mut entries = 0;
    for unit in reference.units() {
        let other = candidate
            .unit(&unit.id)
            .ok_or("unit missing from candidate")?;
        if unit.kind != other.kind {
            return Err("unit types differ".into());
        }
        let expected = reference.fields(&unit.id);
        if expected.len() != candidate.fields(&unit.id).len() {
            return Err("field counts differ".into());
        }
        for (field, a) in expected {
            let b = candidate.required(&unit.id, field)?;
            fields += 1;
            if !same(a, b) {
                return Err(format!(
                    "scalar mismatch in unit type {}, field {}",
                    unit.kind, field
                )
                .into());
            }
            if let Ok(array) = reference.array(&unit.id, field) {
                let other = candidate.array(&unit.id, field)?;
                if array.len() != other.len() {
                    return Err("array counts differ".into());
                }
                for (a, b) in array.iter().zip(other) {
                    entries += 1;
                    if !same(a, b) {
                        return Err(format!(
                            "array mismatch in unit type {}, field {}",
                            unit.kind, field
                        )
                        .into());
                    }
                }
            }
        }
    }
    println!(
        "semantic_comparison=pass units={} field_groups={} array_entries={}",
        reference.unit_count(),
        fields,
        entries
    );
    Ok(())
}
