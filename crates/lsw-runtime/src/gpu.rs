use std::path::Path;

const NVIDIA_VENDOR: &str = "0x10de";
const NVIDIA_EGL_JSON: &str = "/usr/share/glvnd/egl_vendor.d/10_nvidia.json";
const EGL_VENDOR_VAR: &str = "__EGL_VENDOR_LIBRARY_FILENAMES";

pub(crate) fn render_node_vendors(drm_root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(drm_root) else {
        return Vec::new();
    };
    let mut vendors = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with("renderD") {
            continue;
        }
        if let Ok(vendor) = std::fs::read_to_string(entry.path().join("device/vendor")) {
            vendors.push(vendor.trim().to_owned());
        }
    }
    vendors
}

pub(crate) fn egl_vendor_pin_for(
    host_override: bool,
    vendors: &[String],
    nvidia_json: &Path,
) -> Option<(String, String)> {
    if host_override || vendors.is_empty() {
        return None;
    }
    (vendors.iter().all(|v| v == NVIDIA_VENDOR) && nvidia_json.is_file())
        .then(|| (EGL_VENDOR_VAR.to_owned(), nvidia_json.display().to_string()))
}

pub(crate) fn egl_vendor_pin() -> Option<(String, String)> {
    egl_vendor_pin_for(
        std::env::var_os(EGL_VENDOR_VAR).is_some(),
        &render_node_vendors(Path::new("/sys/class/drm")),
        Path::new(NVIDIA_EGL_JSON),
    )
}
