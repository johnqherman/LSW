use std::collections::BTreeMap;
use std::path::Path;

use object::{Object, ObjectSection};

use crate::error::{Error, Result};

const MAX_LINE_ROWS: usize = 8_000_000;
const MAX_DWARF_NAME: usize = 4096;

fn cap_len(mut s: String) -> String {
    if s.len() > MAX_DWARF_NAME {
        let mut end = MAX_DWARF_NAME;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
    s
}
const MAX_FUNCS: usize = 2_000_000;
const MAX_UNIT_SCAN: usize = 100_000;
const MAX_STRING_BYTES: usize = 256 * 1024 * 1024;
const MAX_PE_BYTES: u64 = 512 * 1024 * 1024;

pub(crate) struct DebugInfo {
    pub image_base: u64,
    lines: BTreeMap<(String, u32), Vec<(String, u64)>>,
    by_addr: Vec<(u64, Option<(String, u32)>)>,
    funcs: Vec<(u64, u64, String)>,
}

fn norm(path: &str) -> String {
    let p = path.replace('\\', "/");
    Path::new(&p)
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| p.to_lowercase())
}

fn suffix_score(candidate: &str, requested: &str) -> usize {
    let split = |s: &str| -> Vec<String> {
        s.replace('\\', "/")
            .split('/')
            .filter(|c| !c.is_empty())
            .map(|c| c.to_lowercase())
            .collect()
    };
    let a = split(candidate);
    let b = split(requested);
    a.iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
}

