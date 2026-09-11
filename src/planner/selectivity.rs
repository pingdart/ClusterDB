use crate::planner::stats::{ColumnStats, HistogramValue};

pub struct SelectivityEstimator;

impl SelectivityEstimator {
    /// Estimates equality predicate selectivity: `col = value`
    pub fn estimate_equality(stats: Option<&ColumnStats>, val_str: &str) -> f64 {
        let col = match stats {
            Some(c) => c,
            None => return 0.05, // Default fallback 5%
        };

        if col.ndv > 0 {
            let ndv_sel = 1.0 / col.ndv as f64;

            // If buckets exist, check if target falls in a bucket
            let val = HistogramValue::from_str_auto(val_str);
            for b in &col.buckets {
                if val >= b.lower_bound && val <= b.upper_bound {
                    if b.distinct_count > 0 && b.row_count > 0 {
                        return (1.0 / b.distinct_count as f64).clamp(0.0001, 1.0);
                    }
                }
            }

            ndv_sel.clamp(0.0001, 1.0)
        } else {
            0.05
        }
    }

    /// Estimates range predicate selectivity: `col > value`, `col >= value`, `col < value`, `col <= value`
    pub fn estimate_range(stats: Option<&ColumnStats>, op: &str, val_str: &str) -> f64 {
        let col = match stats {
            Some(c) => c,
            None => return 0.33, // Standard 1/3 default for range without stats
        };

        if col.buckets.is_empty() {
            return 0.33;
        }

        let target = HistogramValue::from_str_auto(val_str);
        let total_buckets = col.buckets.len() as f64;
        let mut matching_fraction = 0.0;

        let is_greater = op.contains('>');

        for b in &col.buckets {
            if is_greater {
                if target < b.lower_bound {
                    // Entire bucket matches
                    matching_fraction += 1.0;
                } else if target >= b.lower_bound && target <= b.upper_bound {
                    // Fractional bucket match
                    let frac = 1.0 - b.lower_bound.range_fraction(&b.upper_bound, &target);
                    matching_fraction += frac.clamp(0.0, 1.0);
                }
            } else {
                // Less than
                if target > b.upper_bound {
                    // Entire bucket matches
                    matching_fraction += 1.0;
                } else if target >= b.lower_bound && target <= b.upper_bound {
                    // Fractional bucket match
                    let frac = b.lower_bound.range_fraction(&b.upper_bound, &target);
                    matching_fraction += frac.clamp(0.0, 1.0);
                }
            }
        }

        (matching_fraction / total_buckets).clamp(0.001, 0.999)
    }

    /// Estimates BETWEEN predicate: `col BETWEEN low AND high`
    pub fn estimate_between(stats: Option<&ColumnStats>, low_str: &str, high_str: &str) -> f64 {
        let s_low = Self::estimate_range(stats, ">=", low_str);
        let s_high = Self::estimate_range(stats, "<=", high_str);
        (s_low + s_high - 1.0).clamp(0.001, 0.999)
    }

    /// Conjunction of two independent predicates (AND)
    pub fn estimate_conjunction(s1: f64, s2: f64) -> f64 {
        (s1 * s2).clamp(0.0001, 1.0)
    }

    /// Disjunction of two independent predicates (OR)
    pub fn estimate_disjunction(s1: f64, s2: f64) -> f64 {
        (s1 + s2 - (s1 * s2)).clamp(0.0001, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::stats::HistogramBucket;

    #[test]
    fn test_selectivity_estimation_with_typed_histogram() {
        let buckets = vec![
            HistogramBucket {
                lower_bound: HistogramValue::Int(0),
                upper_bound: HistogramValue::Int(25),
                row_count: 250,
                distinct_count: 25,
            },
            HistogramBucket {
                lower_bound: HistogramValue::Int(26),
                upper_bound: HistogramValue::Int(50),
                row_count: 250,
                distinct_count: 25,
            },
            HistogramBucket {
                lower_bound: HistogramValue::Int(51),
                upper_bound: HistogramValue::Int(75),
                row_count: 250,
                distinct_count: 25,
            },
            HistogramBucket {
                lower_bound: HistogramValue::Int(76),
                upper_bound: HistogramValue::Int(100),
                row_count: 250,
                distinct_count: 25,
            },
        ];

        let col_stats = ColumnStats {
            column_name: "age".to_string(),
            null_count: 0,
            ndv: 100,
            min_val: Some(HistogramValue::Int(0)),
            max_val: Some(HistogramValue::Int(100)),
            buckets,
        };

        // Equality: 1/100 = 0.01 or 1/25 = 0.04
        let eq_sel = SelectivityEstimator::estimate_equality(Some(&col_stats), "42");
        assert!(eq_sel > 0.0 && eq_sel <= 0.05);

        // Range: age > 75 should match roughly the top 25% (0.25)
        let range_sel = SelectivityEstimator::estimate_range(Some(&col_stats), ">", "75");
        assert!((range_sel - 0.25).abs() < 0.1, "Range sel was {}", range_sel);
    }
}
