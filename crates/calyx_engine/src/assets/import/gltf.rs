//! glTF and GLB import.
//!
//! Each glTF material becomes an [`ImportedMaterial`] on the metallic-roughness inputs: base
//! color factor and texture, metallic and roughness factors and texture, normal texture and
//! scale, emissive factor and texture, and occlusion texture and strength. `OPAQUE` and `MASK`
//! alpha modes are imported, `MASK` with its `alphaCutoff`; a `BLEND` material is imported as
//! opaque. Each pair of an image and a sampler a material samples becomes an
//! [`ImportedTexture`], so an image several slots or materials use is decoded once. Images are
//! read from a buffer view, a data URI or an external file, and one that is missing or does not
//! decode is logged and left for the slot's default. Sampler wrap modes and filters are kept.
//! Each primitive keeps the index of its material. Vertex tangents are read from `TANGENT` when
//! the primitive has them.
//!
//! Not imported: cameras, lights, morph targets, vertex colours, `doubleSided`, the `BLEND`
//! alpha mode and transparency sorting, texture coordinate sets other than `TEXCOORD_0` (a slot
//! that names another set samples `TEXCOORD_0`), and the `KHR_materials_*`,
//! `KHR_texture_transform` and `KHR_texture_basisu` extensions.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use gltf::animation::util::ReadOutputs;
use gltf::animation::{Interpolation, Property};
use gltf::image::Format;
use gltf::material::AlphaMode;
use gltf::mesh::Mode;
use gltf::texture::{MagFilter, MinFilter, WrappingMode};
use image::DynamicImage;
use nalgebra::Unit;
use nalgebra_glm::{Mat4, Vec2, Vec3, Vec4};

use super::{
    quat, smooth_normals, Corner, ImportedAnimation, ImportedBone, ImportedMaterial, ImportedMesh,
    ImportedNode, ImportedSampler, ImportedScene, ImportedTexture, MeshBuilder, TextureFilter,
    TextureWrap, MAX_UV_CHANNELS,
};
use crate::assets::animation::{AnimationKeyFrames, QuatKeyFrame, VectorKeyFrame};
use crate::assets::error::AssetError;

/// Name of the node holding the scene's root nodes, as the FBX importer names its root.
const ROOT_NODE_NAME: &str = "RootNode";

/// A mesh as one node uses it: the mesh index and the node's skin index.
type MeshInstance = (usize, Option<usize>);

/// Rate at which cubic-spline channels are resampled into linear keys.
const CUBIC_SPLINE_SAMPLE_RATE: f64 = 30.0;

pub(super) fn import(path: &Path) -> Result<ImportedScene, AssetError> {
    let load_error = |error: gltf::Error| AssetError::LoadError.with_source(error.to_string());
    let gltf::Gltf { document, blob } = gltf::Gltf::open(path).map_err(load_error)?;
    let buffers = gltf::import_buffers(&document, path.parent(), blob).map_err(load_error)?;
    let names = unique_names(document.nodes().map(|node| node.name()));
    let (materials, textures) = import_materials(&document, path.parent(), &buffers);

    // A mesh is imported once per skin it is used with, named after the first node that uses it
    // with that skin, so each node shows the mesh with its own joints, or unskinned.
    let mut meshes = Vec::new();
    let mut mesh_parts: HashMap<MeshInstance, std::ops::Range<usize>> = HashMap::new();
    for node in document.nodes() {
        let Some(mesh) = node.mesh() else {
            continue;
        };
        let skin = node.skin();
        let instance = (mesh.index(), skin.as_ref().map(gltf::Skin::index));
        if mesh_parts.contains_key(&instance) {
            continue;
        }
        let name = &names[node.index()];
        let first = meshes.len();
        for primitive in mesh.primitives() {
            meshes.extend(import_primitive(
                &primitive,
                name,
                skin.as_ref(),
                &buffers,
                &names,
            )?);
        }
        mesh_parts.insert(instance, first..meshes.len());
    }

    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next());
    let root = ImportedNode {
        name: ROOT_NODE_NAME.to_string(),
        transform: Mat4::identity(),
        meshes: Vec::new(),
        children: scene
            .iter()
            .flat_map(|scene| scene.nodes())
            .map(|node| import_node(&node, &mesh_parts, &names))
            .collect(),
    };

    let animations = document
        .animations()
        .filter_map(|animation| import_animation(&animation, &buffers, &names).transpose())
        .collect::<Result<_, _>>()?;

    Ok(ImportedScene {
        meshes,
        root,
        animations,
        materials,
        textures,
    })
}

