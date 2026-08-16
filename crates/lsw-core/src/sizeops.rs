use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::error::{Error, Result};

const BUCKET_ORDER: &[&str] = &[
    "code",
    "rodata",
    "data",
    "resources",
    "imports",
    "exports",
    "exceptions",
    "debug",
    "relocations",
    "tls",
    "other",
    "overhead",
];

fn bucket_for(section: &str) -> &'static str {
    let lower = section.to_ascii_lowercase();
    if lower.starts_with(".text") {
        "code"
    } else if lower.starts_with(".rdata") || lower.starts_with(".rodata") {
        "rodata"
    } else if lower.starts_with(".data") || lower.starts_with(".bss") {
        "data"
    } else if lower.starts_with(".rsrc") {
        "resources"
    } else if lower.starts_with(".idata") {
        "imports"
    } else if lower.starts_with(".edata") {
        "exports"
    } else if lower.starts_with(".pdata") || lower.starts_with(".xdata") {
        "exceptions"
    } else if lower.starts_with(".debug") {
        "debug"
    } else if lower.starts_with(".reloc") {
        "relocations"
    } else if lower.starts_with(".tls") {
        "tls"
    } else {
        "other"
    }
}

#[derive(Debug, Serialize)]
/// Bucket.
pub struct Bucket {
    /// Name.
    pub name: String,
    /// Bytes.
    pub bytes: u64,
    /// Percent.
    pub percent: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Baseline bytes.
    pub baseline_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Delta.
    pub delta: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Growth percent.
    pub growth_percent: Option<f64>,
}

#[derive(Debug, Serialize)]
/// Size Report.
pub struct SizeReport {
    /// File.
    pub file: String,
    /// File size.
    pub file_size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Baseline.
    pub baseline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Baseline size.
    pub baseline_size: Option<u64>,
    /// Buckets.
    pub buckets: Vec<Bucket>,
    /// Exceeded.
    pub exceeded: Vec<String>,
}

fn bucket_sizes(pe: &Path) -> Result<(u64, BTreeMap<String, u64>)> {
    if !pe.is_file() {
        return Err(Error::NotExecutable {
            program: pe.to_path_buf(),
            detail: "file not found".into(),
        });
    }
    let file_size = std::fs::metadata(pe)
        .map(|m| m.len())
        .map_err(|e| Error::io(pe.to_path_buf(), e))?;
    let details = lsw_pe::details(pe)?;
    let mut buckets: BTreeMap<String, u64> = BTreeMap::new();
    let mut sectioned = 0u64;
    for s in &details.sections {
        *buckets.entry(bucket_for(&s.name).to_owned()).or_default() += u64::from(s.raw_size);
        sectioned = sectioned.saturating_add(u64::from(s.raw_size));
    }
    let overhead = file_size.saturating_sub(sectioned);
    if overhead > 0 {
        buckets.insert("overhead".to_owned(), overhead);
    }
    Ok((file_size, buckets))
}

/// Size.
pub fn size(pe: &Path, baseline: Option<&Path>, max_growth: Option<f64>) -> Result<SizeReport> {
    let (file_size, current) = bucket_sizes(pe)?;
    let base = baseline.map(bucket_sizes).transpose()?;

    let mut names: Vec<&str> = BUCKET_ORDER
        .iter()
        .copied()
        .filter(|n| {
            current.contains_key(*n) || base.as_ref().is_some_and(|(_, b)| b.contains_key(*n))
        })
        .collect();
    for extra in current.keys() {
        if !names.contains(&extra.as_str()) {
            names.push(extra);
        }
    }

    let mut buckets = Vec::new();
    let mut exceeded = Vec::new();
    for name in names {
        let bytes = current.get(name).copied().unwrap_or(0);
        let percent = if file_size > 0 {
            bytes as f64 * 100.0 / file_size as f64
        } else {
            0.0
        };
        let baseline_bytes = base
            .as_ref()
            .map(|(_, b)| b.get(name).copied().unwrap_or(0));
        let delta = baseline_bytes.map(|before| bytes as i64 - before as i64);
        let growth_percent = match (baseline_bytes, delta) {
            (Some(before), Some(d)) if before > 0 => Some(d as f64 * 100.0 / before as f64),
            _ => None,
        };
        if let Some(limit) = max_growth
            && let Some(before) = baseline_bytes
        {
            let over_limit = match growth_percent {
                Some(g) => g > limit,
                None => before == 0 && bytes > 0,
            };
            if over_limit {
                exceeded.push(name.to_owned());
            }
        }
        buckets.push(Bucket {
            name: name.to_owned(),
            bytes,
            percent,
            baseline_bytes,
            delta,
            growth_percent,
        });
    }

    Ok(SizeReport {
        file: pe.display().to_string(),
        file_size,
        baseline: baseline.map(|b| b.display().to_string()),
        baseline_size: base.map(|(len, _)| len),
        buckets,
        exceeded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_map_well_known_sections() {
        assert_eq!(bucket_for(".text"), "code");
        assert_eq!(bucket_for(".rdata"), "rodata");
        assert_eq!(bucket_for(".data"), "data");
        assert_eq!(bucket_for(".bss"), "data");
        assert_eq!(bucket_for(".rsrc"), "resources");
        assert_eq!(bucket_for(".pdata"), "exceptions");
        assert_eq!(bucket_for(".xdata"), "exceptions");
        assert_eq!(bucket_for(".debug_info"), "debug");
        assert_eq!(bucket_for(".reloc"), "relocations");
        assert_eq!(bucket_for(".idata"), "imports");
        assert_eq!(bucket_for(".CRT"), "other");
    }

    #[test]
    fn bucket_matching_is_case_insensitive_and_prefixed() {
        assert_eq!(bucket_for(".TEXT"), "code");
        assert_eq!(bucket_for(".text$mn"), "code");
        assert_eq!(bucket_for(".debug_line"), "debug");
    }
}
