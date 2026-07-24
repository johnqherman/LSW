use std::path::{Path, PathBuf};

use base64::Engine as _;
use sha2::{Digest, Sha256};

use lsw_config::{Dirs, TargetArch};

use crate::buildops::which;
use crate::error::{Error, Result};
use crate::project::Project;

const BLOCK_SIZE: usize = 65536;
const MAX_MSIX_ARTIFACT: u64 = 4 * 1024 * 1024 * 1024;

pub fn build_msix(
    project: &Project,
    arch: TargetArch,
    dist: &Path,
    dir: &Path,
    stem: &str,
    files: &[String],
) -> Result<PathBuf> {
    if which("zip").is_none() {
        return Err(Error::ToolMissing {
            tool: "zip".into(),
            fix: "install zip, or use --target zip".into(),
        });
    }
    let name = &project.manifest.project.name;
    let publisher = "CN=LSW Self-Signed (Development)";

    let logo = "lsw-appx-logo.png";
    const RESERVED: &[&str] = &[
        "AppxManifest.xml",
        "AppxBlockMap.xml",
        "[Content_Types].xml",
    ];
    for f in files {
        let lower = f.to_ascii_lowercase();
        if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(f)) || lower == logo {
            return Err(Error::MsixSign {
                detail: format!("build artifact '{f}' collides with a reserved MSIX package file"),
            });
        }
    }
    std::fs::write(dir.join(logo), minimal_png()).map_err(|e| Error::io(dir.join(logo), e))?;

    let entry = files
        .iter()
        .find(|f| {
            std::path::Path::new(f)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        })
        .cloned()
        .ok_or(Error::MsixSign {
            detail: "MSIX needs an executable; this build produced no .exe (a DLL-only project cannot form a launchable package)".into(),
        })?;
    std::fs::write(
        dir.join("AppxManifest.xml"),
        manifest_xml(name, publisher, arch, &entry, logo),
    )
    .map_err(|e| Error::io(dir.join("AppxManifest.xml"), e))?;

    let mut block_files = vec!["AppxManifest.xml".to_owned(), logo.to_owned()];
    block_files.extend(files.iter().cloned());
    let block_map = block_map_xml(dir, &block_files)?;
    std::fs::write(dir.join("AppxBlockMap.xml"), block_map)
        .map_err(|e| Error::io(dir.join("AppxBlockMap.xml"), e))?;

    std::fs::write(dir.join("[Content_Types].xml"), content_types_xml(files))
        .map_err(|e| Error::io(dir.join("[Content_Types].xml"), e))?;

    let unsigned = dist.join(format!("{stem}.unsigned.msix"));
    let _ = std::fs::remove_file(&unsigned);
    let mut zip_args = vec![
        "-q".to_owned(),
        "-X".to_owned(),
        "-0".to_owned(),
        "-r".to_owned(),
        std::path::absolute(&unsigned)
            .unwrap_or(unsigned.clone())
            .display()
            .to_string(),
        "[Content_Types].xml".to_owned(),
        "AppxBlockMap.xml".to_owned(),
        "AppxManifest.xml".to_owned(),
        logo.to_owned(),
    ];
    zip_args.extend(files.iter().map(|f| {
        if f.starts_with('-') {
            format!("./{f}")
        } else {
            f.clone()
        }
    }));
    let status = std::process::Command::new("zip")
        .args(&zip_args)
        .current_dir(dir)
        .status()
        .map_err(|e| Error::io(PathBuf::from("zip"), e))?;
    if !status.success() {
        return Err(Error::BuildFailed {
            command: "zip (msix)".into(),
            code: status.code(),
        });
    }

    let msix = dist.join(format!("{stem}.msix"));
    let _ = std::fs::remove_file(&msix);
    authenticode_sign(&unsigned, &msix, publisher)?;
    let _ = std::fs::remove_file(&unsigned);
    Ok(msix)
}