/// Imports every material of `document`, in document order, with the textures they sample.
/// External images are read relative to `base`.
fn import_materials(
    document: &gltf::Document,
    base: Option<&Path>,
    buffers: &[gltf::buffer::Data],
) -> (Vec<ImportedMaterial>, Vec<ImportedTexture>) {
    let mut textures = TextureImporter {
        base,
        buffers,
        images: HashMap::new(),
        indices: HashMap::new(),
        textures: Vec::new(),
    };
    let materials = document
        .materials()
        .enumerate()
        .map(|(index, material)| import_material(index, &material, &mut textures))
        .collect();
    (materials, textures.textures)
}

fn import_material(
    index: usize,
    material: &gltf::Material,
    textures: &mut TextureImporter,
) -> ImportedMaterial {
    let name = material
        .name()
        .filter(|name| !name.is_empty())
        .map_or_else(|| format!("material_{index}"), str::to_string);
    let pbr = material.pbr_metallic_roughness();
    let normal = material.normal_texture();
    let occlusion = material.occlusion_texture();
    let alpha_cutoff = match material.alpha_mode() {
        AlphaMode::Opaque => None,
        AlphaMode::Mask => Some(material.alpha_cutoff().unwrap_or(0.5)),
        AlphaMode::Blend => {
            log::warn!(
                "glTF material {name} uses the BLEND alpha mode, which is not supported; it is \
                 imported as opaque"
            );
            None
        }
    };
    let mut slot = |info: Option<(gltf::Texture, u32)>| {
        info.and_then(|(texture, tex_coord)| textures.import(&name, texture, tex_coord))
    };
    ImportedMaterial {
        base_color_factor: pbr.base_color_factor(),
        base_color_texture: slot(
            pbr.base_color_texture()
                .map(|info| (info.texture(), info.tex_coord())),
        ),
        metallic_factor: pbr.metallic_factor(),
        roughness_factor: pbr.roughness_factor(),
        metallic_roughness_texture: slot(
            pbr.metallic_roughness_texture()
                .map(|info| (info.texture(), info.tex_coord())),
        ),
        normal_scale: normal.as_ref().map_or(1.0, |normal| normal.scale()),
        normal_texture: slot(
            normal
                .as_ref()
                .map(|normal| (normal.texture(), normal.tex_coord())),
        ),
        emissive_factor: material.emissive_factor(),
        emissive_texture: slot(
            material
                .emissive_texture()
                .map(|info| (info.texture(), info.tex_coord())),
        ),
        occlusion_strength: occlusion
            .as_ref()
            .map_or(1.0, |occlusion| occlusion.strength()),
        occlusion_texture: slot(
            occlusion
                .as_ref()
                .map(|occlusion| (occlusion.texture(), occlusion.tex_coord())),
        ),
        alpha_cutoff,
        name,
    }
}

/// Collects the textures materials sample, one per pair of an image and a sampler, decoding
/// each image once.
struct TextureImporter<'a> {
    base: Option<&'a Path>,
    buffers: &'a [gltf::buffer::Data],
    /// Decoded pixels by image index, `None` for an image that could not be read.
    images: HashMap<usize, Option<Arc<DynamicImage>>>,
    /// Texture index by image index and sampler.
    indices: HashMap<(usize, ImportedSampler), usize>,
    textures: Vec<ImportedTexture>,
}

