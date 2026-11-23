//! Local read-only inspection and benchmark. Never prints a profile name/path.
use std::{env, fs, hint::black_box, time::Instant};
use ts_save_core::{decode_document, document::Document};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.is_empty() {
        return Err("Usage: inspect <game.sii> [decoded-output.sii]".into());
    }
    let bytes = fs::read(&args[0])?;
    let start = Instant::now();
    let doc = decode_document(&bytes)?;
    println!(
        "decoded_bytes={} units={} field_groups={} decode_and_index_ms={:.3}",
        doc.source_len(),
        doc.unit_count(),
        doc.field_count(),
        start.elapsed().as_secs_f64() * 1000.0
    );
    let text = doc.render()?;
    if let Some(path) = args.get(1) {
        fs::write(path, text.as_bytes())?;
    }
    let mut samples = Vec::new();
    for _ in 0..7 {
        let start = Instant::now();
        let parsed = Document::from_bytes(black_box(text.as_bytes()))?;
        black_box(parsed.unit_count());
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "parse_median_ms={:.3} parse_max_ms={:.3}",
        samples[3], samples[6]
    );
    let start = Instant::now();
    for _ in 0..100 {
        black_box(doc.render()?);
    }
    println!(
        "unchanged_render_mean_ms={:.6}",
        start.elapsed().as_secs_f64() * 10.0
    );
    let mut edited = doc.clone();
    let economy = edited.first_id("economy")?.to_owned();
    let bank = edited.required(&economy, "bank")?.to_owned();
    let original = edited.integer::<i64>(&bank, "money_account")?;
    edited.set(
        &bank,
        "money_account",
        original
            .checked_add(1)
            .ok_or("balance overflow")?
            .to_string(),
    )?;
    let start = Instant::now();
    let changed = edited.render()?;
    println!(
        "changed_render_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.0
    );
    let reload = Document::from_bytes(changed.as_bytes())?;
    assert_eq!(reload.integer::<i64>(&bank, "money_account")?, original + 1);
    println!(
        "money_edit_reload=pass original_units_preserved={}",
        reload.unit_count() == doc.unit_count()
    );
    Ok(())
}