impl DebugInfo {
    pub(crate) fn load(pe: &Path) -> Result<Self> {
        let len = std::fs::metadata(pe)
            .map_err(|e| Error::io(pe.to_path_buf(), e))?
            .len();
        if len > MAX_PE_BYTES {
            return Err(dap(format!(
                "{} is {len} bytes, over the {MAX_PE_BYTES}-byte debug-info limit",
                pe.display()
            )));
        }
        let data = std::fs::read(pe).map_err(|e| Error::io(pe.to_path_buf(), e))?;
        let file = object::File::parse(&*data)
            .map_err(|e| dap(format!("cannot parse {}: {e}", pe.display())))?;
        let image_base = file.relative_address_base();

        let endian = gimli::RunTimeEndian::Little;
        let load = |id: gimli::SectionId| -> std::result::Result<std::borrow::Cow<[u8]>, ()> {
            Ok(match file.section_by_name(id.name()) {
                Some(section) => section.uncompressed_data().unwrap_or_default(),
                None => std::borrow::Cow::Borrowed(&[]),
            })
        };
        let sections =
            gimli::DwarfSections::load(load).map_err(|_: ()| dap("no DWARF sections"))?;
        let dwarf = sections.borrow(|section| gimli::EndianSlice::new(section, endian));

        let mut lines: BTreeMap<(String, u32), Vec<(String, u64)>> = BTreeMap::new();
        let mut by_addr: Vec<(u64, Option<(String, u32)>)> = Vec::new();
        let mut funcs: Vec<(u64, u64, String)> = Vec::new();
        let mut func_visited = 0usize;
        let mut rows_seen = 0usize;
        let mut scan_budget = MAX_UNIT_SCAN;
        let mut string_bytes = 0usize;

        let mut units = dwarf.units();
        while let Ok(Some(header)) = units.next() {
            if (func_visited >= MAX_FUNCS && rows_seen >= MAX_LINE_ROWS && scan_budget == 0)
                || string_bytes >= MAX_STRING_BYTES
            {
                break;
            }
            let Ok(unit) = dwarf.unit(header) else {
                continue;
            };
            collect_functions(
                &dwarf,
                &unit,
                &mut funcs,
                &mut func_visited,
                &mut scan_budget,
                &mut string_bytes,
            );
            let Some(program) = unit.line_program.clone() else {
                continue;
            };
            let comp_dir = unit
                .comp_dir
                .map(|d| d.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut rows = program.rows();
            while let Ok(Some((header, row))) = rows.next_row() {
                if rows_seen >= MAX_LINE_ROWS {
                    break;
                }
                rows_seen += 1;
                if by_addr.len() >= MAX_LINE_ROWS || string_bytes >= MAX_STRING_BYTES {
                    break;
                }
                if row.end_sequence() {
                    by_addr.push((row.address(), None));
                    continue;
                }
                let Some(line) = row.line() else { continue };
                let file = cap_len(
                    row.file(header)
                        .and_then(|f| file_name(&dwarf, &unit, header, f, &comp_dir))
                        .unwrap_or_default(),
                );
                let addr = row.address();
                let line = line.get() as u32;
                string_bytes = string_bytes.saturating_add(file.len().saturating_mul(2));
                lines
                    .entry((norm(&file), line))
                    .or_default()
                    .push((file.clone(), addr));
                by_addr.push((addr, Some((file, line))));
            }
        }

        for v in lines.values_mut() {
            v.sort_by_key(|(_, a)| *a);
            v.dedup();
        }
        by_addr.sort_by_key(|(a, _)| *a);
        let mut collapsed: Vec<(u64, Option<(String, u32)>)> = Vec::with_capacity(by_addr.len());
        for entry in by_addr {
            match collapsed.last_mut() {
                Some(last) if last.0 == entry.0 => {
                    if entry.1.is_some() {
                        *last = entry;
                    }
                }
                _ => collapsed.push(entry),
            }
        }
        by_addr = collapsed;
        funcs.sort_by_key(|(a, _, _)| *a);

        Ok(Self {
            image_base,
            lines,
            by_addr,
            funcs,
        })
    }

    pub(crate) fn line_to_addr(&self, file: &str, line: u32) -> Option<u64> {
        let mut best: Option<(usize, u32, u64)> = None;
        for delta in 0..=20 {
            let Some(target) = line.checked_add(delta) else {
                break;
            };
            let Some(v) = self.lines.get(&(norm(file), target)) else {
                continue;
            };
            for (path, addr) in v {
                let score = suffix_score(path, file);
                let better = match best {
                    None => true,
                    Some((bs, bd, ba)) => score > bs || (score == bs && (delta, *addr) < (bd, ba)),
                };
                if better {
                    best = Some((score, delta, *addr));
                }
            }
        }
        best.map(|(_, _, addr)| addr)
    }

    pub(crate) fn addr_to_line(&self, addr: u64) -> Option<(String, u32)> {
        let idx = match self.by_addr.binary_search_by_key(&addr, |(a, _)| *a) {
            Ok(i) => i,
            Err(0) => return None,
            Err(i) => i - 1,
        };
        self.by_addr.get(idx)?.1.clone()
    }

    fn containing_func(&self, addr: u64) -> Option<&(u64, u64, String)> {
        const MAX_OVERLAP_SCAN: usize = 10_000;
        let idx = match self.funcs.binary_search_by_key(&addr, |(a, _, _)| *a) {
            Ok(i) => i,
            Err(0) => return None,
            Err(i) => i - 1,
        };
        let start = idx.saturating_sub(MAX_OVERLAP_SCAN);
        let mut best: Option<&(u64, u64, String)> = None;
        for f in self.funcs[start..=idx].iter().rev() {
            if f.0 <= addr && addr < f.1 {
                match best {
                    Some(b) if (b.1 - b.0) <= (f.1 - f.0) => {}
                    _ => best = Some(f),
                }
            }
        }
        best
    }

    pub(crate) fn addr_to_func(&self, addr: u64) -> Option<String> {
        self.containing_func(addr).map(|(_, _, name)| name.clone())
    }

    pub(crate) fn func_range(&self, addr: u64) -> Option<(u64, u64)> {
        self.containing_func(addr)
            .map(|(low, high, _)| (*low, *high))
    }
}

fn collect_functions<R: gimli::Reader>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    out: &mut Vec<(u64, u64, String)>,
    visited: &mut usize,
    scan_budget: &mut usize,
    string_bytes: &mut usize,
) {
    let mut entries = unit.entries();
    while let Ok(Some((_, entry))) = entries.next_dfs() {
        if out.len() >= MAX_FUNCS || *visited >= MAX_FUNCS || *string_bytes >= MAX_STRING_BYTES {
            return;
        }
        if entry.tag() != gimli::DW_TAG_subprogram {
            continue;
        }
        *visited += 1;
        let name = cap_len(
            func_name(dwarf, unit, entry, scan_budget).unwrap_or_else(|| "<anonymous>".to_owned()),
        );
        let Ok(mut ranges) = dwarf.die_ranges(unit, entry) else {
            continue;
        };
        let mut range_seen = 0usize;
        while let Ok(Some(range)) = ranges.next() {
            if out.len() >= MAX_FUNCS
                || range_seen >= MAX_FUNCS
                || *string_bytes >= MAX_STRING_BYTES
            {
                return;
            }
            range_seen += 1;
            if range.end > range.begin {
                *string_bytes = string_bytes.saturating_add(name.len());
                out.push((range.begin, range.end, name.clone()));
            }
        }
    }
}

