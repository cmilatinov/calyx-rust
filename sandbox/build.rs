fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    embed_windows_icon();
}

/// Embeds the Calyx icon, `resources/app_icon/calyx.ico`, in the `game` player executable
/// as its Windows icon resource, so Explorer, shortcuts and the taskbar show it
/// before the window sets its own. Only Windows targets carry the resource.
fn embed_windows_icon() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = manifest_dir.join("../resources/app_icon/calyx.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    // The resource script names the icon by absolute path, so the resource
    // compiler finds it whatever directory it runs in.
    let script = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("calyx_icon.rc");
    let icon_path = icon.display().to_string().replace('\\', "/");
    std::fs::write(&script, format!("1 ICON \"{icon_path}\"\n")).unwrap();
    match embed_resource::compile(&script, embed_resource::NONE) {
        embed_resource::CompilationResult::Ok | embed_resource::CompilationResult::NotWindows => {}
        embed_resource::CompilationResult::NotAttempted(reason) => {
            println!("cargo:warning=The executable has no icon: no resource compiler ({reason})");
        }
        embed_resource::CompilationResult::Failed(reason) => {
            panic!("Failed to compile the icon resource: {reason}");
        }
    }
}
