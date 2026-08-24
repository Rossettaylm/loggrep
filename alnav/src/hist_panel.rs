//! File-mode time histogram overlay (`th`): buckets over `visible`, jump or bind.

use std::collections::BTreeMap;

use alnav::histogram::{self, bucket_key, bucket_until};
use alnav::parser::Level;

use crate::filter_model::TimeBound;
use crate::model::{is_severe_row, EntryRow};

pub const PREVIEW_CAP: usize = 10;

pub enum HistView {
    Closed,
    Loading { interval_secs: u64 },
    Ready(HistReport),
}

pub struct HistJobMsg {
    pub gen: u64,
    pub report: HistReport,
}

#[derive(Debug, Clone)]
pub struct HistBucket {
    pub key: String,
    pub v: usize,
    pub d: usize,
    pub i: usize,
    pub w: usize,
    pub e: usize,
    pub f: usize,
    pub severe: usize,
    pub first_visible: usize,
    pub first_severe_visible: Option<usize>,
    pub preview: Vec<EntryRow>,
}

impl HistBucket {
    pub fn total(&self) -> usize {
        self.v + self.d + self.i + self.w + self.e + self.f
    }

    pub fn jump_visible(&self) -> usize {
        self.first_severe_visible.unwrap_or(self.first_visible)
    }

    pub fn time_bound(&self, interval_secs: u64) -> Option<TimeBound> {
        Some(TimeBound {
            since: Some(self.key.clone()),
            until: bucket_until(&self.key, interval_secs),
        })
    }
}

#[derive(Debug, Clone)]
pub struct HistReport {
    pub interval_secs: u64,
    pub buckets: Vec<HistBucket>,
    pub spike_keys: Vec<String>,
}

impl HistReport {
    pub fn is_spike(&self, key: &str) -> bool {
        self.spike_keys.iter().any(|k| k == key)
    }

    pub fn index_for_key(&self, key: &str) -> usize {
        self.buckets
            .iter()
            .position(|b| b.key.as_str() >= key)
            .unwrap_or_else(|| self.buckets.len().saturating_sub(1))
    }
}

struct Acc {
    v: usize,
    d: usize,
    i: usize,
    w: usize,
    e: usize,
    f: usize,
    severe: usize,
    first_visible: usize,
    first_severe_visible: Option<usize>,
    preview: Vec<EntryRow>,
}

impl Acc {
    fn new(vis_i: usize) -> Self {
        Self {
            v: 0,
            d: 0,
            i: 0,
            w: 0,
            e: 0,
            f: 0,
            severe: 0,
            first_visible: vis_i,
            first_severe_visible: None,
            preview: Vec::new(),
        }
    }

    fn add(&mut self, vis_i: usize, row: &EntryRow) {
        match row.level {
            Level::V => self.v += 1,
            Level::D => self.d += 1,
            Level::I => self.i += 1,
            Level::W => self.w += 1,
            Level::E => self.e += 1,
            Level::F => self.f += 1,
        }
        if is_severe_row(row) {
            self.severe += 1;
            if self.first_severe_visible.is_none() {
                self.first_severe_visible = Some(vis_i);
            }
        }
        if self.preview.len() < PREVIEW_CAP {
            self.preview.push(row.clone());
        }
    }

    fn finish(self, key: String) -> HistBucket {
        HistBucket {
            key,
            v: self.v,
            d: self.d,
            i: self.i,
            w: self.w,
            e: self.e,
            f: self.f,
            severe: self.severe,
            first_visible: self.first_visible,
            first_severe_visible: self.first_severe_visible,
            preview: self.preview,
        }
    }
}

/// Fold timestamped rows into time buckets (visible index is the jump handle).
pub fn build_report<I>(rows: I, interval_secs: u64) -> HistReport
where
    I: IntoIterator<Item = (usize, EntryRow)>,
{
    let mut map: BTreeMap<String, Acc> = BTreeMap::new();
    for (vis_i, row) in rows {
        let Some(key) = bucket_key(&row.as_log_entry(), interval_secs) else {
            continue;
        };
        map.entry(key)
            .and_modify(|acc| acc.add(vis_i, &row))
            .or_insert_with(|| {
                let mut acc = Acc::new(vis_i);
                acc.add(vis_i, &row);
                acc
            });
    }
    let buckets: Vec<HistBucket> = map.into_iter().map(|(key, acc)| acc.finish(key)).collect();
    let spike_keys = spike_keys(&buckets);
    HistReport {
        interval_secs,
        buckets,
        spike_keys,
    }
}

fn spike_keys(buckets: &[HistBucket]) -> Vec<String> {
    if buckets.is_empty() {
        return Vec::new();
    }
    let n = buckets.len() as f64;
    let sum: f64 = buckets.iter().map(|b| b.severe as f64).sum();
    let mean = sum / n;
    let variance: f64 = buckets
        .iter()
        .map(|b| {
            let d = b.severe as f64 - mean;
            d * d
        })
        .sum::<f64>()
        / n;
    let stddev = variance.sqrt();
    if stddev <= 0.0 {
        return Vec::new();
    }
    let threshold = mean + 2.0 * stddev;
    buckets
        .iter()
        .filter(|b| b.severe as f64 > threshold)
        .map(|b| b.key.clone())
        .collect()
}

pub fn pick_interval_from_span(span_secs: u64) -> u64 {
    histogram::pick_interval_secs(span_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(line: &str) -> EntryRow {
        EntryRow::from_line(line).unwrap()
    }

    #[test]
    fn build_report_jumps_to_first_severe() {
        let rows = vec![
            (0, row("04-02 10:32:05.000  1  1 I Tag     : ok")),
            (1, row("04-02 10:32:30.000  1  1 E Tag     : boom")),
            (2, row("04-02 10:33:05.000  1  1 I Tag     : later")),
        ];
        let report = build_report(rows, 60);
        assert_eq!(report.buckets.len(), 2);
        assert_eq!(report.buckets[0].key, "04-02 10:32:00");
        assert_eq!(report.buckets[0].e, 1);
        assert_eq!(report.buckets[0].jump_visible(), 1);
        assert_eq!(report.buckets[1].jump_visible(), 2);
        let bound = report.buckets[0].time_bound(60).unwrap();
        assert_eq!(bound.since.as_deref(), Some("04-02 10:32:00"));
        assert_eq!(bound.until.as_deref(), Some("04-02 10:32:59"));
    }
}
