use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;

use crate::buildops::{self, BuildOptions, which};
use crate::envops::Environment;
use crate::error::{Error, Result};
use crate::project::Project;

const MAX_ARTIFACTS: usize = 4096;

fn strip_existing(path: &std::path::Path) -> Result<()> {
    if fs::symlink_metadata(path).is_ok() {
        fs::remove_file(path).map_err(|e| Error::io(path.to_path_buf(), e))?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageTarget {
    PortableDirectory,
    Zip,
    Msi,
    Msix,
    Nsis,
    Winget,
}

#[derive(Debug, Serialize)]
pub struct PackageReport {
    pub directory: PathBuf,
    pub zip: Option<PathBuf>,
    pub msi: Option<PathBuf>,
    pub msix: Option<PathBuf>,
    pub nsis: Option<PathBuf>,
    pub winget: Option<PathBuf>,
    pub files: Vec<String>,
    pub bundled: Vec<String>,
    pub assumed_system: Vec<String>,
    pub missing: Vec<String>,
}

pub fn package(
    project: &Project,
    env: &Environment,
    target: PackageTarget,
    bundle_deps: bool,
) -> Result<PackageReport> {
    let build = buildops::build(project, env, &BuildOptions::default())?;
    if build.artifacts.is_empty() {
        return Err(Error::NoBuildSystem);
    }

    let stem = format!(
        "{}-{}",
        project.manifest.project.name, env.manifest.target_arch
    );
    if build.artifacts.len() > MAX_ARTIFACTS {
        return Err(Error::InitFailed {
            path: project.root.join("dist"),
            detail: format!("build produced more than {MAX_ARTIFACTS} artifacts to package"),
        });
    }
    let dist = project.root.join("dist");
    let dir = dist.join(&stem);
    if dir.parent() != Some(dist.as_path()) {
        return Err(Error::InitFailed {
            path: dir,
            detail: "package output directory escaped dist/".into(),
        });
    }
    if fs::symlink_metadata(&dist).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(Error::InitFailed {
            path: dist,
            detail: "dist/ is a symlink; refusing to package through it".into(),
        });
    }
    if let Ok(meta) = fs::symlink_metadata(&dir) {
        if meta.file_type().is_symlink() {
            fs::remove_file(&dir).map_err(|e| Error::io(dir.clone(), e))?;
        } else {
            fs::remove_dir_all(&dir).map_err(|e| Error::io(dir.clone(), e))?;
        }
    }
    fs::create_dir_all(&dir).map_err(|e| Error::io(dir.clone(), e))?;

    let mut seen: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();
    for artifact in &build.artifacts {
        let name = artifact
            .file_name()
            .expect("artifacts always have file names")
            .to_string_lossy()
            .into_owned();
        if let Some(previous) = seen.insert(name.to_ascii_lowercase(), artifact.clone()) {
            return Err(Error::PackageNameCollision {
                name,
                first: previous,
                second: artifact.clone(),
            });
        }
    }

    let canon_root = project.root.canonicalize().ok();
    let mut files = Vec::new();
    for artifact in &build.artifacts {
        let source = project.root.join(artifact);
        let name = source
            .file_name()
            .expect("artifacts always have file names")
            .to_owned();
        let within = source
            .canonicalize()
            .ok()
            .zip(canon_root.clone())
            .is_some_and(|(s, root)| s.starts_with(&root));
        if !within {
            return Err(Error::InitFailed {
                path: source,
                detail: "refusing to package an artifact that resolves outside the project".into(),
            });
        }
        let dest = dir.join(&name);
        fs::copy(&source, &dest).map_err(|e| Error::io(source.clone(), e))?;
        files.push(name.to_string_lossy().into_owned());
    }

    let (bundled, assumed_system, missing) = if bundle_deps {
        bundle_dependencies(project, env, &dir, &mut files)?
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };

    let mut zip = None;
    let mut msi = None;
    let mut msix = None;
    let mut nsis = None;
    let mut winget = None;
    match target {
        PackageTarget::PortableDirectory => {}
        PackageTarget::Zip => {
            if which("zip").is_none() {
                return Err(Error::ToolMissing {
                    tool: "zip".into(),
                    fix: "install zip, or use --target portable-directory".into(),
                });
            }
            let zip_path = dist.join(format!("{stem}.zip"));
            strip_existing(&zip_path)?;
            let status = Command::new("zip")
                .args(["-r", "-q"])
                .arg(&zip_path)
                .arg(format!("./{stem}"))
                .current_dir(&dist)
                .status()
                .map_err(|e| Error::io(zip_path.clone(), e))?;
            if !status.success() {
                return Err(Error::BuildFailed {
                    command: format!("zip -r {} {stem}", zip_path.display()),
                    code: status.code(),
                });
            }
            zip = Some(zip_path);
        }
        PackageTarget::Msi => {
            msi = Some(build_msi(project, env, &dist, &dir, &stem, &files)?);
        }
        PackageTarget::Msix => {
            msix = Some(crate::msixops::build_msix(
                project,
                env.manifest.target_arch,
                &dist,
                &dir,
                &stem,
                &files,
            )?);
        }
        PackageTarget::Nsis => {
            nsis = Some(build_nsis(project, &dist, &dir, &stem, &files)?);
        }
        PackageTarget::Winget => {
            let installer = build_msi(project, env, &dist, &dir, &stem, &files)?;
            msi = Some(installer.clone());
            winget = Some(build_winget(project, env, &dist, &installer)?);
        }
    }

    Ok(PackageReport {
        directory: dir,
        zip,
        msi,
        msix,
        nsis,
        winget,
        files,
        bundled,
        assumed_system,
        missing,
    })
}

fn bundle_dependencies(
    project: &Project,
    env: &Environment,
    dir: &std::path::Path,
    files: &mut Vec<String>,
) -> Result<(Vec<String>, Vec<String>, Vec<String>)> {
    let mut search = vec![dir.to_path_buf()];
    if let Some((_, _, bin)) = crate::depsops::dep_dirs(project, env.manifest.target_arch)
        && bin.is_dir()
    {
        search.push(bin);
    }
    let sysroot = &env.manifest.toolchain.sysroot;
    search.push(sysroot.join("bin"));
    search.extend(crate::depsops::vc_runtime_dirs(sysroot));

    let mut present: std::collections::BTreeSet<String> =
        files.iter().map(|f| f.to_ascii_lowercase()).collect();
    let mut bundled = std::collections::BTreeSet::new();
    let mut assumed_system = std::collections::BTreeSet::new();
    let mut missing = std::collections::BTreeSet::new();

    let roots: Vec<String> = files.clone();
    for name in &roots {
        let is_pe = std::path::Path::new(name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("dll"));
        if !is_pe {
            continue;
        }
        let node = crate::depsops::tree_with_dirs(&search, &dir.join(name))?;
        collect_bundle(
            &node,
            dir,
            &mut present,
            files,
            &mut bundled,
            &mut assumed_system,
            &mut missing,
        )?;
    }

    Ok((
        bundled.into_iter().collect(),
        assumed_system.into_iter().collect(),
        missing.into_iter().collect(),
    ))
}

fn collect_bundle(
    node: &crate::depsops::DepNode,
    dir: &std::path::Path,
    present: &mut std::collections::BTreeSet<String>,
    files: &mut Vec<String>,
    bundled: &mut std::collections::BTreeSet<String>,
    assumed_system: &mut std::collections::BTreeSet<String>,
    missing: &mut std::collections::BTreeSet<String>,
) -> Result<()> {
    use crate::depsops::DepKind;
    match node.kind {
        DepKind::Root | DepKind::Seen => {}
        DepKind::System => {
            assumed_system.insert(node.name.to_ascii_lowercase());
        }
        DepKind::Missing => {
            missing.insert(node.name.to_ascii_lowercase());
        }
        DepKind::Resolved => {
            let lower = node.name.to_ascii_lowercase();
            if !present.contains(&lower) {
                let source = PathBuf::from(node.path.as_deref().unwrap_or_default());
                if crate::runops::is_real_windows_binary(&source) {
                    let dest = dir.join(&lower);
                    fs::copy(&source, &dest).map_err(|e| Error::io(source.clone(), e))?;
                    present.insert(lower.clone());
                    files.push(lower.clone());
                    bundled.insert(lower);
                } else {
                    assumed_system.insert(lower);
                }
            }
        }
    }
    for child in &node.children {
        collect_bundle(child, dir, present, files, bundled, assumed_system, missing)?;
    }
    Ok(())
}

fn build_msi(
    project: &Project,
    env: &Environment,
    dist: &std::path::Path,
    dir: &std::path::Path,
    stem: &str,
    files: &[String],
) -> Result<PathBuf> {
    if which("wixl").is_none() {
        return Err(Error::ToolMissing {
            tool: "wixl".into(),
            fix: "install msitools (provides wixl), or use --target zip".into(),
        });
    }

    let name = &project.manifest.project.name;
    let wxs = render_wxs(name, &project.manifest.package, files);
    let wxs_path = dist.join(format!("{stem}.wxs"));
    if fs::symlink_metadata(&wxs_path).is_ok_and(|m| m.file_type().is_symlink()) {
        fs::remove_file(&wxs_path).map_err(|e| Error::io(wxs_path.clone(), e))?;
    }
    fs::write(&wxs_path, wxs).map_err(|e| Error::io(wxs_path.clone(), e))?;

    let msi_path = dist.join(format!("{stem}.msi"));
    strip_existing(&msi_path)?;

    let arch = env.manifest.target_arch.win_arch_name();

    let abs_wxs = std::path::absolute(&wxs_path).map_err(|e| Error::io(wxs_path.clone(), e))?;
    let abs_msi = std::path::absolute(&msi_path).map_err(|e| Error::io(msi_path.clone(), e))?;
    let output = Command::new("wixl")
        .arg("-a")
        .arg(arch)
        .arg("-o")
        .arg(&abs_msi)
        .arg(&abs_wxs)
        .current_dir(dir)
        .output()
        .map_err(|e| Error::io(PathBuf::from("wixl"), e))?;
    if !output.status.success() {
        return Err(Error::BuildFailed {
            command: format!(
                "wixl -a {arch} -o {} {}: {}",
                msi_path.display(),
                wxs_path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            code: output.status.code(),
        });
    }
    Ok(msi_path)
}

fn render_wxs(name: &str, package: &lsw_config::PackageSection, files: &[String]) -> String {
    let upgrade_code = package
        .upgrade_code
        .clone()
        .unwrap_or_else(|| deterministic_guid(&format!("lsw:{name}:upgrade")));
    let version = crate::xml_escape(package.version.as_deref().unwrap_or("1.0.0"));
    let manufacturer = crate::xml_escape(package.publisher.as_deref().unwrap_or("LSW"));
    let ename = crate::xml_escape(name);

    let mut components = String::new();
    let mut refs = String::new();
    for (i, file) in files.iter().enumerate() {
        let comp_id = format!("cmp{i}");
        let file_id = format!("file{i}");
        let guid = deterministic_guid(&format!("lsw:{name}:{file}"));
        let efile = crate::xml_escape(file);
        let _ = write!(
            components,
            "          <Component Id=\"{comp_id}\" Guid=\"{guid}\">\n\
             \x20           <File Id=\"{file_id}\" Source=\"{efile}\" KeyPath=\"yes\"/>\n\
             \x20         </Component>\n"
        );
        let _ = writeln!(refs, "        <ComponentRef Id=\"{comp_id}\"/>");
    }

    let mut menu_dir = String::new();
    let mut menu_component = String::new();
    if package.shortcuts == Some(true) {
        let exes: Vec<&String> = files
            .iter()
            .filter(|f| {
                std::path::Path::new(f)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
            })
            .collect();
        if !exes.is_empty() {
            menu_dir = format!(
                "\x20   <Directory Id=\"ProgramMenuFolder\">\n\
                 \x20     <Directory Id=\"AppMenuFolder\" Name=\"{ename}\"/>\n\
                 \x20   </Directory>\n"
            );
            let guid = deterministic_guid(&format!("lsw:{name}:shortcuts"));
            let mut shortcuts = String::new();
            for (i, exe) in exes.iter().enumerate() {
                let stem = std::path::Path::new(exe.as_str())
                    .file_stem()
                    .map_or_else(|| (*exe).clone(), |s| s.to_string_lossy().into_owned());
                let _ = writeln!(
                    shortcuts,
                    "\x20       <Shortcut Id=\"menu{i}\" Name=\"{}\" Target=\"[INSTALLDIR]{}\" WorkingDirectory=\"INSTALLDIR\"/>",
                    crate::xml_escape(&stem),
                    crate::xml_escape(exe)
                );
            }
            menu_component = format!(
                "\x20   <DirectoryRef Id=\"AppMenuFolder\">\n\
                 \x20     <Component Id=\"menuShortcuts\" Guid=\"{guid}\">\n\
                 {shortcuts}\
                 \x20       <RemoveFolder Id=\"AppMenuFolder\" On=\"uninstall\"/>\n\
                 \x20       <RegistryValue Root=\"HKCU\" Key=\"Software\\{ename}\\Install\" Name=\"shortcuts\" Type=\"integer\" Value=\"1\" KeyPath=\"yes\"/>\n\
                 \x20     </Component>\n\
                 \x20   </DirectoryRef>\n"
            );
            let _ = writeln!(refs, "        <ComponentRef Id=\"menuShortcuts\"/>");
        }
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <Wix xmlns=\"http://schemas.microsoft.com/wix/2006/wi\">\n\
         \x20 <Product Id=\"*\" Name=\"{ename}\" Language=\"1033\" Version=\"{version}\"\n\
         \x20          Manufacturer=\"{manufacturer}\" UpgradeCode=\"{upgrade_code}\">\n\
         \x20   <Package InstallerVersion=\"200\" Compressed=\"yes\" InstallScope=\"perMachine\"/>\n\
         \x20   <Media Id=\"1\" Cabinet=\"main.cab\" EmbedCab=\"yes\"/>\n\
         \x20   <Directory Id=\"TARGETDIR\" Name=\"SourceDir\">\n\
         \x20     <Directory Id=\"ProgramFilesFolder\">\n\
         \x20       <Directory Id=\"INSTALLDIR\" Name=\"{ename}\">\n\
         {components}\
         \x20       </Directory>\n\
         \x20     </Directory>\n\
         {menu_dir}\
         \x20   </Directory>\n\
         {menu_component}\
         \x20   <Feature Id=\"Main\" Title=\"{ename}\" Level=\"1\">\n\
         {refs}\
         \x20   </Feature>\n\
         \x20 </Product>\n\
         </Wix>\n"
    )
}

fn build_nsis(
    project: &Project,
    dist: &std::path::Path,
    dir: &std::path::Path,
    stem: &str,
    files: &[String],
) -> Result<PathBuf> {
    if which("makensis").is_none() {
        return Err(Error::ToolMissing {
            tool: "makensis".into(),
            fix: "install nsis (provides makensis), or use --target msi".into(),
        });
    }
    let name = &project.manifest.project.name;
    let pkg = &project.manifest.package;
    let version = pkg.version.as_deref().unwrap_or("1.0.0");
    let publisher = pkg.publisher.as_deref().unwrap_or("LSW");
    let out_name = format!("{stem}-setup.exe");

    let mut install_files = String::new();
    let mut delete_files = String::new();
    for f in files {
        let win = f.replace('/', "\\");
        let _ = writeln!(install_files, "  File \"/oname={win}\" \"{f}\"");
        let _ = writeln!(delete_files, "  Delete \"$INSTDIR\\{win}\"");
    }
    let mut shortcuts = String::new();
    let mut delete_shortcuts = String::new();
    if pkg.shortcuts == Some(true) {
        let _ = writeln!(shortcuts, "  CreateDirectory \"$SMPROGRAMS\\{name}\"");
        let _ = writeln!(delete_shortcuts, "  RMDir /r \"$SMPROGRAMS\\{name}\"");
        for f in files {
            let p = std::path::Path::new(f);
            if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) {
                let title = p
                    .file_stem()
                    .map_or_else(|| f.clone(), |s| s.to_string_lossy().into_owned());
                let win = f.replace('/', "\\");
                let _ = writeln!(
                    shortcuts,
                    "  CreateShortcut \"$SMPROGRAMS\\{name}\\{title}.lnk\" \"$INSTDIR\\{win}\""
                );
            }
        }
    }

    let nsi = format!(
        "Unicode true\n\
         Name \"{name}\"\n\
         OutFile \"{out}\"\n\
         InstallDir \"$PROGRAMFILES64\\{name}\"\n\
         RequestExecutionLevel admin\n\
         VIProductVersion \"{vi_version}\"\n\
         VIAddVersionKey ProductName \"{name}\"\n\
         VIAddVersionKey CompanyName \"{publisher}\"\n\
         VIAddVersionKey FileVersion \"{version}\"\n\
         VIAddVersionKey ProductVersion \"{version}\"\n\
         \n\
         Section \"Install\"\n\
         \x20 SetOutPath \"$INSTDIR\"\n\
         {install_files}\
         {shortcuts}\
         \x20 WriteUninstaller \"$INSTDIR\\uninstall.exe\"\n\
         \x20 WriteRegStr HKLM \"Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{name}\" DisplayName \"{name}\"\n\
         \x20 WriteRegStr HKLM \"Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{name}\" DisplayVersion \"{version}\"\n\
         \x20 WriteRegStr HKLM \"Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{name}\" Publisher \"{publisher}\"\n\
         \x20 WriteRegStr HKLM \"Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{name}\" UninstallString \"$INSTDIR\\uninstall.exe\"\n\
         SectionEnd\n\
         \n\
         Section \"Uninstall\"\n\
         {delete_files}\
         {delete_shortcuts}\
         \x20 Delete \"$INSTDIR\\uninstall.exe\"\n\
         \x20 RMDir \"$INSTDIR\"\n\
         \x20 DeleteRegKey HKLM \"Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{name}\"\n\
         SectionEnd\n",
        out = dist_relative_out(dist, dir, &out_name),
        vi_version = nsis_vi_version(pkg.version.as_deref()),
    );
    let nsi_path = dist.join(format!("{stem}.nsi"));
    strip_existing(&nsi_path)?;
    fs::write(&nsi_path, nsi).map_err(|e| Error::io(nsi_path.clone(), e))?;

    let out_path = dist.join(&out_name);
    strip_existing(&out_path)?;
    let abs_nsi = std::path::absolute(&nsi_path).map_err(|e| Error::io(nsi_path.clone(), e))?;
    let output = Command::new("makensis")
        .arg(&abs_nsi)
        .current_dir(dir)
        .output()
        .map_err(|e| Error::io(PathBuf::from("makensis"), e))?;
    if !output.status.success() {
        return Err(Error::BuildFailed {
            command: format!(
                "makensis {}: {}",
                nsi_path.display(),
                String::from_utf8_lossy(&output.stdout).trim()
            ),
            code: output.status.code(),
        });
    }
    Ok(out_path)
}

fn dist_relative_out(dist: &std::path::Path, dir: &std::path::Path, out_name: &str) -> String {
    let out = dist.join(out_name);
    pathdiff_display(&out, dir)
}

fn pathdiff_display(target: &std::path::Path, base: &std::path::Path) -> String {
    match (std::path::absolute(target), std::path::absolute(base)) {
        (Ok(t), Ok(b)) => match t.strip_prefix(&b) {
            Ok(rel) => rel.display().to_string(),
            Err(_) => t.display().to_string(),
        },
        _ => target.display().to_string(),
    }
}

fn nsis_vi_version(raw: Option<&str>) -> String {
    let mut parts: Vec<u64> = raw
        .unwrap_or("1.0.0")
        .split('.')
        .map(|p| p.parse().unwrap_or(0))
        .take(4)
        .collect();
    while parts.len() < 4 {
        parts.push(0);
    }
    parts
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

fn build_winget(
    project: &Project,
    env: &Environment,
    dist: &std::path::Path,
    installer: &std::path::Path,
) -> Result<PathBuf> {
    let pkg = &project.manifest.package;
    let name = &project.manifest.project.name;
    let Some(url_base) = pkg.installer_url.as_deref() else {
        return Err(Error::InitFailed {
            path: dist.to_path_buf(),
            detail: "winget manifests need [package] installer_url (the URL where you will host the installer)".into(),
        });
    };
    let publisher = pkg.publisher.as_deref().unwrap_or("LSW");
    let version = pkg.version.as_deref().unwrap_or("1.0.0");
    let sha256 = crate::sha256_file_checked(installer)?.to_uppercase();
    let installer_name = installer
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let url = if url_base.ends_with('/') {
        format!("{url_base}{installer_name}")
    } else {
        format!("{url_base}/{installer_name}")
    };
    let ident_publisher: String = publisher
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    let ident_name: String = name.chars().filter(char::is_ascii_alphanumeric).collect();
    let identifier = format!("{ident_publisher}.{ident_name}");
    let arch = match env.manifest.target_arch {
        lsw_config::TargetArch::X86_64 => "x64",
        lsw_config::TargetArch::X86 => "x86",
        lsw_config::TargetArch::Aarch64 | lsw_config::TargetArch::Arm64Ec => "arm64",
        lsw_config::TargetArch::Armv7 => "arm",
    };

    let out_dir = dist.join("winget");
    fs::create_dir_all(&out_dir).map_err(|e| Error::io(out_dir.clone(), e))?;
    let write = |file: &str, contents: String| -> Result<()> {
        let path = out_dir.join(file);
        fs::write(&path, contents).map_err(|e| Error::io(path, e))
    };
    write(
        &format!("{identifier}.yaml"),
        format!(
            "PackageIdentifier: {identifier}\nPackageVersion: {version}\nDefaultLocale: en-US\nManifestType: version\nManifestVersion: 1.6.0\n"
        ),
    )?;
    write(
        &format!("{identifier}.locale.en-US.yaml"),
        format!(
            "PackageIdentifier: {identifier}\nPackageVersion: {version}\nPackageLocale: en-US\nPublisher: {publisher}\nPackageName: {name}\nLicense: proprietary\nShortDescription: {desc}\nManifestType: defaultLocale\nManifestVersion: 1.6.0\n",
            desc = pkg.description.as_deref().unwrap_or(name),
        ),
    )?;
    write(
        &format!("{identifier}.installer.yaml"),
        format!(
            "PackageIdentifier: {identifier}\nPackageVersion: {version}\nInstallers:\n- Architecture: {arch}\n  InstallerType: wix\n  InstallerUrl: {url}\n  InstallerSha256: {sha256}\nManifestType: installer\nManifestVersion: 1.6.0\n"
        ),
    )?;
    Ok(out_dir)
}

fn deterministic_guid(seed: &str) -> String {
    let hex = lsw_toolchain::sha256_bytes(seed.as_bytes());
    let b = hex.as_bytes();
    let s = |start: usize, len: usize| -> String {
        std::str::from_utf8(&b[start..start + len])
            .unwrap()
            .to_ascii_uppercase()
    };
    format!(
        "{}-{}-{}-{}-{}",
        s(0, 8),
        s(8, 4),
        s(12, 4),
        s(16, 4),
        s(20, 12)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::depsops::{DepKind, DepNode};

    fn node(name: &str, kind: DepKind, path: Option<PathBuf>, children: Vec<DepNode>) -> DepNode {
        DepNode {
            name: name.to_owned(),
            kind,
            path: path.map(|p| p.display().to_string()),
            children,
        }
    }

    #[test]
    fn collect_bundle_copies_resolved_skips_system_and_records_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = tmp.path().join("pkg");
        std::fs::create_dir_all(&pkg).unwrap();
        let src = tmp.path().join("libfoo-1.dll");
        let mut image = vec![0u8; 128];
        image[..2].copy_from_slice(b"MZ");
        std::fs::write(&src, &image).unwrap();

        let tree = node(
            "app.exe",
            DepKind::Root,
            None,
            vec![
                node("KERNEL32.dll", DepKind::System, None, vec![]),
                node("libFoo-1.dll", DepKind::Resolved, Some(src), vec![]),
                node("gone.dll", DepKind::Missing, None, vec![]),
            ],
        );

        let mut present = std::collections::BTreeSet::from(["app.exe".to_owned()]);
        let mut files = vec!["app.exe".to_owned()];
        let mut bundled = std::collections::BTreeSet::new();
        let mut assumed = std::collections::BTreeSet::new();
        let mut missing = std::collections::BTreeSet::new();
        collect_bundle(
            &tree,
            &pkg,
            &mut present,
            &mut files,
            &mut bundled,
            &mut assumed,
            &mut missing,
        )
        .unwrap();

        assert!(pkg.join("libfoo-1.dll").is_file());
        assert!(files.contains(&"libfoo-1.dll".to_owned()));
        assert_eq!(
            bundled.into_iter().collect::<Vec<_>>(),
            vec!["libfoo-1.dll"]
        );
        assert_eq!(
            assumed.into_iter().collect::<Vec<_>>(),
            vec!["kernel32.dll"]
        );
        assert_eq!(missing.into_iter().collect::<Vec<_>>(), vec!["gone.dll"]);
    }

    #[test]
    fn collect_bundle_treats_wine_builtins_as_system() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = tmp.path().join("pkg");
        std::fs::create_dir_all(&pkg).unwrap();
        let src = tmp.path().join("builtin.dll");
        let mut image = vec![0u8; 128];
        image[..2].copy_from_slice(b"MZ");
        image[64..80].copy_from_slice(b"Wine builtin DLL");
        std::fs::write(&src, &image).unwrap();

        let tree = node(
            "app.exe",
            DepKind::Root,
            None,
            vec![node("builtin.dll", DepKind::Resolved, Some(src), vec![])],
        );
        let mut present = std::collections::BTreeSet::new();
        let mut files = Vec::new();
        let mut bundled = std::collections::BTreeSet::new();
        let mut assumed = std::collections::BTreeSet::new();
        let mut missing = std::collections::BTreeSet::new();
        collect_bundle(
            &tree,
            &pkg,
            &mut present,
            &mut files,
            &mut bundled,
            &mut assumed,
            &mut missing,
        )
        .unwrap();

        assert!(!pkg.join("builtin.dll").exists());
        assert!(bundled.is_empty());
        assert_eq!(assumed.into_iter().collect::<Vec<_>>(), vec!["builtin.dll"]);
    }
}
