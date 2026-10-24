use crate::columnar::types::{ColumnArray, ColumnType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RleRun<T> {
    pub value: T,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DictionaryColumn {
    pub dictionary: Vec<String>,
    pub codes: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BitPackedI64Column {
    pub base_value: i64,
    pub deltas: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CompressedColumn {
    Uncompressed(ColumnArray),
    RleInt64(Vec<RleRun<i64>>),
    RleText(Vec<RleRun<String>>),
    Dictionary(DictionaryColumn),
    BitPackedDelta(BitPackedI64Column),
}

impl CompressedColumn {
    /// Compresses a column array using the optimal compression algorithm
    pub fn compress(array: &ColumnArray, col_type: &ColumnType) -> Self {
        match (array, col_type) {
            (ColumnArray::Int64(vals), _) => {
                if vals.is_empty() {
                    return CompressedColumn::Uncompressed(array.clone());
                }

                // Check RLE compression potential
                let mut runs = Vec::new();
                let mut current_val = vals[0];
                let mut current_count = 1;

                for &v in &vals[1..] {
                    if v == current_val {
                        current_count += 1;
                    } else {
                        runs.push(RleRun {
                            value: current_val,
                            count: current_count,
                        });
                        current_val = v;
                        current_count = 1;
                    }
                }
                runs.push(RleRun {
                    value: current_val,
                    count: current_count,
                });

                if runs.len() * 2 < vals.len() {
                    return CompressedColumn::RleInt64(runs);
                }

                // Check Bit-Packed / Delta (Frame-of-Reference)
                let min_val = *vals.iter().min().unwrap();
                let max_val = *vals.iter().max().unwrap();
                if max_val >= min_val && (max_val - min_val) <= (u32::MAX as i64) {
                    let deltas: Vec<u32> = vals.iter().map(|&v| (v - min_val) as u32).collect();
                    return CompressedColumn::BitPackedDelta(BitPackedI64Column {
                        base_value: min_val,
                        deltas,
                    });
                }

                CompressedColumn::Uncompressed(array.clone())
            }
            (ColumnArray::Text(vals), _) => {
                if vals.is_empty() {
                    return CompressedColumn::Uncompressed(array.clone());
                }

                // Dictionary encoding: index distinct values
                let mut dict = Vec::new();
                let mut dict_map = HashMap::new();
                let mut codes = Vec::with_capacity(vals.len());

                for s in vals {
                    let code = if let Some(&idx) = dict_map.get(s) {
                        idx
                    } else {
                        let idx = dict.len() as u32;
                        dict.push(s.clone());
                        dict_map.insert(s.clone(), idx);
                        idx
                    };
                    codes.push(code);
                }

                // If distinct cardinality is significantly smaller than total rows
                if dict.len() <= vals.len() / 2 || vals.len() >= 10 {
                    CompressedColumn::Dictionary(DictionaryColumn {
                        dictionary: dict,
                        codes,
                    })
                } else {
                    CompressedColumn::Uncompressed(array.clone())
                }
            }
            _ => CompressedColumn::Uncompressed(array.clone()),
        }
    }

    /// Decompresses the column back to raw ColumnArray
    pub fn decompress(&self) -> ColumnArray {
        match self {
            CompressedColumn::Uncompressed(arr) => arr.clone(),
            CompressedColumn::RleInt64(runs) => {
                let mut out = Vec::new();
                for r in runs {
                    for _ in 0..r.count {
                        out.push(r.value);
                    }
                }
                ColumnArray::Int64(out)
            }
            CompressedColumn::RleText(runs) => {
                let mut out = Vec::new();
                for r in runs {
                    for _ in 0..r.count {
                        out.push(r.value.clone());
                    }
                }
                ColumnArray::Text(out)
            }
            CompressedColumn::Dictionary(dict_col) => {
                let mut out = Vec::with_capacity(dict_col.codes.len());
                for &code in &dict_col.codes {
                    if let Some(str_val) = dict_col.dictionary.get(code as usize) {
                        out.push(str_val.clone());
                    } else {
                        out.push(String::new());
                    }
                }
                ColumnArray::Text(out)
            }
            CompressedColumn::BitPackedDelta(bp) => {
                let mut out = Vec::with_capacity(bp.deltas.len());
                for &delta in &bp.deltas {
                    out.push(bp.base_value + delta as i64);
                }
                ColumnArray::Int64(out)
            }
        }
    }

    /// Calculates total logical rows in this compressed column
    pub fn row_count(&self) -> usize {
        match self {
            CompressedColumn::Uncompressed(arr) => arr.len(),
            CompressedColumn::RleInt64(runs) => runs.iter().map(|r| r.count as usize).sum(),
            CompressedColumn::RleText(runs) => runs.iter().map(|r| r.count as usize).sum(),
            CompressedColumn::Dictionary(d) => d.codes.len(),
            CompressedColumn::BitPackedDelta(bp) => bp.deltas.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rle_int64_compression_roundtrip() {
        // High repetition dataset: 1000 items with only 3 distinct runs
        let mut raw = Vec::new();
        for _ in 0..300 { raw.push(10); }
        for _ in 0..400 { raw.push(20); }
        for _ in 0..300 { raw.push(30); }

        let arr = ColumnArray::Int64(raw.clone());
        let compressed = CompressedColumn::compress(&arr, &ColumnType::Int64);

        if let CompressedColumn::RleInt64(runs) = &compressed {
            assert_eq!(runs.len(), 3);
        } else {
            panic!("Expected RLE compression");
        }

        let decompressed = compressed.decompress();
        assert_eq!(decompressed, ColumnArray::Int64(raw));
    }

    #[test]
    fn test_dictionary_compression_roundtrip() {
        let mut raw = Vec::new();
        let cities = vec!["Hyderabad", "Bengaluru", "Mumbai", "Delhi"];
        for i in 0..200 {
            raw.push(cities[i % cities.len()].to_string());
        }

        let arr = ColumnArray::Text(raw.clone());
        let compressed = CompressedColumn::compress(&arr, &ColumnType::Text);

        if let CompressedColumn::Dictionary(dict) = &compressed {
            assert_eq!(dict.dictionary.len(), 4);
            assert_eq!(dict.codes.len(), 200);
        } else {
            panic!("Expected Dictionary compression");
        }

        let decompressed = compressed.decompress();
        assert_eq!(decompressed, ColumnArray::Text(raw));
    }

    #[test]
    fn test_delta_bitpacked_compression_roundtrip() {
        // Sequential timestamps: high base, small delta
        let base: i64 = 1_700_000_000;
        let raw: Vec<i64> = (0..100).map(|i| base + i * 5).collect();

        let arr = ColumnArray::Int64(raw.clone());
        let compressed = CompressedColumn::compress(&arr, &ColumnType::Int64);

        if let CompressedColumn::BitPackedDelta(bp) = &compressed {
            assert_eq!(bp.base_value, base);
            assert_eq!(bp.deltas.len(), 100);
            assert_eq!(bp.deltas[1], 5);
        } else {
            panic!("Expected BitPackedDelta compression");
        }

        let decompressed = compressed.decompress();
        assert_eq!(decompressed, ColumnArray::Int64(raw));
    }
}
