use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use image::{imageops, DynamicImage, Rgba, RgbaImage};
use resvg::usvg::TreeParsing;
use serde::Serialize;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug, Parser)]
#[command(
    name = "icon_atlas",
    about = "Build a PNG texture atlas and JSON manifest from SVG or PNG icons."
)]
struct Config {
    /// Input folder or glob expression. Folder input is scanned recursively.
    #[arg(short, long)]
    input: String,

    /// Output atlas PNG path.
    #[arg(short = 'o', long, alias = "output")]
    atlas: PathBuf,

    /// Output manifest JSON path. Defaults to the atlas path with .json extension.
    #[arg(short, long)]
    manifest: Option<PathBuf>,

    /// Width and height of each icon tile in pixels.
    #[arg(long, default_value_t = 128)]
    tile_size: u32,

    /// Transparent padding around each tile in pixels.
    #[arg(long, default_value_t = 1)]
    padding: u32,

    /// Override automatic near-square packing with a fixed number of columns.
    #[arg(long)]
    columns: Option<u32>,

    /// Recolor non-transparent icon pixels. Accepts #RGB, #RGBA, #RRGGBB, or #RRGGBBAA.
    #[arg(long, value_parser = parse_color)]
    color: Option<IconColor>,
}

#[derive(Serialize)]
struct AtlasManifest {
    atlas: String,
    width: u32,
    height: u32,
    tile_size: u32,
    padding: u32,
    columns: u32,
    rows: u32,
    color: Option<String>,
    icons: BTreeMap<String, IconManifest>,
}

#[derive(Serialize)]
struct IconManifest {
    source: String,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
}

#[derive(Clone, Copy, Debug)]
struct IconColor(Rgba<u8>);

impl IconColor {
    fn css_hex(self) -> String {
        let Rgba([r, g, b, a]) = self.0;
        if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let config = Config::parse().validated()?;
    let inputs = collect_inputs(&config.input)?;
    if inputs.is_empty() {
        return Err(format!("no supported icon files matched '{}'", config.input).into());
    }

    let columns = config
        .columns
        .unwrap_or_else(|| square_columns(inputs.len() as u32));
    let rows = inputs.len().div_ceil(columns as usize) as u32;
    let cell_size = config
        .tile_size
        .checked_add(config.padding.saturating_mul(2))
        .ok_or("tile size plus padding overflowed")?;
    let width = columns
        .checked_mul(cell_size)
        .ok_or("atlas width overflowed")?;
    let height = rows
        .checked_mul(cell_size)
        .ok_or("atlas height overflowed")?;

    let mut atlas = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));
    let mut icons = BTreeMap::new();

    for (index, path) in inputs.iter().enumerate() {
        let index = index as u32;
        let col = index % columns;
        let row = index / columns;
        let x = col * cell_size + config.padding;
        let y = row * cell_size + config.padding;
        let mut tile = render_icon(path, config.tile_size)?;
        if let Some(color) = config.color {
            apply_color(&mut tile, color);
        }
        imageops::overlay(&mut atlas, &tile, x.into(), y.into());

        let name = icon_name(path);
        let previous = icons.insert(
            name.clone(),
            IconManifest {
                source: slash_path(path),
                x,
                y,
                w: config.tile_size,
                h: config.tile_size,
                u0: x as f32 / width as f32,
                v0: y as f32 / height as f32,
                u1: (x + config.tile_size) as f32 / width as f32,
                v1: (y + config.tile_size) as f32 / height as f32,
            },
        );
        if previous.is_some() {
            return Err(format!(
                "duplicate icon name '{name}'; use one input set at a time or rename colliding files"
            )
            .into());
        }
    }

    if let Some(parent) = config.atlas.parent() {
        fs::create_dir_all(parent)?;
    }
    atlas.save(&config.atlas)?;

    let manifest_path = config.manifest_path();
    let manifest = AtlasManifest {
        atlas: config
            .atlas
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("atlas.png")
            .to_string(),
        width,
        height,
        tile_size: config.tile_size,
        padding: config.padding,
        columns,
        rows,
        color: config.color.map(IconColor::css_hex),
        icons,
    };
    if let Some(parent) = manifest_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;

    println!(
        "packed {} icons into {} ({}x{}, {} columns, {} rows)",
        inputs.len(),
        config.atlas.display(),
        width,
        height,
        columns,
        rows
    );
    println!("wrote manifest {}", manifest_path.display());
    Ok(())
}

impl Config {
    fn validated(self) -> Result<Self> {
        if self.tile_size == 0 {
            return Err("--tile-size must be greater than zero".into());
        }
        if self.columns == Some(0) {
            return Err("--columns must be greater than zero".into());
        }
        Ok(self)
    }

    fn manifest_path(&self) -> PathBuf {
        self.manifest
            .clone()
            .unwrap_or_else(|| self.atlas.with_extension("json"))
    }
}

