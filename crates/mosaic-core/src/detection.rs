//! Heuristic only: a random-looking original tile is indistinguishable from ciphertext.
use crate::{Bitmap, Error, GridSelection, Rect, Result};

#[derive(Debug)]
pub struct BlockSuggestions {
    pub selection: GridSelection,
    pub uncertain_blocks: usize,
}
pub fn detect_blocks(bytes: &[u8], size: u32) -> Result<BlockSuggestions> {
    if ![8, 16, 32, 64].contains(&size) {
        return Err(Error::Region("信息熵检测须使用 8/16/32/64 像素区块"));
    }
    let b = Bitmap::parse(bytes.to_vec())?;
    let mut selection = GridSelection::empty(b.width, b.height, size)?;
    let cols = b.width.div_ceil(size);
    let rows = b.height.div_ceil(size);
    let mut uncertain_blocks = 0;
    let mut expected_by_length = std::collections::HashMap::new();
    for y in 0..rows {
        for x in 0..cols {
            let r = Rect {
                x: x * size,
                y: y * size,
                width: size.min(b.width - x * size),
                height: size.min(b.height - y * size),
            };
            let data = b.read(r);
            // A tiny clipped edge cannot provide useful evidence. Do not silently call it plaintext.
            if data.len() < 96 {
                uncertain_blocks += 1;
                continue;
            }
            let expected = *expected_by_length
                .entry(data.len())
                .or_insert_with(|| uniform_entropy(data.len()));
            if candidate(&data, r.width as usize, r.height as usize, expected) {
                let i = (y * cols + x) as usize;
                selection.bits[i / 8] |= 1 << (i % 8);
            }
        }
    }
    Ok(BlockSuggestions {
        selection,
        uncertain_blocks,
    })
}
fn candidate(data: &[u8], width: usize, height: usize, expected_entropy: f64) -> bool {
    let n = data.len() as f64;
    let mut counts = [0usize; 256];
    for v in data {
        counts[*v as usize] += 1;
    }
    let entropy: f64 = counts
        .iter()
        .filter(|c| **c > 0)
        .map(|c| {
            let p = *c as f64 / n;
            -p * p.log2()
        })
        .sum();
    if entropy < expected_entropy - 0.30 {
        return false;
    }
    let expected = n / 256.0;
    let chi: f64 = counts
        .iter()
        .map(|c| (*c as f64 - expected).powi(2) / expected)
        .sum();
    if chi > 420.0 {
        return false;
    }
    // Smooth gradients can have broad histograms but retain strong local structure.
    let mut difference = 0usize;
    let mut pairs = 0usize;
    for y in 0..height {
        for x in 0..width {
            let i = (y * width + x) * 3;
            for j in [
                if x + 1 < width { Some(i + 3) } else { None },
                if y + 1 < height {
                    Some(i + width * 3)
                } else {
                    None
                },
            ]
            .into_iter()
            .flatten()
            {
                for c in 0..3 {
                    difference += data[i + c].abs_diff(data[j + c]) as usize;
                    pairs += 1;
                }
            }
        }
    }
    pairs > 0 && difference as f64 / pairs as f64 >= 48.0
}

fn uniform_entropy(count: usize) -> f64 {
    let n = count as f64;
    // Expected empirical entropy of n IID uniform bytes, rather than the unattainable 8-bit limit.
    if count <= 4096 {
        let mut probability = (255.0_f64 / 256.0).powf(n);
        let mut sum = 0.0;
        for k in 1..=count {
            probability *= ((count - k + 1) as f64) / (k as f64 * 255.0);
            sum += probability * k as f64 * (k as f64).log2();
        }
        n.log2() - 256.0 / n * sum
    } else {
        8.0 - 255.0 / (2.0 * n * std::f64::consts::LN_2)
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct EntropyRange {
    pub min: f64,
    pub max: f64,
    pub mean: f64,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct EncryptionPreview {
    pub encrypted_blocks: usize,
    pub plain_blocks: usize,
    pub encrypted_entropy: Option<EntropyRange>,
    pub plain_entropy: Option<EntropyRange>,
    pub entropy_auc: Option<f64>,
    pub missed_blocks: usize,
    pub false_positive_blocks: usize,
    pub uncertain_blocks: usize,
    pub warning: bool,
}
/// Analyze a candidate ciphertext against its actual target mask. Does not mutate it.
pub fn assess_encryption(bytes: &[u8], actual: &GridSelection) -> Result<EncryptionPreview> {
    let b = Bitmap::parse(bytes.to_vec())?;
    actual.validate(b.width, b.height)?;
    let suggestions = detect_blocks(bytes, actual.block_size)?;
    let cols = b.width.div_ceil(actual.block_size);
    let rows = b.height.div_ceil(actual.block_size);
    let mut encrypted = vec![];
    let mut plain = vec![];
    let mut missed = 0;
    let mut false_positive = 0;
    for y in 0..rows {
        for x in 0..cols {
            let i = (y * cols + x) as usize;
            let selected = actual.bits[i / 8] & (1 << (i % 8)) != 0;
            let suggested = suggestions.selection.bits[i / 8] & (1 << (i % 8)) != 0;
            if selected && !suggested {
                missed += 1;
            }
            if !selected && suggested {
                false_positive += 1;
            }
            let size = actual.block_size;
            let r = Rect {
                x: x * size,
                y: y * size,
                width: size.min(b.width - x * size),
                height: size.min(b.height - y * size),
            };
            let data = b.read(r);
            let mut counts = [0usize; 256];
            for v in &data {
                counts[*v as usize] += 1;
            }
            let entropy = counts
                .into_iter()
                .filter(|c| *c > 0)
                .map(|c| {
                    let p = c as f64 / data.len() as f64;
                    -p * p.log2()
                })
                .sum::<f64>();
            if selected {
                encrypted.push(entropy);
            } else {
                plain.push(entropy);
            }
        }
    }
    fn range(v: &[f64]) -> Option<EntropyRange> {
        if v.is_empty() {
            None
        } else {
            Some(EntropyRange {
                min: v.iter().copied().fold(f64::INFINITY, f64::min),
                max: v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                mean: v.iter().sum::<f64>() / v.len() as f64,
            })
        }
    }
    let encrypted_entropy = range(&encrypted);
    let plain_entropy = range(&plain);
    let auc = if encrypted.is_empty() || plain.is_empty() {
        None
    } else {
        plain.sort_by(f64::total_cmp);
        let wins: f64 = encrypted
            .iter()
            .map(|h| {
                let lower = plain.partition_point(|p| p < h);
                let upper = plain.partition_point(|p| p <= h);
                lower as f64 + (upper - lower) as f64 * 0.5
            })
            .sum();
        Some(wins / (encrypted.len() as f64 * plain.len() as f64))
    };
    Ok(EncryptionPreview {
        encrypted_blocks: encrypted.len(),
        plain_blocks: plain.len(),
        encrypted_entropy,
        plain_entropy,
        entropy_auc: auc,
        missed_blocks: missed,
        false_positive_blocks: false_positive,
        uncertain_blocks: suggestions.uncertain_blocks,
        warning: !encrypted.is_empty()
            && (missed > 0 || false_positive > 0 || auc.is_none_or(|a| a < 0.95)),
    })
}