pub fn authenticode_sign(unsigned: &Path, out: &Path, publisher: &str) -> Result<()> {
    if which("osslsigncode").is_none() {
        return Err(Error::ToolMissing {
            tool: "osslsigncode".into(),
            fix: "install osslsigncode (AUR or https://github.com/mtrojnar/osslsigncode) to sign MSIX packages".into(),
        });
    }
    let pfx = ensure_signing_identity(publisher)?;
    let output = std::process::Command::new("osslsigncode")
        .arg("sign")
        .args(["-pkcs12"])
        .arg(&pfx)
        .args(["-pass", "lsw"])
        .arg("-in")
        .arg(unsigned)
        .arg("-out")
        .arg(out)
        .output()
        .map_err(|e| Error::io(PathBuf::from("osslsigncode"), e))?;
    if !output.status.success() {
        return Err(Error::MsixSign {
            detail: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(())
}

fn ensure_signing_identity(publisher: &str) -> Result<PathBuf> {
    use sha2::Digest;
    let dirs = Dirs::resolve()?;
    let msix_dir = dirs.data.join("msix");
    std::fs::create_dir_all(&msix_dir).map_err(|e| Error::io(msix_dir.clone(), e))?;
    let tag = format!("{:x}", sha2::Sha256::digest(publisher.as_bytes()));
    let tag = &tag[..16];
    let pfx = msix_dir.join(format!("signing-{tag}.pfx"));
    let cert = msix_dir.join(format!("signing-{tag}.cert.pem"));
    if pfx.is_file() && cert_still_valid(&cert) {
        return Ok(pfx);
    }
    if which("openssl").is_none() {
        return Err(Error::ToolMissing {
            tool: "openssl".into(),
            fix: "install openssl to generate a signing certificate".into(),
        });
    }
    let stamp = std::process::id();
    let key_tmp = msix_dir.join(format!("signing-{tag}.key.{stamp}.tmp"));
    let cert_tmp = msix_dir.join(format!("signing-{tag}.cert.{stamp}.tmp"));
    let pfx_tmp = msix_dir.join(format!("signing-{tag}.pfx.{stamp}.tmp"));
    run_openssl(&[
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-keyout",
        &key_tmp.display().to_string(),
        "-out",
        &cert_tmp.display().to_string(),
        "-days",
        "3650",
        "-nodes",
        "-subj",
        &openssl_subj(publisher),
        "-addext",
        "extendedKeyUsage=codeSigning",
    ])?;
    run_openssl(&[
        "pkcs12",
        "-export",
        "-out",
        &pfx_tmp.display().to_string(),
        "-inkey",
        &key_tmp.display().to_string(),
        "-in",
        &cert_tmp.display().to_string(),
        "-passout",
        "pass:lsw",
    ])?;
    std::fs::rename(&pfx_tmp, &pfx).map_err(|e| Error::io(pfx.clone(), e))?;
    std::fs::rename(&cert_tmp, &cert).map_err(|e| Error::io(cert.clone(), e))?;
    let _ = std::fs::remove_file(&key_tmp);
    Ok(pfx)
}

fn cert_still_valid(cert: &Path) -> bool {
    if !cert.is_file() {
        return false;
    }
    if which("openssl").is_none() {
        return true;
    }
    std::process::Command::new("openssl")
        .args(["x509", "-checkend", "2592000", "-noout", "-in"])
        .arg(cert)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn openssl_subj(dn: &str) -> String {
    let mut out = String::new();
    for rdn in dn.split(',') {
        let rdn = rdn.trim();
        if rdn.is_empty() {
            continue;
        }
        let escape = |s: &str| s.replace('\\', "\\\\").replace('/', "\\/");
        out.push('/');
        if let Some((attr, val)) = rdn.split_once('=') {
            out.push_str(attr.trim());
            out.push('=');
            out.push_str(&escape(val));
        } else {
            out.push_str(&escape(rdn));
        }
    }
    if out.is_empty() {
        out.push_str("/CN=LSW");
    }
    out
}

fn run_openssl(args: &[&str]) -> Result<()> {
    let output = std::process::Command::new("openssl")
        .args(args)
        .output()
        .map_err(|e| Error::io(PathBuf::from("openssl"), e))?;
    if !output.status.success() {
        return Err(Error::MsixSign {
            detail: format!(
                "openssl {} failed: {}",
                args.first().copied().unwrap_or(""),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    Ok(())
}

fn minimal_png() -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==")
        .expect("static PNG decodes")
}

fn manifest_xml(name: &str, publisher: &str, arch: TargetArch, entry: &str, logo: &str) -> String {
    let proc_arch = match arch {
        TargetArch::X86_64 => "x64",
        TargetArch::X86 => "x86",
        TargetArch::Aarch64 | TargetArch::Arm64Ec => "arm64",
        TargetArch::Armv7 => "arm",
    };
    let ident = sanitize_identity(name);
    let name = crate::xml_escape(name);
    let entry = crate::xml_escape(entry);
    let publisher = crate::xml_escape(publisher);
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
<Package xmlns=\"http://schemas.microsoft.com/appx/manifest/foundation/windows10\" \
xmlns:uap=\"http://schemas.microsoft.com/appx/manifest/uap/windows10\" \
xmlns:rescap=\"http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities\">\n\
  <Identity Name=\"{ident}\" Publisher=\"{publisher}\" Version=\"1.0.0.0\" ProcessorArchitecture=\"{proc_arch}\"/>\n\
  <Properties>\n\
    <DisplayName>{name}</DisplayName>\n\
    <PublisherDisplayName>LSW</PublisherDisplayName>\n\
    <Logo>{logo}</Logo>\n\
  </Properties>\n\
  <Dependencies>\n\
    <TargetDeviceFamily Name=\"Windows.Desktop\" MinVersion=\"10.0.17763.0\" MaxVersionTested=\"10.0.22621.0\"/>\n\
  </Dependencies>\n\
  <Capabilities>\n\
    <rescap:Capability Name=\"runFullTrust\"/>\n\
  </Capabilities>\n\
  <Applications>\n\
    <Application Id=\"App\" Executable=\"{entry}\" EntryPoint=\"Windows.FullTrustApplication\">\n\
      <uap:VisualElements DisplayName=\"{name}\" Description=\"{name}\" BackgroundColor=\"#464646\" \
Square150x150Logo=\"{logo}\" Square44x44Logo=\"{logo}\"/>\n\
    </Application>\n\
  </Applications>\n\
</Package>\n"
    )
}

fn sanitize_identity(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed: String = cleaned.trim_matches(['-', '.']).chars().take(46).collect();
    let base = trimmed.trim_end_matches(['-', '.']);
    let ident = if base.is_empty() {
        "LSW.app".to_owned()
    } else {
        format!("LSW.{base}")
    };
    ident
        .split('.')
        .map(|seg| {
            if seg.len() >= 4 && seg[..4].eq_ignore_ascii_case("xn--") {
                format!("x{seg}")
            } else {
                seg.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn block_map_xml(dir: &Path, files: &[String]) -> Result<String> {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<BlockMap xmlns=\"http://schemas.microsoft.com/appx/2010/blockmap\" \
HashMethod=\"http://www.w3.org/2001/04/xmlenc#sha256\">\n",
    );
    for file in files {
        use std::io::Read;
        let path = dir.join(file);
        let size = std::fs::metadata(&path)
            .map_err(|e| Error::io(path.clone(), e))?
            .len();
        if size > MAX_MSIX_ARTIFACT {
            return Err(Error::MsixSign {
                detail: format!(
                    "artifact '{file}' is {size} bytes, exceeds the {MAX_MSIX_ARTIFACT}-byte MSIX limit"
                ),
            });
        }
        let lfh = 30 + file.len();
        out.push_str(&format!(
            "  <File Name=\"{}\" Size=\"{}\" LfhSize=\"{}\">\n",
            crate::xml_escape(&file.replace('/', "\\")),
            size,
            lfh
        ));
        let mut reader = std::io::BufReader::new(
            std::fs::File::open(&path).map_err(|e| Error::io(path.clone(), e))?,
        );
        let mut block = vec![0u8; BLOCK_SIZE];
        let mut emitted = false;
        loop {
            let mut filled = 0;
            while filled < BLOCK_SIZE {
                let n = reader
                    .read(&mut block[filled..])
                    .map_err(|e| Error::io(path.clone(), e))?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
            if filled == 0 {
                break;
            }
            emitted = true;
            let digest = Sha256::digest(&block[..filled]);
            let hash = base64::engine::general_purpose::STANDARD.encode(digest);
            out.push_str(&format!("    <Block Hash=\"{hash}\"/>\n"));
            if filled < BLOCK_SIZE {
                break;
            }
        }
        if !emitted {
            let hash = base64::engine::general_purpose::STANDARD.encode(Sha256::digest([]));
            out.push_str(&format!("    <Block Hash=\"{hash}\"/>\n"));
        }
        out.push_str("  </File>\n");
    }
    out.push_str("</BlockMap>\n");
    Ok(out)
}

fn content_types_xml(files: &[String]) -> String {
    let mut exts: std::collections::BTreeSet<String> = files
        .iter()
        .filter_map(|f| f.rsplit('.').next().map(|e| e.to_ascii_lowercase()))
        .collect();
    exts.insert("xml".to_owned());
    exts.insert("png".to_owned());
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n",
    );
    for ext in &exts {
        let ct = match ext.as_str() {
            "xml" => "application/vnd.ms-appx.manifest+xml",
            "png" => "image/png",
            _ => "application/octet-stream",
        };
        out.push_str(&format!(
            "  <Default Extension=\"{}\" ContentType=\"{ct}\"/>\n",
            crate::xml_escape(ext)
        ));
    }
    out.push_str(
        "  <Override PartName=\"/AppxBlockMap.xml\" ContentType=\"application/vnd.ms-appx.blockmap+xml\"/>\n\
  <Override PartName=\"/AppxSignature.p7x\" ContentType=\"application/vnd.ms-appx.signature\"/>\n\
</Types>\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_identity_produces_valid_name() {
        assert_eq!(sanitize_identity("hello world!"), "LSW.hello-world");
        assert_eq!(sanitize_identity("my.app-1"), "LSW.my.app-1");
    }

    #[test]
    fn block_map_hashes_blocks() {
        let tmp = tempfile::tempdir().unwrap();
        let data = vec![7u8; BLOCK_SIZE * 2 + 10];
        std::fs::write(tmp.path().join("app.exe"), &data).unwrap();
        let xml = block_map_xml(tmp.path(), &["app.exe".to_owned()]).unwrap();
        assert_eq!(xml.matches("<Block ").count(), 3);
        assert!(xml.contains("Name=\"app.exe\""));
        assert!(xml.contains(&format!("Size=\"{}\"", data.len())));
    }

    #[test]
    fn content_types_covers_payload_and_overrides() {
        let ct = content_types_xml(&["app.exe".to_owned(), "lib.dll".to_owned()]);
        assert!(ct.contains("Extension=\"exe\""));
        assert!(ct.contains("Extension=\"dll\""));
        assert!(ct.contains("Extension=\"png\" ContentType=\"image/png\""));
        assert!(ct.contains("/AppxSignature.p7x"));
        assert!(ct.contains("/AppxBlockMap.xml"));
    }

    #[test]
    fn manifest_has_identity_and_application() {
        let m = manifest_xml(
            "Hello",
            "CN=Test",
            TargetArch::X86_64,
            "hello.exe",
            "logo.png",
        );
        assert!(m.contains("Publisher=\"CN=Test\""));
        assert!(m.contains("ProcessorArchitecture=\"x64\""));
        assert!(m.contains("Executable=\"hello.exe\""));
        assert!(m.contains("Name=\"LSW.Hello\""));
        assert!(m.contains("<Logo>logo.png</Logo>"));
    }

    #[test]
    fn minimal_png_is_valid_png() {
        let png = minimal_png();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }
}
