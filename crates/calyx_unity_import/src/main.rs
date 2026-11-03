//! `calyx_unity_import`: imports a Unity Asset Store pack into a Calyx
//! project. Run it from the repository root:
//!
//! ```text
//! cargo run --release -p calyx_unity_import -- [--package <.unitypackage>]
//!     [--blender <blender.exe>] [--scene] [--out <dir>]
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use calyx_unity_import::package::UnityPackage;
use calyx_unity_import::packs::{self, Pack};
use calyx_unity_import::{build_scene, import_assets, Layout};
use clap::Parser;

const DEFAULT_BLENDER: &str = r"C:\Program Files\Blender Foundation\Blender 4.0\blender.exe";

/// Imports a Unity Asset Store pack into the demos project: its models
/// (through headless Blender), textures and materials into a gitignored
/// vendor folder, and with `--scene` its demo scene into a Calyx scene.
#[derive(Parser)]
#[command(name = "calyx_unity_import")]
struct Cli {
    /// The pack to import.
    #[arg(long, default_value = "rpg-poly-pack-lite")]
    pack: String,
    /// The `.unitypackage`; defaults to where the Unity editor's Package
    /// Manager saves the pack.
    #[arg(long)]
    package: Option<PathBuf>,
    /// Blender 4.x, which converts the FBX models.
    #[arg(long, default_value = DEFAULT_BLENDER)]
    blender: PathBuf,
    /// Also regenerate the pack's Calyx scene from its demo scene.
    #[arg(long)]
    scene: bool,
    /// Only regenerate the scene; leave the converted assets as they are.
    #[arg(long, hide = true)]
    skip_assets: bool,
    /// The output folder; defaults to the pack's folder in the demos project.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Where to put the import's scratch folder, outside every asset root;
    /// defaults to the pack's project folder.
    #[arg(long)]
    work: Option<PathBuf>,
}

fn default_package(pack: &Pack) -> PathBuf {
    let mut path = PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default());
    path.push("Unity");
    path.push("Asset Store-5.x");
    for part in pack.asset_store_path {
        path.push(part);
    }
    path
}

fn run(cli: Cli) -> Result<(), String> {
    let pack = packs::find(&cli.pack).ok_or_else(|| {
        let known: Vec<_> = packs::PACKS.iter().map(|p| p.id).collect();
        format!(
            "unknown pack {}; known packs: {}",
            cli.pack,
            known.join(", ")
        )
    })?;
    let package_path = cli.package.unwrap_or_else(|| default_package(pack));
    if !package_path.is_file() {
        return Err(format!(
            "{} not found at {}; download it through the Unity editor's Package Manager \
             (My Assets) or pass --package",
            pack.title,
            package_path.display()
        ));
    }
    let root = cli.out.unwrap_or_else(|| PathBuf::from(pack.root));
    if !root.is_dir() {
        return Err(format!(
            "{} does not exist; run this from the repository root or pass --out",
            root.display()
        ));
    }
    let package = UnityPackage::open(&package_path)?;
    let work_root = cli.work.unwrap_or_else(|| PathBuf::from(pack.work_root));
    let layout = Layout {
        pack,
        root,
        work_root,
    };
    if !cli.skip_assets {
        if !cli.blender.is_file() {
            return Err(format!(
                "Blender not found at {}; pass --blender",
                cli.blender.display()
            ));
        }
        println!("{}", import_assets(&package, &layout, &cli.blender)?);
    }
    if cli.scene {
        println!("{}", build_scene(&package, &layout)?);
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}
