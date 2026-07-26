use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use lsw_config::{PackageSection, ResolvedToolchain, TargetArch};

use crate::envops::Environment;
use crate::error::{Error, Result};
use crate::project::Project;

fn wants_resources(pkg: &PackageSection) -> bool {
    pkg.icon.is_some()
        || pkg.version.is_some()
        || pkg.description.is_some()
        || pkg.publisher.is_some()
        || pkg.dpi_aware.is_some()
        || pkg.requires_admin.is_some()
}

fn version_quad(raw: Option<&str>) -> [u64; 4] {
    let mut quad = [0u64; 4];
    if let Some(raw) = raw {
        for (i, part) in raw.split('.').take(4).enumerate() {
            quad[i] = part.parse::<u64>().unwrap_or(0).min(65535);
        }
    } else {
        quad[0] = 1;
    }
    quad
}

fn rc_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\"\"")
}

fn app_manifest(pkg: &PackageSection) -> String {
    let level = if pkg.requires_admin == Some(true) {
        "requireAdministrator"
    } else {
        "asInvoker"
    };
    let dpi = if pkg.dpi_aware == Some(false) {
        "false"
    } else {
        "true/pm"
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<assembly xmlns=\"urn:schemas-microsoft-com:asm.v1\" manifestVersion=\"1.0\">\n\
  <trustInfo xmlns=\"urn:schemas-microsoft-com:asm.v3\">\n\
    <security>\n\
      <requestedPrivileges>\n\
        <requestedExecutionLevel level=\"{level}\" uiAccess=\"false\"/>\n\
      </requestedPrivileges>\n\
    </security>\n\
  </trustInfo>\n\
  <application xmlns=\"urn:schemas-microsoft-com:asm.v3\">\n\
    <windowsSettings>\n\
      <dpiAware xmlns=\"http://schemas.microsoft.com/SMI/2005/WindowsSettings\">{dpi}</dpiAware>\n\
    </windowsSettings>\n\
  </application>\n\
</assembly>\n"
    )
}

fn render_rc(name: &str, pkg: &PackageSection, icon: Option<&str>, manifest: bool) -> String {
    let quad = version_quad(pkg.version.as_deref());
    let version_str = format!("{}.{}.{}.{}", quad[0], quad[1], quad[2], quad[3]);
    let description = rc_escape(pkg.description.as_deref().unwrap_or(name));
    let company = rc_escape(pkg.publisher.as_deref().unwrap_or(""));
    let product = rc_escape(name);
    let mut rc = String::new();
    if let Some(icon) = icon {
        let _ = writeln!(rc, "1 ICON \"{icon}\"");
    }
    if manifest {
        let _ = writeln!(rc, "1 24 \"app.manifest\"");
    }
    let _ = write!(
        rc,
        "1 VERSIONINFO\n\
FILEVERSION {fv}\n\
PRODUCTVERSION {fv}\n\
BEGIN\n\
  BLOCK \"StringFileInfo\"\n\
  BEGIN\n\
    BLOCK \"040904b0\"\n\
    BEGIN\n\
      VALUE \"CompanyName\", \"{company}\"\n\
      VALUE \"FileDescription\", \"{description}\"\n\
      VALUE \"FileVersion\", \"{version_str}\"\n\
      VALUE \"ProductName\", \"{product}\"\n\
      VALUE \"ProductVersion\", \"{version_str}\"\n\
    END\n\
  END\n\
  BLOCK \"VarFileInfo\"\n\
  BEGIN\n\
    VALUE \"Translation\", 0x409, 1200\n\
  END\n\
END\n",
        fv = format!("{},{},{},{}", quad[0], quad[1], quad[2], quad[3]),
    );
    rc
}

fn bfd_target(arch: TargetArch) -> Option<&'static str> {
    match arch {
        TargetArch::X86_64 => Some("pe-x86-64"),
        TargetArch::X86 => Some("pe-i386"),
        TargetArch::Aarch64 | TargetArch::Armv7 | TargetArch::Arm64Ec => None,
    }
}

