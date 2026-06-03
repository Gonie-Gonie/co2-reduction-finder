#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistributionPoint {
    pub x: f64,
    pub density: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistributionSummary {
    pub mean: f64,
    pub std_dev: f64,
    pub p05: f64,
    pub p50: f64,
    pub p95: f64,
    pub smoothed: Vec<DistributionPoint>,
}

pub fn summarize_distribution(values: &[f64], bins: usize) -> Result<DistributionSummary, String> {
    if values.is_empty() {
        return Err("cannot summarize an empty distribution".to_string());
    }
    if bins < 3 {
        return Err("bins must be at least 3".to_string());
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));

    let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
    let variance = sorted
        .iter()
        .map(|value| {
            let diff = value - mean;
            diff * diff
        })
        .sum::<f64>()
        / sorted.len() as f64;

    Ok(DistributionSummary {
        mean,
        std_dev: variance.sqrt(),
        p05: percentile_sorted(&sorted, 0.05),
        p50: percentile_sorted(&sorted, 0.50),
        p95: percentile_sorted(&sorted, 0.95),
        smoothed: smoothed_histogram(&sorted, bins),
    })
}

fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    let last = sorted.len() - 1;
    let position = (last as f64 * p.clamp(0.0, 1.0)).round() as usize;
    sorted[position]
}

fn smoothed_histogram(sorted: &[f64], bins: usize) -> Vec<DistributionPoint> {
    let min = sorted[0];
    let max = sorted[sorted.len() - 1];

    if (max - min).abs() < f64::EPSILON {
        return vec![DistributionPoint {
            x: min,
            density: 1.0,
        }];
    }

    let width = (max - min) / bins as f64;
    let mut counts = vec![0.0; bins];

    for value in sorted {
        let mut index = ((value - min) / width).floor() as usize;
        if index >= bins {
            index = bins - 1;
        }
        counts[index] += 1.0;
    }

    let mut smoothed = counts.clone();
    for index in 0..bins {
        let previous = if index > 0 {
            counts[index - 1]
        } else {
            counts[index]
        };
        let next = if index + 1 < bins {
            counts[index + 1]
        } else {
            counts[index]
        };
        smoothed[index] = previous * 0.25 + counts[index] * 0.5 + next * 0.25;
    }

    let total = smoothed.iter().sum::<f64>().max(f64::EPSILON);
    smoothed
        .into_iter()
        .enumerate()
        .map(|(index, count)| DistributionPoint {
            x: min + width * (index as f64 + 0.5),
            density: count / total,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::summarize_distribution;

    #[test]
    fn summarizes_empirical_distribution() {
        let summary =
            summarize_distribution(&[1.0, 2.0, 3.0, 4.0, 5.0], 5).expect("summary should succeed");

        assert_eq!(summary.mean, 3.0);
        assert_eq!(summary.p50, 3.0);
        assert_eq!(summary.smoothed.len(), 5);
    }
}
