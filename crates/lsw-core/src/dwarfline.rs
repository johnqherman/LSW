use std::collections::BTreeMap;
use std::path::Path;

use object::{Object, ObjectSection};

use crate::error::{Error, Result};

const MAX_LINE_ROWS: usize = 8_000_000;
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

        let mut units = dwarf.units();
        while let Ok(Some(header)) = units.next() {
            let Ok(unit) = dwarf.unit(header) else {
                continue;
            };
            collect_functions(&dwarf, &unit, &mut funcs);
            let Some(program) = unit.line_program.clone() else {
                continue;
            };
            let comp_dir = unit
                .comp_dir
                .map(|d| d.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut rows = program.rows();
            while let Ok(Some((header, row))) = rows.next_row() {
                if by_addr.len() >= MAX_LINE_ROWS {
                    break;
                }
                if row.end_sequence() {
                    by_addr.push((row.address(), None));
                    continue;
                }
                let Some(line) = row.line() else { continue };
                let file = row
                    .file(header)
                    .and_then(|f| file_name(&dwarf, &unit, header, f, &comp_dir))
                    .unwrap_or_default();
                let addr = row.address();
                let line = line.get() as u32;
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
        by_addr.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.is_none().cmp(&b.1.is_none())));
        by_addr.dedup_by_key(|(a, _)| *a);
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

    pub(crate) fn addr_to_func(&self, addr: u64) -> Option<String> {
        let idx = match self.funcs.binary_search_by_key(&addr, |(a, _, _)| *a) {
            Ok(i) => i,
            Err(0) => return None,
            Err(i) => i - 1,
        };
        let (low, high, name) = self.funcs.get(idx)?;
        if addr >= *low && addr < *high {
            Some(name.clone())
        } else {
            None
        }
    }

    pub(crate) fn func_range(&self, addr: u64) -> Option<(u64, u64)> {
        let idx = match self.funcs.binary_search_by_key(&addr, |(a, _, _)| *a) {
            Ok(i) => i,
            Err(0) => return None,
            Err(i) => i - 1,
        };
        let (low, high, _) = self.funcs.get(idx)?;
        if addr >= *low && addr < *high {
            Some((*low, *high))
        } else {
            None
        }
    }
}

fn collect_functions<R: gimli::Reader>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    out: &mut Vec<(u64, u64, String)>,
) {
    let mut entries = unit.entries();
    while let Ok(Some((_, entry))) = entries.next_dfs() {
        if entry.tag() != gimli::DW_TAG_subprogram {
            continue;
        }
        let name = func_name(dwarf, unit, entry).unwrap_or_else(|| "<anonymous>".to_owned());
        let Ok(mut ranges) = dwarf.die_ranges(unit, entry) else {
            continue;
        };
        while let Ok(Some(range)) = ranges.next() {
            if range.end > range.begin {
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

fn ref_attr<R: gimli::Reader>(
    entry: &gimli::DebuggingInformationEntry<R>,
    attr: gimli::DwAt,
) -> Option<gimli::UnitOffset<R::Offset>> {
    match entry.attr_value(attr) {
        Ok(Some(gimli::AttributeValue::UnitRef(offset))) => Some(offset),
        _ => None,
    }
}

fn func_name<R: gimli::Reader>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    entry: &gimli::DebuggingInformationEntry<R>,
) -> Option<String> {
    if let Some(name) = die_name(dwarf, unit, entry) {
        return Some(name);
    }
    let mut next = ref_attr(entry, gimli::DW_AT_specification)
        .or_else(|| ref_attr(entry, gimli::DW_AT_abstract_origin));
    for _ in 0..4 {
        let offset = next?;
        let referenced = unit.entry(offset).ok()?;
        if let Some(name) = die_name(dwarf, unit, &referenced) {
            return Some(name);
        }
        next = ref_attr(&referenced, gimli::DW_AT_specification)
            .or_else(|| ref_attr(&referenced, gimli::DW_AT_abstract_origin));
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
        Some(dir) if !dir.is_empty() => Some(format!("{comp_dir}/{dir}/{path}")),
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