impl TextureImporter<'_> {
    /// The index of the texture `texture` names, for a slot of the material named `material`,
    /// or `None` when its image is missing or does not decode.
    fn import(&mut self, material: &str, texture: gltf::Texture, tex_coord: u32) -> Option<usize> {
        if tex_coord != 0 {
            log::warn!(
                "glTF material {material} samples a texture with TEXCOORD_{tex_coord}, which is \
                 not supported; it samples TEXCOORD_0"
            );
        }
        let image = texture.source();
        let sampler = import_sampler(&texture.sampler());
        let key = (image.index(), sampler);
        if let Some(&index) = self.indices.get(&key) {
            return Some(index);
        }
        let (base, buffers) = (self.base, self.buffers);
        let pixels = self
            .images
            .entry(image.index())
            .or_insert_with(|| match decode_image(&image, base, buffers) {
                Ok(pixels) => Some(Arc::new(pixels)),
                Err(error) => {
                    log::warn!(
                        "glTF image {} of material {material} could not be read, so the slot \
                         samples its default: {error}",
                        image_name(&image)
                    );
                    None
                }
            })
            .clone()?;
        let index = self.textures.len();
        self.textures.push(ImportedTexture {
            name: image_name(&image),
            image: pixels,
            sampler,
        });
        self.indices.insert(key, index);
        Some(index)
    }
}

/// The name of `image`: its own, else the file name of an external image without its
/// extension, else a name made from its index.
fn image_name(image: &gltf::Image) -> String {
    if let Some(name) = image.name().filter(|name| !name.is_empty()) {
        return name.to_string();
    }
    if let gltf::image::Source::Uri { uri, .. } = image.source() {
        if !uri.starts_with("data:") {
            let stem = Path::new(uri)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|stem| !stem.is_empty());
            if let Some(stem) = stem {
                return stem.to_string();
            }
        }
    }
    format!("image_{}", image.index())
}

/// Decodes `image` from its buffer view, its data URI or its file relative to `base`.
fn decode_image(
    image: &gltf::Image,
    base: Option<&Path>,
    buffers: &[gltf::buffer::Data],
) -> Result<DynamicImage, String> {
    if let gltf::image::Source::View { view, .. } = image.source() {
        let length = buffers
            .get(view.buffer().index())
            .map_or(0, |data| data.0.len());
        if view.offset() + view.length() > length {
            return Err(format!(
                "its buffer view ends at byte {} of a {length} byte buffer",
                view.offset() + view.length()
            ));
        }
    }
    let data = gltf::image::Data::from_source(image.source(), base, buffers)
        .map_err(|error| error.to_string())?;
    let (width, height, pixels) = (data.width, data.height, data.pixels);
    let wide = || {
        pixels
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&bytes| u16::from_ne_bytes(bytes))
            .collect::<Vec<_>>()
    };
    let float = || {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&bytes| f32::from_ne_bytes(bytes))
            .collect::<Vec<_>>()
    };
    let image = match data.format {
        Format::R8 => image::GrayImage::from_raw(width, height, pixels.clone()).map(Into::into),
        Format::R8G8 => {
            image::GrayAlphaImage::from_raw(width, height, pixels.clone()).map(Into::into)
        }
        Format::R8G8B8 => image::RgbImage::from_raw(width, height, pixels.clone()).map(Into::into),
        Format::R8G8B8A8 => {
            image::RgbaImage::from_raw(width, height, pixels.clone()).map(Into::into)
        }
        Format::R16 => image::ImageBuffer::<image::Luma<u16>, _>::from_raw(width, height, wide())
            .map(Into::into),
        Format::R16G16 => {
            image::ImageBuffer::<image::LumaA<u16>, _>::from_raw(width, height, wide())
                .map(Into::into)
        }
        Format::R16G16B16 => {
            image::ImageBuffer::<image::Rgb<u16>, _>::from_raw(width, height, wide())
                .map(Into::into)
        }
        Format::R16G16B16A16 => {
            image::ImageBuffer::<image::Rgba<u16>, _>::from_raw(width, height, wide())
                .map(Into::into)
        }
        Format::R32G32B32FLOAT => {
            image::Rgb32FImage::from_raw(width, height, float()).map(Into::into)
        }
        Format::R32G32B32A32FLOAT => {
            image::Rgba32FImage::from_raw(width, height, float()).map(Into::into)
        }
    };
    image.ok_or_else(|| "its decoded pixels do not fill its size".to_string())
}

