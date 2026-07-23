use std::collections::BTreeMap;
use std::path::Path;

use object::{Object, ObjectSection};

use crate::error::{Error, Result};

pub(crate) struct DebugInfo {
    pub image_base: u64,
    lines: BTreeMap<(String, u32), Vec<u64>>,
    by_addr: Vec<(u64, String, u32)>,
    funcs: Vec<(u64, u64, String)>,
}

fn norm(path: &str) -> String {
    let p = path.replace('\\', "/");
    Path::new(&p)
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| p.to_lowercase())
}

impl DebugInfo {
    pub(crate) fn load(pe: &Path) -> Result<Self> {
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

        let mut lines: BTreeMap<(String, u32), Vec<u64>> = BTreeMap::new();
        let mut by_addr: Vec<(u64, String, u32)> = Vec::new();
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
                if row.end_sequence() {
                    continue;
                }
                let Some(line) = row.line() else { continue };
                let file = row
                    .file(header)
                    .and_then(|f| file_name(&dwarf, &unit, f, &comp_dir))
                    .unwrap_or_default();
                let addr = row.address();
                lines
                    .entry((norm(&file), line.get() as u32))
                    .or_default()
                    .push(addr);
                by_addr.push((addr, file, line.get() as u32));
            }
        }

        for v in lines.values_mut() {
            v.sort_unstable();
            v.dedup();
        }
        by_addr.sort_by_key(|(a, _, _)| *a);
        funcs.sort_by_key(|(a, _, _)| *a);

        Ok(Self {
            image_base,
            lines,
            by_addr,
            funcs,
        })
    }

    pub(crate) fn line_to_addr(&self, file: &str, line: u32) -> Option<u64> {
        let key = (norm(file), line);
        if let Some(v) = self.lines.get(&key) {
            return v.first().copied();
        }
        for delta in 1..=20 {
            if let Some(v) = self.lines.get(&(norm(file), line + delta)) {
                return v.first().copied();
            }
        }
        None
    }

    pub(crate) fn addr_to_line(&self, addr: u64) -> Option<(String, u32)> {
        let idx = match self.by_addr.binary_search_by_key(&addr, |(a, _, _)| *a) {
            Ok(i) => i,
            Err(0) => return None,
            Err(i) => i - 1,
        };
        let (_, file, line) = self.by_addr.get(idx)?;
        Some((file.clone(), *line))
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
        let low = match entry.attr_value(gimli::DW_AT_low_pc) {
            Ok(Some(gimli::AttributeValue::Addr(a))) => a,
            _ => continue,
        };
        let high = match entry.attr_value(gimli::DW_AT_high_pc) {
            Ok(Some(gimli::AttributeValue::Addr(a))) => a,
            Ok(Some(gimli::AttributeValue::Udata(n))) => low + n,
            _ => low + 1,
        };
        let name = entry
            .attr(gimli::DW_AT_name)
            .ok()
            .flatten()
            .and_then(|a| dwarf.attr_string(unit, a.value()).ok())
            .and_then(|s| s.to_string_lossy().ok().map(|c| c.into_owned()))
            .unwrap_or_else(|| "<anonymous>".to_owned());
        out.push((low, high, name));
    }
}

fn file_name<R: gimli::Reader>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    file: &gimli::FileEntry<R>,
    comp_dir: &str,
) -> Option<String> {
    let path = dwarf
        .attr_string(unit, file.path_name())
        .ok()?
        .to_string_lossy()
        .ok()?
        .into_owned();
    if path.starts_with('/') || path.contains(":\\") || path.contains(":/") {
        Some(path)
    } else {
        Some(format!("{comp_dir}/{path}"))
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
}