fn die_name<R: gimli::Reader>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    entry: &gimli::DebuggingInformationEntry<R>,
) -> Option<String> {
    let attr = entry.attr(gimli::DW_AT_name).ok().flatten()?;
    dwarf
        .attr_string(unit, attr.value())
        .ok()?
        .to_string_lossy()
        .ok()
        .map(|c| c.into_owned())
}

enum OriginRef<R: gimli::Reader> {
    Unit(gimli::UnitOffset<R::Offset>),
    Info(gimli::DebugInfoOffset<R::Offset>),
}

fn origin_ref<R: gimli::Reader>(
    entry: &gimli::DebuggingInformationEntry<R>,
) -> Option<OriginRef<R>> {
    for attr in [gimli::DW_AT_specification, gimli::DW_AT_abstract_origin] {
        match entry.attr_value(attr) {
            Ok(Some(gimli::AttributeValue::UnitRef(offset))) => {
                return Some(OriginRef::Unit(offset));
            }
            Ok(Some(gimli::AttributeValue::DebugInfoRef(offset))) => {
                return Some(OriginRef::Info(offset));
            }
            _ => {}
        }
    }
    None
}

fn func_name<R: gimli::Reader>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    entry: &gimli::DebuggingInformationEntry<R>,
    scan_budget: &mut usize,
) -> Option<String> {
    func_name_at(dwarf, unit, entry, 0, scan_budget)
}

fn func_name_at<R: gimli::Reader>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    entry: &gimli::DebuggingInformationEntry<R>,
    depth: u32,
    scan_budget: &mut usize,
) -> Option<String> {
    if depth > 8 {
        return None;
    }
    if let Some(name) = die_name(dwarf, unit, entry) {
        return Some(name);
    }
    let mut next = origin_ref(entry);
    for _ in 0..4 {
        match next? {
            OriginRef::Unit(offset) => {
                let referenced = unit.entry(offset).ok()?;
                if let Some(name) = die_name(dwarf, unit, &referenced) {
                    return Some(name);
                }
                next = origin_ref(&referenced);
            }
            OriginRef::Info(offset) => {
                return name_at_debug_info(dwarf, offset, depth + 1, scan_budget);
            }
        }
    }
    None
}

fn name_at_debug_info<R: gimli::Reader>(
    dwarf: &gimli::Dwarf<R>,
    offset: gimli::DebugInfoOffset<R::Offset>,
    depth: u32,
    scan_budget: &mut usize,
) -> Option<String> {
    if depth > 8 {
        return None;
    }
    let mut units = dwarf.units();
    while let Ok(Some(header)) = units.next() {
        if *scan_budget == 0 {
            return None;
        }
        *scan_budget -= 1;
        let Some(unit_offset) = offset.to_unit_offset(&header) else {
            continue;
        };
        let Ok(unit) = dwarf.unit(header) else {
            continue;
        };
        let entry = unit.entry(unit_offset).ok()?;
        return func_name_at(dwarf, &unit, &entry, depth + 1, scan_budget);
    }
    None
}

