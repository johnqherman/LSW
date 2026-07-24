use std::path::Path;

use crate::error::{Error, Result};

fn is_printable(b: u8) -> bool {
    (0x20..=0x7e).contains(&b) || b == b'\t'
}

pub fn extract_strings(data: &[u8], min_len: usize) -> Vec<String> {
    let mut out = Vec::new();

    let mut cur = String::new();
    for &b in data {
        if is_printable(b) {
            cur.push(b as char);
        } else {
            if cur.len() >= min_len {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        }
    }
    if cur.len() >= min_len {
        out.push(std::mem::take(&mut cur));
    }

    for start in [0usize, 1] {
        let mut cur = String::new();
        let mut i = start;
        while i + 1 < data.len() {
            let lo = data[i];
            let hi = data[i + 1];
            if hi == 0 && is_printable(lo) {
                cur.push(lo as char);
            } else if cur.len() >= min_len {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
            i += 2;
        }
        if cur.len() >= min_len {
            out.push(std::mem::take(&mut cur));
        }
    }

    out
}

pub fn strings(path: &Path, min_len: usize) -> Result<Vec<String>> {
    use std::io::Read;
    const MAX_INPUT: u64 = 256 * 1024 * 1024;
    let file = std::fs::File::open(path).map_err(|e| Error::io(path.to_path_buf(), e))?;
    let mut data = Vec::new();
    file.take(MAX_INPUT)
        .read_to_end(&mut data)
        .map_err(|e| Error::io(path.to_path_buf(), e))?;
    Ok(extract_strings(&data, min_len.max(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_ascii_and_utf16_runs_over_min_len() {
        let data = b"hi\x00\x00Hello\x00\x00world!!";
        let s = extract_strings(data, 4);
        assert!(s.contains(&"Hello".to_owned()));
        assert!(s.contains(&"world!!".to_owned()));
        assert!(!s.iter().any(|x| x == "hi"));

        let wide = b"H\x00e\x00l\x00p\x00\x00\x00";
        assert!(extract_strings(wide, 4).contains(&"Help".to_owned()));

        let odd = b"\x01H\x00e\x00l\x00p\x00\x00";
        assert!(extract_strings(odd, 4).contains(&"Help".to_owned()));
    }
}