/// The wrap modes and filters of a glTF sampler. An unset filter is linear, with mips.
fn import_sampler(sampler: &gltf::texture::Sampler) -> ImportedSampler {
    use TextureFilter::{Linear, Nearest};
    let wrap = |mode: WrappingMode| match mode {
        WrappingMode::Repeat => TextureWrap::Repeat,
        WrappingMode::MirroredRepeat => TextureWrap::MirroredRepeat,
        WrappingMode::ClampToEdge => TextureWrap::ClampToEdge,
    };
    let (min_filter, mipmap_filter) = match sampler.min_filter() {
        Some(MinFilter::Nearest) => (Nearest, None),
        Some(MinFilter::Linear) => (Linear, None),
        Some(MinFilter::NearestMipmapNearest) => (Nearest, Some(Nearest)),
        Some(MinFilter::LinearMipmapNearest) => (Linear, Some(Nearest)),
        Some(MinFilter::NearestMipmapLinear) => (Nearest, Some(Linear)),
        Some(MinFilter::LinearMipmapLinear) | None => (Linear, Some(Linear)),
    };
    ImportedSampler {
        wrap_u: wrap(sampler.wrap_s()),
        wrap_v: wrap(sampler.wrap_t()),
        mag_filter: match sampler.mag_filter() {
            Some(MagFilter::Nearest) => Nearest,
            Some(MagFilter::Linear) | None => Linear,
        },
        min_filter,
        mipmap_filter,
    }
}

fn buffer_data<'a>(buffers: &'a [gltf::buffer::Data], buffer: gltf::Buffer) -> Option<&'a [u8]> {
    buffers.get(buffer.index()).map(|data| &data.0[..])
}

/// A distinct name for each node, by node index. Bones and animation channels refer to nodes by
/// name, but glTF names are optional and need not be unique: a node keeps its name when it is the
/// first to use it, and otherwise, or when unnamed, gets a name made from its index that no node
/// is given.
fn unique_names<'a>(names: impl Iterator<Item = Option<&'a str>>) -> Vec<String> {
    let names = names
        .map(|name| name.filter(|name| !name.is_empty()))
        .collect::<Vec<_>>();
    let given = names.iter().flatten().copied().collect::<HashSet<_>>();
    let mut taken = HashSet::new();
    names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let mut unique = match name {
                Some(name) if !taken.contains(*name) => name.to_string(),
                Some(name) => format!("{name}_{index}"),
                None => format!("node_{index}"),
            };
            while taken.contains(&unique)
                || (name != &Some(unique.as_str()) && given.contains(unique.as_str()))
            {
                unique = format!("{unique}_{index}");
            }
            taken.insert(unique.clone());
            unique
        })
        .collect()
}

fn import_node(
    node: &gltf::Node,
    mesh_parts: &HashMap<MeshInstance, std::ops::Range<usize>>,
    names: &[String],
) -> ImportedNode {
    ImportedNode {
        name: names[node.index()].clone(),
        transform: Mat4::from(node.transform().matrix()),
        meshes: node
            .mesh()
            .and_then(|mesh| mesh_parts.get(&(mesh.index(), node.skin().map(|skin| skin.index()))))
            .cloned()
            .map(Iterator::collect)
            .unwrap_or_default(),
        children: node
            .children()
            .map(|child| import_node(&child, mesh_parts, names))
            .collect(),
    }
}

