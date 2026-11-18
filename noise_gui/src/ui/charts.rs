//! Charts. These read the same in-memory results the tables show, so a bar and
//! a table row can never disagree.

use egui::Ui;

use super::{ACCENT, WARN};
use crate::state::AppState;

const CHART_HEIGHT: f32 = 210.0;
/// More than this many bars stops being readable, so the bar chart shows the
/// largest samples and says what it left out.
const MAX_BARS: usize = 24;

pub fn charts_section(ui: &mut Ui, state: &mut AppState) {
    egui::CollapsingHeader::new("Charts")
        .default_open(true)
        .id_salt("charts")
        .show(ui, |ui| {
            if state.live.is_some() {
                sequence_charts(ui, state);
            }
            if !state.counts.is_empty() {
                count_charts(ui, state);
            }
        });
    let _ = state;
}

/// Per-sample counts for the current query, plus how those counts are spread.
fn sequence_charts(ui: &mut Ui, state: &AppState) {
    let Some(result) = &state.live else {
        return;
    };
    let mut hits: Vec<&noise_core::SampleHit> = result.hits.iter().collect();
    hits.sort_by_key(|hit| std::cmp::Reverse(crate::state::shown_count(hit)));

    ui.label(egui::RichText::new(format!("Counts per sample — {}", result.sequence)).small());
    let largest: Vec<&noise_core::SampleHit> = hits.iter().copied().take(MAX_BARS).collect();
    if hits.len() > MAX_BARS {
        ui.label(
            egui::RichText::new(format!(
                "showing the {MAX_BARS} largest of {} samples",
                hits.len()
            ))
            .small()
            .color(WARN),
        );
    }

    let bars: Vec<egui_plot::Bar> = largest
        .iter()
        .enumerate()
        .map(|(index, hit)| {
            egui_plot::Bar::new(index as f64, crate::state::shown_count(hit) as f64)
                .name(hit.sample_name.clone())
                .fill(ACCENT)
        })
        .collect();

    egui_plot::Plot::new("per_sample_counts")
        .height(CHART_HEIGHT)
        .show_axes([false, true])
        .legend(egui_plot::Legend::default())
        .show(ui, |plot| {
            plot.bar_chart(egui_plot::BarChart::new("counts", bars).width(0.8));
        });

    ui.add_space(6.0);
    ui.label(egui::RichText::new("How the counts are spread").small());
    distribution_chart(ui, &hits);
}

/// A histogram of the per-sample counts: the interesting signal in this dataset
/// is that most samples share a handful of sequences and the rest are noise.
fn distribution_chart(ui: &mut Ui, hits: &[&noise_core::SampleHit]) {
    let mut buckets: Vec<(u64, u64)> = Vec::new();
    for power in 0..14u32 {
        let low = 1u64 << power;
        let high = 1u64 << (power + 1);
        let samples = hits
            .iter()
            .filter(|hit| {
                let count = crate::state::shown_count(hit);
                count >= low && count < high
            })
            .count() as u64;
        if samples > 0 {
            buckets.push((low, samples));
        }
    }
    let zero = hits
        .iter()
        .filter(|hit| crate::state::shown_count(hit) == 0)
        .count() as u64;
    if zero > 0 {
        buckets.insert(0, (0, zero));
    }

    let bars: Vec<egui_plot::Bar> = buckets
        .iter()
        .enumerate()
        .map(|(index, (low, samples))| {
            let label = if *low == 0 {
                "0".to_string()
            } else {
                format!("{low}–{}", low * 2 - 1)
            };
            egui_plot::Bar::new(index as f64, *samples as f64)
                .name(label)
                .fill(super::OK)
        })
        .collect();

    egui_plot::Plot::new("count_distribution")
        .height(CHART_HEIGHT)
        .show_axes([false, true])
        .legend(egui_plot::Legend::default())
        .show(ui, |plot| {
            plot.bar_chart(egui_plot::BarChart::new("samples", bars).width(0.8));
        });
}

/// The duplicate-count run: reads used versus skipped, the figure that makes a
/// length filter's effect visible.
fn count_charts(ui: &mut Ui, state: &AppState) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new("Reads used versus skipped per sample").small());
    let bars_used: Vec<egui_plot::Bar> = state
        .counts
        .iter()
        .enumerate()
        .map(|(index, result)| {
            egui_plot::Bar::new(index as f64, result.sequences_used as f64)
                .name(result.sample_name.clone())
                .fill(ACCENT)
        })
        .collect();
    let bars_skipped: Vec<egui_plot::Bar> = state
        .counts
        .iter()
        .enumerate()
        .map(|(index, result)| {
            egui_plot::Bar::new(index as f64, result.lines_skipped as f64)
                .base_offset(result.sequences_used as f64)
                .name(result.sample_name.clone())
                .fill(WARN)
        })
        .collect();

    egui_plot::Plot::new("reads_per_sample")
        .height(CHART_HEIGHT)
        .show_axes([false, true])
        .legend(egui_plot::Legend::default())
        .show(ui, |plot| {
            plot.bar_chart(egui_plot::BarChart::new("used", bars_used).width(0.8));
            plot.bar_chart(egui_plot::BarChart::new("skipped", bars_skipped).width(0.8));
        });
}
