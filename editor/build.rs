fn main() {
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let cargo_profile = match profile.as_str() {
        "debug" => "dev",
        profile => profile,
    };

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-env=CALYX_EDITOR_PROFILE={cargo_profile}");
}