/// Imports a triangle primitive, or returns `None` for points, lines and empty primitives.
fn import_primitive(
    primitive: &gltf::Primitive,
    name: &str,
    skin: Option<&gltf::Skin>,
    buffers: &[gltf::buffer::Data],
    names: &[String],
) -> Result<Option<ImportedMesh>, AssetError> {
    let reader = primitive.reader(|data| buffer_data(buffers, data));
    let Some(positions) = reader.read_positions() else {
        return Ok(None);
    };
    let positions = positions.map(Vec3::from).collect::<Vec<_>>();
    let indices = match reader.read_indices() {
        Some(indices) => indices.into_u32().collect::<Vec<_>>(),
        None => (0..positions.len() as u32).collect(),
    };
    if let Some(&index) = indices.iter().find(|&&i| i as usize >= positions.len()) {
        return Err(AssetError::LoadError.with_source(format!(
            "primitive index {index} is out of range for {} vertices",
            positions.len()
        )));
    }
    let triangles = match primitive.mode() {
        Mode::Triangles => indices.as_chunks::<3>().0.to_vec(),
        // Every other strip triangle is reversed, so all keep the winding of the first.
        Mode::TriangleStrip => indices
            .windows(3)
            .enumerate()
            .map(|(i, t)| {
                if i.is_multiple_of(2) {
                    [t[0], t[1], t[2]]
                } else {
                    [t[1], t[0], t[2]]
                }
            })
            .collect(),
        Mode::TriangleFan => indices
            .windows(2)
            .skip(1)
            .map(|t| [indices[0], t[0], t[1]])
            .collect(),
        Mode::Points | Mode::Lines | Mode::LineLoop | Mode::LineStrip => return Ok(None),
    };
    if triangles.is_empty() {
        return Ok(None);
    }

    let normals = match reader.read_normals() {
        Some(normals) => {
            let normals = normals.map(Vec3::from).collect::<Vec<_>>();
            triangles
                .iter()
                .map(|t| t.map(|i| normals.get(i as usize).copied().unwrap_or_default()))
                .collect()
        }
        None => smooth_normals(
            &triangles
                .iter()
                .map(|t| t.map(|i| positions[i as usize]))
                .collect::<Vec<_>>(),
        ),
    };
    // Tangents are only read with normals: they are given relative to them.
    let tangents = reader
        .read_tangents()
        .filter(|_| reader.read_normals().is_some())
        .map(|tangents| tangents.map(Vec4::from).collect::<Vec<_>>())
        .filter(|tangents| tangents.len() == positions.len());
    let uv_sets = (0..MAX_UV_CHANNELS as u32)
        .map_while(|set| reader.read_tex_coords(set))
        .map(|uvs| uvs.into_f32().map(Vec2::from).collect::<Vec<_>>())
        .collect::<Vec<_>>();

    // Joint and weight sets beyond the first are dropped: the renderer takes four weights.
    let influences = match (skin, reader.read_joints(0), reader.read_weights(0)) {
        (Some(_), Some(joints), Some(weights)) => joints
            .into_u16()
            .zip(weights.into_f32())
            .map(|(joints, weights)| {
                joints
                    .into_iter()
                    .zip(weights)
                    .filter(|(_, weight)| *weight > 0.0)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let mut weight_sets = HashMap::new();
    let skin_weights = influences
        .iter()
        .map(|influence| {
            let key = influence
                .iter()
                .map(|(joint, weight)| (*joint, weight.to_bits()))
                .collect::<Vec<_>>();
            let next = weight_sets.len() as u32;
            *weight_sets.entry(key).or_insert(next)
        })
        .collect::<Vec<_>>();

    let mut builder = MeshBuilder::new(name.to_string(), uv_sets.len());
    for (triangle, normals) in triangles.iter().zip(normals) {
        builder.push_triangle([0, 1, 2].map(|i| {
            let index = triangle[i];
            let mut uvs = [Vec2::zeros(); MAX_UV_CHANNELS];
            for (uv, set) in uvs.iter_mut().zip(&uv_sets) {
                *uv = set.get(index as usize).copied().unwrap_or_default();
            }
            Corner {
                control_point: index,
                skin_weights: skin_weights.get(index as usize).copied().unwrap_or(0),
                position: positions[index as usize],
                normal: normals[i],
                uvs,
                tangent: tangents.as_ref().map(|tangents| tangents[index as usize]),
            }
        }));
    }
    let (mut mesh, control_points) = builder.finish();
    mesh.material = primitive.material().index();
    if let Some(skin) = skin {
        mesh.bones = import_skin(skin, &influences, &control_points, buffers, names);
    }
    Ok(Some(mesh))
}

/// One bone per joint of `skin`, with the weights of the output vertices that came from each
/// influenced source vertex.
fn import_skin(
    skin: &gltf::Skin,
    influences: &[Vec<(u16, f32)>],
    control_points: &[u32],
    buffers: &[gltf::buffer::Data],
    names: &[String],
) -> Vec<ImportedBone> {
    let inverse_binds = skin
        .reader(|data| buffer_data(buffers, data))
        .read_inverse_bind_matrices()
        .map(|matrices| matrices.map(Mat4::from).collect::<Vec<_>>())
        .unwrap_or_default();
    let mut bones = skin
        .joints()
        .enumerate()
        .map(|(joint, node)| ImportedBone {
            name: names[node.index()].clone(),
            inverse_bind_transform: inverse_binds
                .get(joint)
                .copied()
                .unwrap_or_else(Mat4::identity),
            weights: Vec::new(),
        })
        .collect::<Vec<_>>();
    for (vertex, &source) in control_points.iter().enumerate() {
        for &(joint, weight) in influences.get(source as usize).into_iter().flatten() {
            if let Some(bone) = bones.get_mut(joint as usize) {
                bone.weights.push((vertex as u32, weight));
            }
        }
    }
    bones
}

/// Imports one animation, or returns `None` when it animates no node transform. Linear and step
/// keys are kept, a step as a pair of keys at each key time; cubic-spline channels are resampled
/// into linear keys at [`CUBIC_SPLINE_SAMPLE_RATE`].
fn import_animation(
    animation: &gltf::Animation,
    buffers: &[gltf::buffer::Data],
    names: &[String],
) -> Result<Option<ImportedAnimation>, AssetError> {
    let mut channels: HashMap<String, AnimationKeyFrames> = HashMap::new();
    let mut duration_seconds = 0.0f64;
    for channel in animation.channels() {
        let reader = channel.reader(|data| buffer_data(buffers, data));
        let (Some(inputs), Some(outputs)) = (reader.read_inputs(), reader.read_outputs()) else {
            continue;
        };
        let times = inputs.map(f64::from).collect::<Vec<_>>();
        let interpolation = channel.sampler().interpolation();
        let keys = |values: Vec<[f32; 4]>| -> Result<Vec<(f64, [f32; 4])>, AssetError> {
            let per_key = if interpolation == Interpolation::CubicSpline {
                3
            } else {
                1
            };
            if values.len() != times.len() * per_key {
                return Err(AssetError::LoadError.with_source(format!(
                    "animation channel has {} values for {} key times",
                    values.len(),
                    times.len()
                )));
            }
            Ok(match interpolation {
                Interpolation::Linear => times.iter().copied().zip(values).collect(),
                Interpolation::Step => step_keys(&times, &values),
                Interpolation::CubicSpline => resample_cubic_spline(&times, &values),
            })
        };
        let target = channel.target();
        let frames = channels
            .entry(names[target.node().index()].clone())
            .or_insert_with(|| AnimationKeyFrames {
                positions: Vec::new(),
                rotations: Vec::new(),
                scaling: Vec::new(),
            });
        let vector_keys = |keys: Vec<(f64, [f32; 4])>| {
            keys.into_iter()
                .map(|(time, [x, y, z, _])| VectorKeyFrame {
                    value: Vec3::new(x, y, z),
                    time,
                })
                .collect()
        };
        let extend = |values: [f32; 3]| [values[0], values[1], values[2], 0.0];
        match (target.property(), outputs) {
            (Property::Translation, ReadOutputs::Translations(values)) => {
                frames.positions = vector_keys(keys(values.map(extend).collect())?);
            }
            (Property::Scale, ReadOutputs::Scales(values)) => {
                frames.scaling = vector_keys(keys(values.map(extend).collect())?);
            }
            (Property::Rotation, ReadOutputs::Rotations(values)) => {
                frames.rotations = keys(values.into_f32().collect())?
                    .into_iter()
                    .map(|(time, [x, y, z, w])| QuatKeyFrame {
                        value: Unit::new_normalize(quat(x as f64, y as f64, z as f64, w as f64)),
                        time,
                    })
                    .collect();
            }
            // Morph target weights are not imported.
            _ => continue,
        }
        duration_seconds = duration_seconds.max(times.last().copied().unwrap_or(0.0));
    }
    channels.retain(|_, frames| {
        !(frames.positions.is_empty() && frames.rotations.is_empty() && frames.scaling.is_empty())
    });
    if channels.is_empty() {
        return Ok(None);
    }
    Ok(Some(ImportedAnimation {
        name: animation.name().unwrap_or_default().to_string(),
        duration_seconds,
        channels,
    }))
}

/// Step keys as linear keys: each value is held until the next key time, where a second key
/// at the same time switches to the next value, which the animator plays from that time on.
fn step_keys(times: &[f64], values: &[[f32; 4]]) -> Vec<(f64, [f32; 4])> {
    let mut keys = Vec::with_capacity(times.len() * 2);
    for (i, (&time, &value)) in times.iter().zip(values).enumerate() {
        if i > 0 {
            keys.push((time, values[i - 1]));
        }
        keys.push((time, value));
    }
    keys
}

/// Samples a cubic-spline channel, whose values are in-tangent, value and out-tangent triples,
/// at [`CUBIC_SPLINE_SAMPLE_RATE`] from its first key time to its last, and at every key time.
fn resample_cubic_spline(times: &[f64], values: &[[f32; 4]]) -> Vec<(f64, [f32; 4])> {
    let (Some(&first), Some(&last)) = (times.first(), times.last()) else {
        return Vec::new();
    };
    let value = |key: usize| values[key * 3 + 1];
    let mut sample_times = times.to_vec();
    let steps = ((last - first) * CUBIC_SPLINE_SAMPLE_RATE).floor() as usize;
    sample_times.extend((1..=steps).map(|step| first + step as f64 / CUBIC_SPLINE_SAMPLE_RATE));
    sample_times.sort_by(f64::total_cmp);
    sample_times.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    sample_times
        .into_iter()
        .map(|time| {
            let next = times
                .partition_point(|&key| key <= time)
                .min(times.len() - 1);
            if next == 0 || time >= last {
                let key = if time >= last { times.len() - 1 } else { 0 };
                return (time, value(key));
            }
            let previous = next - 1;
            let interval = times[next] - times[previous];
            let s = ((time - times[previous]) / interval) as f32;
            let interval = interval as f32;
            let (s2, s3) = (s * s, s * s * s);
            let p0 = value(previous);
            let m0 = values[previous * 3 + 2];
            let p1 = value(next);
            let m1 = values[next * 3];
            let sampled = std::array::from_fn(|i| {
                (2.0 * s3 - 3.0 * s2 + 1.0) * p0[i]
                    + (s3 - 2.0 * s2 + s) * interval * m0[i]
                    + (-2.0 * s3 + 3.0 * s2) * p1[i]
                    + (s3 - s2) * interval * m1[i]
            });
            (time, sampled)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_node_gets_a_distinct_name_and_given_names_win() {
        let names = unique_names(
            [
                Some("joint"),
                Some("joint"),
                None,
                Some(""),
                Some("joint_1"),
                Some("node_2"),
            ]
            .into_iter(),
        );

        assert_eq!(
            names,
            [
                "joint",
                "joint_1_1",
                "node_2_2",
                "node_3",
                "joint_1",
                "node_2"
            ]
        );
    }

    #[test]
    fn materials_textures_and_samplers_are_read_from_every_image_source() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/models/gltf/Materials.gltf");
        let scene = import(&path).expect("the material fixture imports");

        let names = scene
            .materials
            .iter()
            .map(|material| material.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["Painted", "material_1", "Unused"]);
        let painted = &scene.materials[0];
        assert_eq!(painted.alpha_cutoff, None);
        assert_eq!(painted.normal_texture, None, "the missing image is skipped");
        assert_eq!(
            painted.metallic_roughness_texture, painted.occlusion_texture,
            "one texture serves both slots of the ORM image"
        );
        assert_eq!(scene.materials[1].alpha_cutoff, Some(0.3));
        assert_eq!(
            scene
                .meshes
                .iter()
                .map(|mesh| mesh.material)
                .collect::<Vec<_>>(),
            [Some(0), Some(1), None]
        );

        let textures = scene
            .textures
            .iter()
            .map(|texture| {
                (
                    texture.name.as_str(),
                    (texture.image.width(), texture.image.height()),
                )
            })
            .collect::<Vec<_>>();
        // The buffer view image twice, with two samplers, then the external and data URI
        // images.
        assert_eq!(
            textures,
            [
                ("image_0", (2, 2)),
                ("Materials_orm", (2, 2)),
                ("Emissive", (1, 1)),
                ("image_0", (2, 2)),
            ]
        );
        assert!(std::sync::Arc::ptr_eq(
            &scene.textures[0].image,
            &scene.textures[3].image
        ));
        assert_eq!(scene.textures[0].sampler, ImportedSampler::default());
        assert_eq!(
            scene.textures[3].sampler,
            ImportedSampler {
                wrap_u: TextureWrap::MirroredRepeat,
                wrap_v: TextureWrap::ClampToEdge,
                mag_filter: TextureFilter::Nearest,
                min_filter: TextureFilter::Nearest,
                mipmap_filter: Some(TextureFilter::Linear),
            }
        );
    }

    #[test]
    fn step_keys_hold_each_value_until_the_next_key() {
        let keys = step_keys(&[0.0, 1.0, 2.0], &[[1.0; 4], [2.0; 4], [3.0; 4]]);
        assert_eq!(
            keys,
            vec![
                (0.0, [1.0; 4]),
                (1.0, [1.0; 4]),
                (1.0, [2.0; 4]),
                (2.0, [2.0; 4]),
                (2.0, [3.0; 4]),
            ]
        );
    }

    #[test]
    fn cubic_spline_is_resampled_at_thirty_hertz_through_its_keys() {
        // One component from 0 to 1 over one second, with zero tangents: a smoothstep.
        let zero = [0.0; 4];
        let values = [zero, [0.0; 4], zero, zero, [1.0; 4], zero];
        let keys = resample_cubic_spline(&[0.0, 1.0], &values);

        assert_eq!(keys.len(), 31);
        assert_eq!(keys[0], (0.0, [0.0; 4]));
        assert_eq!(keys[30], (1.0, [1.0; 4]));
        let (time, value) = keys[15];
        assert!((time - 0.5).abs() < 1e-9);
        assert!((value[0] - 0.5).abs() < 1e-6);
        let (time, value) = keys[6];
        let s = time as f32;
        assert!((value[0] - (3.0 * s * s - 2.0 * s * s * s)).abs() < 1e-6);
    }

    #[test]
    fn cubic_spline_follows_its_tangents() {
        // Constant value with a unit out-tangent leaving the first key bends upwards.
        let values = [[0.0; 4], [0.0; 4], [1.0; 4], [0.0; 4], [0.0; 4], [0.0; 4]];
        let keys = resample_cubic_spline(&[0.0, 1.0], &values);
        let (time, value) = keys[15];
        let s = time as f32;
        assert!((value[0] - (s * s * s - 2.0 * s * s + s)).abs() < 1e-6);
        assert!(value[0] > 0.0);
    }
}