pub(crate) fn embed_object(
    project: &Project,
    env: &Environment,
    tc: &ResolvedToolchain,
) -> Result<Option<PathBuf>> {
    let pkg = &project.manifest.package;
    if !wants_resources(pkg) {
        return Ok(None);
    }
    let arch = env.manifest.target_arch;
    let triple = arch.mingw_triple();
    let Some(windres) = lsw_toolchain::find_windres(&tc.cc, triple) else {
        tracing::warn!(
            "[package] resources requested but no windres found ({triple}-windres or llvm-windres); skipping icon/version embedding"
        );
        return Ok(None);
    };

    let dir = project.root.join("build").join("lsw-res");
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(dir.clone(), e))?;

    let icon = match &pkg.icon {
        Some(icon)
            if Path::new(icon)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("ico")) =>
        {
            let src = project.root.join(icon);
            let dst = dir.join("app.ico");
            std::fs::copy(&src, &dst).map_err(|e| Error::io(src, e))?;
            Some("app.ico")
        }
        Some(icon) => {
            tracing::warn!(
                "[package] icon = \"{icon}\" is not an .ico; PE icon embedding needs .ico (MSIX logos take .png)"
            );
            None
        }
        None => None,
    };

    let manifest = pkg.dpi_aware.is_some() || pkg.requires_admin.is_some();
    if manifest {
        let path = dir.join("app.manifest");
        std::fs::write(&path, app_manifest(pkg)).map_err(|e| Error::io(path, e))?;
    }

    let rc_path = dir.join("resources.rc");
    let rc = render_rc(&project.manifest.project.name, pkg, icon, manifest);
    std::fs::write(&rc_path, rc).map_err(|e| Error::io(rc_path.clone(), e))?;

    let prefixed = windres
        .file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with(triple));
    let mut command = Command::new(&windres);
    if !prefixed {
        match bfd_target(arch) {
            Some(target) => {
                command.arg("-F").arg(target);
            }
            None => {
                tracing::warn!(
                    "[package] resources need {triple}-windres for {arch}; unprefixed windres cannot target it - skipping embedding"
                );
                return Ok(None);
            }
        }
    }
    let obj = dir.join("resources.o");
    let output = command
        .arg("-O")
        .arg("coff")
        .arg("resources.rc")
        .arg("-o")
        .arg("resources.o")
        .current_dir(&dir)
        .output()
        .map_err(|e| Error::io(windres.clone(), e))?;
    if !output.status.success() {
        return Err(Error::BuildFailed {
            command: format!(
                "{} resources.rc: {}",
                windres.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            code: output.status.code(),
        });
    }
    std::path::absolute(&obj)
        .map(Some)
        .map_err(|e| Error::io(obj, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_quad_parses_and_pads() {
        assert_eq!(version_quad(Some("2.1")), [2, 1, 0, 0]);
        assert_eq!(version_quad(Some("1.2.3.4")), [1, 2, 3, 4]);
        assert_eq!(version_quad(Some("999999.0")), [65535, 0, 0, 0]);
        assert_eq!(version_quad(None), [1, 0, 0, 0]);
    }

    #[test]
    fn rc_contains_version_and_strings() {
        let pkg = PackageSection {
            version: Some("2.5.1".into()),
            publisher: Some("Acme".into()),
            ..Default::default()
        };
        let rc = render_rc("hello", &pkg, Some("app.ico"), true);
        assert!(rc.contains("FILEVERSION 2,5,1,0"));
        assert!(rc.contains("VALUE \"CompanyName\", \"Acme\""));
        assert!(rc.contains("1 ICON \"app.ico\""));
        assert!(rc.contains("1 24 \"app.manifest\""));
    }

    #[test]
    fn manifest_reflects_admin_and_dpi() {
        let pkg = PackageSection {
            requires_admin: Some(true),
            dpi_aware: Some(false),
            ..Default::default()
        };
        let xml = app_manifest(&pkg);
        assert!(xml.contains("requireAdministrator"));
        assert!(xml.contains(">false</dpiAware>"));
    }
}