fn is_absolute(path: &str) -> bool {
    path.starts_with('/') || path.contains(":\\") || path.contains(":/")
}

fn file_name<R: gimli::Reader>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    header: &gimli::LineProgramHeader<R>,
    file: &gimli::FileEntry<R>,
    comp_dir: &str,
) -> Option<String> {
    let path = dwarf
        .attr_string(unit, file.path_name())
        .ok()?
        .to_string_lossy()
        .ok()?
        .into_owned();
    if is_absolute(&path) {
        return Some(path);
    }
    let dir = header.directory(file.directory_index()).and_then(|d| {
        dwarf
            .attr_string(unit, d)
            .ok()?
            .to_string_lossy()
            .ok()
            .map(|c| c.into_owned())
    });
    match dir {
        Some(dir) if is_absolute(&dir) => Some(format!("{dir}/{path}")),
        Some(dir) if !dir.is_empty() && dir != comp_dir => Some(format!("{comp_dir}/{dir}/{path}")),
        _ => Some(format!("{comp_dir}/{path}")),
    }
}

fn dap(detail: impl Into<String>) -> Error {
    Error::Dap {
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_takes_lowercased_basename() {
        assert_eq!(norm("C:\\src\\Main.c"), "main.c");
        assert_eq!(norm("/home/u/proj/foo.rs"), "foo.rs");
    }

    #[test]
    fn suffix_score_prefers_longer_path_match() {
        assert_eq!(suffix_score("/a/b/util.c", "/x/b/util.c"), 2);
        assert_eq!(suffix_score("/a/client/util.c", "/z/server/util.c"), 1);
        assert_eq!(suffix_score("C:\\proj\\main.c", "/proj/main.c"), 2);
    }

    #[test]
    fn line_to_addr_disambiguates_same_basename() {
        let info = DebugInfo {
            image_base: 0,
            lines: BTreeMap::from([(
                ("util.c".to_owned(), 10),
                vec![
                    ("/src/client/util.c".to_owned(), 0x2000),
                    ("/src/server/util.c".to_owned(), 0x1000),
                ],
            )]),
            by_addr: Vec::new(),
            funcs: Vec::new(),
        };
        assert_eq!(info.line_to_addr("/src/client/util.c", 10), Some(0x2000));
        assert_eq!(info.line_to_addr("/src/server/util.c", 10), Some(0x1000));
    }

    #[test]
    fn line_to_addr_high_line_does_not_overflow() {
        let info = DebugInfo {
            image_base: 0,
            lines: BTreeMap::new(),
            by_addr: Vec::new(),
            funcs: Vec::new(),
        };
        assert_eq!(info.line_to_addr("a.c", u32::MAX), None);
        assert_eq!(info.line_to_addr("a.c", u32::MAX - 5), None);
    }

    #[test]
    fn line_to_addr_ranks_suffix_over_delta() {
        let info = DebugInfo {
            image_base: 0,
            lines: BTreeMap::from([
                (
                    ("foo.c".to_owned(), 11),
                    vec![("/b/foo.c".to_owned(), 0x200)],
                ),
                (
                    ("foo.c".to_owned(), 12),
                    vec![("/a/foo.c".to_owned(), 0x300)],
                ),
            ]),
            by_addr: Vec::new(),
            funcs: Vec::new(),
        };
        assert_eq!(info.line_to_addr("/a/foo.c", 10), Some(0x300));
    }

    #[test]
    fn addr_to_line_respects_sequence_gaps() {
        let info = DebugInfo {
            image_base: 0,
            lines: BTreeMap::new(),
            by_addr: vec![
                (0x1000, Some(("a.c".to_owned(), 5))),
                (0x1010, None),
                (0x2000, Some(("a.c".to_owned(), 6))),
            ],
            funcs: Vec::new(),
        };
        assert_eq!(info.addr_to_line(0x1004), Some(("a.c".to_owned(), 5)));
        assert_eq!(info.addr_to_line(0x1800), None);
        assert_eq!(info.addr_to_line(0x2004), Some(("a.c".to_owned(), 6)));
    }
}