fn collect_inputs(input: &str) -> Result<Vec<PathBuf>> {
    let path = Path::new(input);
    let mut files = if path.is_dir() {
        let mut files = Vec::new();
        collect_folder(path, &mut files)?;
        files
    } else {
        glob::glob(input)?
            .filter_map(|entry| match entry {
                Ok(path) if path.is_file() => Some(Ok(path)),
                Ok(_) => None,
                Err(err) => Some(Err(err)),
            })
            .collect::<std::result::Result<Vec<_>, _>>()?
    };

    files.retain(|path| is_supported_icon(path));
    files.sort_by_key(|path| slash_path(path));
    files.dedup();
    Ok(files)
}

fn collect_folder(folder: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(folder)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_folder(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn is_supported_icon(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg") || ext.eq_ignore_ascii_case("png"))
}

fn render_icon(path: &Path, tile_size: u32) -> Result<RgbaImage> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("svg") => render_svg(path, tile_size),
        Some(ext) if ext.eq_ignore_ascii_case("png") => render_raster(path, tile_size),
        _ => Err(format!("unsupported icon type: {}", path.display()).into()),
    }
}

fn render_svg(path: &Path, tile_size: u32) -> Result<RgbaImage> {
    let mut options = resvg::usvg::Options::default();
    options.resources_dir = fs::canonicalize(path)
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));

    let data = fs::read(path)?;
    let tree = resvg::usvg::Tree::from_data(&data, &options)?;
    let tree = resvg::Tree::from_usvg(&tree);
    let svg_size = tree.size;
    let scale = (tile_size as f32 / svg_size.width()).min(tile_size as f32 / svg_size.height());
    let render_width = svg_size.width() * scale;
    let render_height = svg_size.height() * scale;
    let offset_x = (tile_size as f32 - render_width) * 0.5;
    let offset_y = (tile_size as f32 - render_height) * 0.5;
    let transform =
        resvg::tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, offset_x, offset_y);

    let mut pixmap = resvg::tiny_skia::Pixmap::new(tile_size, tile_size)
        .ok_or_else(|| format!("failed to allocate {tile_size}x{tile_size} SVG tile"))?;
    tree.render(transform, &mut pixmap.as_mut());

    let mut pixels = pixmap.take();
    unpremultiply_rgba(&mut pixels);
    RgbaImage::from_raw(tile_size, tile_size, pixels)
        .ok_or_else(|| "failed to convert SVG tile pixels".into())
}

fn render_raster(path: &Path, tile_size: u32) -> Result<RgbaImage> {
    let image = image::open(path)?.into_rgba8();
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return Err(format!("image has zero size: {}", path.display()).into());
    }

    let scale = (tile_size as f32 / width as f32).min(tile_size as f32 / height as f32);
    let scaled_width = ((width as f32 * scale).round() as u32).max(1);
    let scaled_height = ((height as f32 * scale).round() as u32).max(1);
    let resized = DynamicImage::ImageRgba8(image)
        .resize_exact(scaled_width, scaled_height, imageops::FilterType::Lanczos3)
        .into_rgba8();

    let mut tile = RgbaImage::from_pixel(tile_size, tile_size, Rgba([0, 0, 0, 0]));
    let x = (tile_size - scaled_width) / 2;
    let y = (tile_size - scaled_height) / 2;
    imageops::overlay(&mut tile, &resized, x.into(), y.into());
    Ok(tile)
}

fn apply_color(image: &mut RgbaImage, color: IconColor) {
    let Rgba([r, g, b, a]) = color.0;
    for pixel in image.pixels_mut() {
        let alpha = ((pixel[3] as u16 * a as u16 + 127) / 255) as u8;
        if alpha == 0 {
            *pixel = Rgba([0, 0, 0, 0]);
        } else {
            *pixel = Rgba([r, g, b, alpha]);
        }
    }
}

fn square_columns(count: u32) -> u32 {
    (count as f64).sqrt().ceil().max(1.0) as u32
}

fn icon_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("icon")
        .to_string()
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn unpremultiply_rgba(pixels: &mut [u8]) {
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = pixel[3] as u32;
        if alpha == 0 || alpha == 255 {
            continue;
        }
        pixel[0] = ((pixel[0] as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
        pixel[1] = ((pixel[1] as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
        pixel[2] = ((pixel[2] as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
    }
}

fn parse_color(value: &str) -> std::result::Result<IconColor, String> {
    let value = value
        .strip_prefix('#')
        .or_else(|| value.strip_prefix("0x"))
        .unwrap_or(value);

    let expanded = match value.len() {
        3 | 4 => value.chars().flat_map(|ch| [ch, ch]).collect::<String>(),
        6 | 8 => value.to_string(),
        _ => {
            return Err(
                "expected #RGB, #RGBA, #RRGGBB, #RRGGBBAA, or the same without #".to_string(),
            )
        }
    };

    let parse_pair = |range: std::ops::Range<usize>| {
        u8::from_str_radix(&expanded[range], 16)
            .map_err(|_| "color must contain only hexadecimal digits".to_string())
    };

    let r = parse_pair(0..2)?;
    let g = parse_pair(2..4)?;
    let b = parse_pair(4..6)?;
    let a = if expanded.len() == 8 {
        parse_pair(6..8)?
    } else {
        255
    };

    Ok(IconColor(Rgba([r, g, b, a])))
}
