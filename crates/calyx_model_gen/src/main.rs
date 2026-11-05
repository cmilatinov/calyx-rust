//! `calyx_model_gen`: generates the sandbox's low-poly clay models. Run it
//! from the repository root:
//!
//! ```text
//! cargo run --release -p calyx_model_gen -- [--models <dir>] [--out <dir>]
//!     [--credits <THIRD_PARTY_ASSETS.md>] [--blender <blender.exe>] [--work <dir>]
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use calyx_model_gen::layout::Layout;
use calyx_model_gen::{blender, credits, generate, params};
use clap::Parser;

const DEFAULT_BLENDER: &str = r"C:\Program Files\Blender Foundation\Blender 4.0\blender.exe";

/// Builds every model in the parameter folder with headless Blender into
/// the output folder, with its outline hull and the palette materials of
/// all three maps, keeping every asset id across runs.
#[derive(Parser)]
#[command(name = "calyx_model_gen")]
struct Cli {
    /// The folder of model parameter files, one `<model>.toml` each.
    #[arg(long, default_value = "sandbox/asset_sources/models")]
    models: PathBuf,
    /// The output folder, inside a project's assets folder.
    #[arg(long, default_value = "sandbox/assets/generated")]
    out: PathBuf,
    /// The credits every sourced (`import`) model must appear in.
    #[arg(long, default_value = "sandbox/THIRD_PARTY_ASSETS.md")]
    credits: PathBuf,
    /// Blender 4.x, which builds the models.
    #[arg(long, default_value = DEFAULT_BLENDER)]
    blender: PathBuf,
    /// Where to put the run's scratch folder, outside every asset root;
    /// defaults to the parameter folder's parent.
    #[arg(long)]
    work: Option<PathBuf>,
}

fn run(cli: Cli) -> Result<(), String> {
    let models = params::load_dir(&cli.models)?;
    let credit_text = std::fs::read_to_string(&cli.credits)
        .map_err(|err| format!("cannot read {}: {err}", cli.credits.display()))?;
    credits::check(&credit_text, &models)?;
    let layout = Layout::new(&cli.out)?;
    if !cli.blender.is_file() {
        return Err(format!(
            "Blender not found at {}; pass --blender",
            cli.blender.display()
        ));
    }
    let work = cli
        .work
        .unwrap_or_else(|| cli.models.parent().unwrap_or(&cli.models).to_path_buf())
        .join(format!(".model-gen-{}", std::process::id()));
    let result = generate(&layout, &models, |jobs| {
        blender::build(&cli.blender, &work, jobs)
    });
    let _ = std::fs::remove_dir_all(&work);
    println!("{}", result?);
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
