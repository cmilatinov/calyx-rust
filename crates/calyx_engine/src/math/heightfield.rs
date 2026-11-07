use image::DynamicImage;
use nalgebra_glm as glm;
use nalgebra_glm::{vec2, vec3, vec4, Vec2, Vec3, Vec4};
use std::path::Path;

/// Most samples a [`Heightfield`] keeps along either side. A larger image is
/// resampled down to this many, so a terrain mesh stays within about a
/// million vertices.
pub const MAX_HEIGHTFIELD_SAMPLES: usize = 1024;

/// A grid of normalized heights, read from a heightmap image.
///
/// Sample `(column, row)` is the image's pixel at that column and row, and
/// holds a height from 0 (black) to 1 (white). Laid on a surface by a
/// [`HeightfieldExtent`], the grid spans the extent's local XZ rectangle,
/// centered on the origin: column 0 sits on the -X edge and the last column
/// on the +X edge, row 0 (the image's top) on the +Z edge and the last row on
/// the -Z edge, so the image reads unflipped seen from above with +Z up.
///
/// Every cell between four samples is split into two triangles along the
/// diagonal from its +X, +Z corner to its -X, -Z corner. [`Self::sample`]
/// and [`Self::geometry`] share that split, so a sampled height lies on the
/// drawn surface exactly.
#[derive(Debug, Clone, PartialEq)]
pub struct Heightfield {
    columns: usize,
    rows: usize,
    heights: Vec<f32>,
}

/// How a [`Heightfield`] is laid on a surface, in the surface's local units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeightfieldExtent {
    /// Width along X and depth along Z of the whole grid.
    pub size: Vec2,
    /// Height of a sample of 1; a sample of 0 lies on Y = 0.
    pub height_scale: f32,
}

impl HeightfieldExtent {
    /// Whether the extent lays a grid on a surface: a finite, positive size
    /// and a finite height scale.
    pub fn is_valid(&self) -> bool {
        self.size.x.is_finite()
            && self.size.y.is_finite()
            && self.size.x > 0.0
            && self.size.y > 0.0
            && self.height_scale.is_finite()
    }
}

/// The surface at one point of a [`Heightfield`], in its extent's local
/// space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeightfieldSample {
    /// Local Y of the surface.
    pub height: f32,
    /// Unit surface normal, interpolated across the triangle like the shaded
    /// normal of the drawn surface, so it always has a positive Y.
    pub normal: Vec3,
}

/// The triangle mesh of a [`Heightfield`]: one vertex per sample, in row
/// order, and two counter-clockwise triangles per cell seen from above.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HeightfieldGeometry {
    /// Local vertex positions.
    pub positions: Vec<Vec3>,
    /// Unit vertex normals, from the slope across the neighbouring samples.
    pub normals: Vec<Vec3>,
    /// Unit tangents along +U (+X), with the bitangent's handedness in `w`,
    /// as [`crate::assets::mesh::Mesh::tangents`] holds them.
    pub tangents: Vec<Vec4>,
    /// Texture coordinates: U from 0 on the -X edge to 1 on the +X edge, V
    /// from 0 on the +Z edge to 1 on the -Z edge.
    pub uvs: Vec<Vec2>,
    /// Triangle list indices into the vertices.
    pub indices: Vec<u32>,
}

impl Heightfield {
    /// A heightfield of `columns` by `rows` samples, given row by row.
    /// `None` unless both sides have from 2 to [`MAX_HEIGHTFIELD_SAMPLES`]
    /// samples, `heights` holds exactly that many, and every height is
    /// finite.
    pub fn new(columns: usize, rows: usize, heights: Vec<f32>) -> Option<Self> {
        let sides = 2..=MAX_HEIGHTFIELD_SAMPLES;
        (sides.contains(&columns)
            && sides.contains(&rows)
            && heights.len() == columns * rows
            && heights.iter().all(|h| h.is_finite()))
        .then_some(Self {
            columns,
            rows,
            heights,
        })
    }

    /// The heightfield of `image`'s brightness, at its full precision: a
    /// 16-bit grayscale image keeps all of its 65536 levels. An image wider
    /// or taller than [`MAX_HEIGHTFIELD_SAMPLES`] is resampled bilinearly
    /// down to it on that side. `None` for an image narrower or shorter than
    /// 2 pixels.
    pub fn from_image(image: &DynamicImage) -> Option<Self> {
        let luma = image.to_luma32f();
        let (width, height) = (luma.width() as usize, luma.height() as usize);
        if width < 2 || height < 2 {
            return None;
        }
        let source = |x: usize, y: usize| luma.get_pixel(x as u32, y as u32).0[0];
        let columns = width.min(MAX_HEIGHTFIELD_SAMPLES);
        let rows = height.min(MAX_HEIGHTFIELD_SAMPLES);
        let mut heights = Vec::with_capacity(columns * rows);
        for row in 0..rows {
            let y = resample(row, rows, height);
            for column in 0..columns {
                let x = resample(column, columns, width);
                let (x0, fx) = (x.floor() as usize, x.fract());
                let (y0, fy) = (y.floor() as usize, y.fract());
                let (x1, y1) = ((x0 + 1).min(width - 1), (y0 + 1).min(height - 1));
                let top = source(x0, y0) * (1.0 - fx) + source(x1, y0) * fx;
                let bottom = source(x0, y1) * (1.0 - fx) + source(x1, y1) * fx;
                heights.push(top * (1.0 - fy) + bottom * fy);
            }
        }
        Self::new(columns, rows, heights)
    }

    /// Reads the heightmap image at `path`; see [`Self::from_image`].
    ///
    /// # Errors
    ///
    /// Describes why the file could not be read or decoded, or why its image
    /// is no heightfield.
    pub fn load(path: &Path) -> Result<Self, String> {
        let image = image::ImageReader::open(path)
            .map_err(|err| err.to_string())?
            .with_guessed_format()
            .map_err(|err| err.to_string())?
            .decode()
            .map_err(|err| err.to_string())?;
        Self::from_image(&image).ok_or_else(|| {
            format!(
                "a {}x{} image is too small for a heightfield",
                image.width(),
                image.height()
            )
        })
    }

    /// Samples along X.
    pub fn columns(&self) -> usize {
        self.columns
    }

    /// Samples along Z.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// The normalized height of the sample at `column` and `row`.
    ///
    /// # Panics
    ///
    /// When the sample is outside the grid.
    pub fn height(&self, column: usize, row: usize) -> f32 {
        assert!(column < self.columns && row < self.rows);
        self.heights[row * self.columns + column]
    }

    /// Local distance between neighbouring samples along X and Z.
    fn spacing(&self, extent: &HeightfieldExtent) -> Vec2 {
        vec2(
            extent.size.x / (self.columns - 1) as f32,
            extent.size.y / (self.rows - 1) as f32,
        )
    }

    fn position(&self, extent: &HeightfieldExtent, column: usize, row: usize) -> Vec3 {
        let spacing = self.spacing(extent);
        vec3(
            column as f32 * spacing.x - extent.size.x * 0.5,
            self.height(column, row) * extent.height_scale,
            extent.size.y * 0.5 - row as f32 * spacing.y,
        )
    }

    /// The vertex normal at a sample, from the slope between its neighbours
    /// (one-sided on the edges).
    fn vertex_normal(&self, extent: &HeightfieldExtent, column: usize, row: usize) -> Vec3 {
        let spacing = self.spacing(extent);
        let (left, right) = (column.saturating_sub(1), (column + 1).min(self.columns - 1));
        // Rows run toward -Z, so the row above is the +Z neighbour.
        let (front, back) = (row.saturating_sub(1), (row + 1).min(self.rows - 1));
        let slope_x = (self.height(right, row) - self.height(left, row)) * extent.height_scale
            / ((right - left) as f32 * spacing.x);
        let slope_z = (self.height(column, front) - self.height(column, back))
            * extent.height_scale
            / ((back - front) as f32 * spacing.y);
        glm::normalize(&vec3(-slope_x, 1.0, -slope_z))
    }

    /// The surface at local `point` (X, Z) under `extent`, or `None` when the
    /// point is outside the grid's rectangle or the extent is not valid.
    pub fn sample(&self, extent: &HeightfieldExtent, point: Vec2) -> Option<HeightfieldSample> {
        if !extent.is_valid() {
            return None;
        }
        let u = point.x / extent.size.x + 0.5;
        let v = 0.5 - point.y / extent.size.y;
        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
            return None;
        }
        let x = u * (self.columns - 1) as f32;
        let y = v * (self.rows - 1) as f32;
        let column = (x.floor() as usize).min(self.columns - 2);
        let row = (y.floor() as usize).min(self.rows - 2);
        let (fx, fy) = (x - column as f32, y - row as f32);
        // Corners: a at (column, row), b one column on, c one row on, d both.
        let corner = |dc: usize, dr: usize| {
            (
                self.height(column + dc, row + dr) * extent.height_scale,
                self.vertex_normal(extent, column + dc, row + dr),
            )
        };
        let (b, c) = (corner(1, 0), corner(0, 1));
        let (height, normal) = if fx + fy <= 1.0 {
            // Triangle a, c, b, measured from a.
            let a = corner(0, 0);
            (
                a.0 + fx * (b.0 - a.0) + fy * (c.0 - a.0),
                a.1 + (b.1 - a.1) * fx + (c.1 - a.1) * fy,
            )
        } else {
            // Triangle b, c, d, measured from d.
            let d = corner(1, 1);
            let (gx, gy) = (1.0 - fx, 1.0 - fy);
            (
                d.0 + gx * (c.0 - d.0) + gy * (b.0 - d.0),
                d.1 + (c.1 - d.1) * gx + (b.1 - d.1) * gy,
            )
        };
        Some(HeightfieldSample {
            height,
            normal: glm::normalize(&normal),
        })
    }

    /// The triangle mesh of the grid under `extent`, or `None` when the
    /// extent is not valid.
    pub fn geometry(&self, extent: &HeightfieldExtent) -> Option<HeightfieldGeometry> {
        if !extent.is_valid() {
            return None;
        }
        let count = self.columns * self.rows;
        let mut geometry = HeightfieldGeometry {
            positions: Vec::with_capacity(count),
            normals: Vec::with_capacity(count),
            tangents: Vec::with_capacity(count),
            uvs: Vec::with_capacity(count),
            indices: Vec::with_capacity((self.columns - 1) * (self.rows - 1) * 6),
        };
        for row in 0..self.rows {
            for column in 0..self.columns {
                let normal = self.vertex_normal(extent, column, row);
                // +X projected onto the surface; the bitangent cross(normal,
                // tangent) * w must point up the image, toward +Z, so w = -1.
                let tangent = glm::normalize(&(Vec3::x() - normal * normal.x));
                geometry.positions.push(self.position(extent, column, row));
                geometry.normals.push(normal);
                geometry
                    .tangents
                    .push(vec4(tangent.x, tangent.y, tangent.z, -1.0));
                geometry.uvs.push(vec2(
                    column as f32 / (self.columns - 1) as f32,
                    row as f32 / (self.rows - 1) as f32,
                ));
            }
        }
        let index = |column: usize, row: usize| (row * self.columns + column) as u32;
        for row in 0..self.rows - 1 {
            for column in 0..self.columns - 1 {
                let (a, b) = (index(column, row), index(column + 1, row));
                let (c, d) = (index(column, row + 1), index(column + 1, row + 1));
                geometry.indices.extend_from_slice(&[a, c, b, b, c, d]);
            }
        }
        Some(geometry)
    }
}

/// The source coordinate of sample `index` of `count`, spread end to end over
/// `size` source pixels.
fn resample(index: usize, count: usize, size: usize) -> f32 {
    if count == size {
        index as f32
    } else {
        index as f32 * (size - 1) as f32 / (count - 1) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use image::{ImageBuffer, Luma};

    fn extent(size: f32, height_scale: f32) -> HeightfieldExtent {
        HeightfieldExtent {
            size: vec2(size, size),
            height_scale,
        }
    }

    /// Heights of the plane h = 0.1 * column + 0.05 * row.
    fn ramp(columns: usize, rows: usize) -> Heightfield {
        let heights = (0..rows)
            .flat_map(|row| (0..columns).map(move |column| 0.1 * column as f32 + 0.05 * row as f32))
            .collect();
        Heightfield::new(columns, rows, heights).unwrap()
    }

    #[test]
    fn sides_must_be_at_least_two_samples_and_heights_finite() {
        assert!(Heightfield::new(1, 4, vec![0.0; 4]).is_none());
        assert!(Heightfield::new(2, 2, vec![0.0; 3]).is_none());
        assert!(Heightfield::new(2, 2, vec![0.0, f32::NAN, 0.0, 0.0]).is_none());
        let too_wide = MAX_HEIGHTFIELD_SAMPLES + 1;
        assert!(Heightfield::new(too_wide, 2, vec![0.0; too_wide * 2]).is_none());
        assert!(Heightfield::new(2, 2, vec![0.0; 4]).is_some());
    }

    #[test]
    fn a_sixteen_bit_image_keeps_its_precision_and_rows_run_from_its_top() {
        let image = ImageBuffer::from_fn(3, 2, |x, y| Luma([(x * 1000 + y * 30000 + 1) as u16]));
        let field = Heightfield::from_image(&DynamicImage::ImageLuma16(image)).unwrap();
        assert_eq!((field.columns(), field.rows()), (3, 2));
        assert_abs_diff_eq!(field.height(0, 0), 1.0 / 65535.0, epsilon = 1e-7);
        assert_abs_diff_eq!(field.height(2, 1), 32001.0 / 65535.0, epsilon = 1e-7);
        // Row 0 is the image's top, on the +Z edge.
        let sample = field.sample(&extent(2.0, 1.0), vec2(-1.0, 1.0)).unwrap();
        assert_abs_diff_eq!(sample.height, field.height(0, 0), epsilon = 1e-6);
    }

    #[test]
    fn an_oversized_image_is_resampled_end_to_end() {
        let width = MAX_HEIGHTFIELD_SAMPLES as u32 * 2;
        let image = ImageBuffer::from_fn(width, 2, |x, _| {
            Luma([(x as f32 / (width - 1) as f32 * 65535.0).round() as u16])
        });
        let field = Heightfield::from_image(&DynamicImage::ImageLuma16(image)).unwrap();
        assert_eq!(field.columns(), MAX_HEIGHTFIELD_SAMPLES);
        assert_abs_diff_eq!(field.height(0, 0), 0.0, epsilon = 1e-6);
        assert_abs_diff_eq!(
            field.height(MAX_HEIGHTFIELD_SAMPLES - 1, 1),
            1.0,
            epsilon = 1e-6
        );
        assert_abs_diff_eq!(
            field.height(MAX_HEIGHTFIELD_SAMPLES / 2, 0),
            (MAX_HEIGHTFIELD_SAMPLES / 2) as f32 / (MAX_HEIGHTFIELD_SAMPLES - 1) as f32,
            epsilon = 2e-5
        );
        assert!(Heightfield::from_image(&DynamicImage::new_luma8(1, 8)).is_none());
    }

    #[test]
    fn a_plane_samples_its_height_and_normal_everywhere() {
        // 5 x 5 samples over 8 x 8 units: 2 units apart. Height rises 0.1 per
        // column (+X) and 0.05 per row (-Z), scaled by 4.
        let field = ramp(5, 5);
        let extent = extent(8.0, 4.0);
        let slope_x = 0.1 * 4.0 / 2.0;
        let slope_z = -0.05 * 4.0 / 2.0;
        let expected_normal = glm::normalize(&vec3(-slope_x, 1.0, -slope_z));
        for (x, z) in [
            (-4.0, 4.0),
            (0.3, -1.7),
            (3.9, -3.9),
            (4.0, -4.0),
            (-2.5, 0.75),
        ] {
            let sample = field.sample(&extent, vec2(x, z)).unwrap();
            let expected = (0.1 * (x + 4.0) / 2.0 + 0.05 * (4.0 - z) / 2.0) * 4.0;
            assert_abs_diff_eq!(sample.height, expected, epsilon = 1e-5);
            assert_abs_diff_eq!(sample.normal, expected_normal, epsilon = 1e-5);
        }
    }

    #[test]
    fn samples_follow_the_drawn_triangles_and_are_none_outside() {
        // One raised corner: (column 1, row 1), the +X, -Z corner. The cell's
        // diagonal runs from (1, 0) to (0, 1), so the a-c-b triangle stays
        // flat and only the b-c-d triangle rises.
        let field = Heightfield::new(2, 2, vec![0.0, 0.0, 0.0, 1.0]).unwrap();
        let extent = extent(2.0, 1.0);
        let flat = field.sample(&extent, vec2(-0.5, 0.5)).unwrap();
        assert_abs_diff_eq!(flat.height, 0.0, epsilon = 1e-6);
        let raised = field.sample(&extent, vec2(0.5, -0.5)).unwrap();
        assert_abs_diff_eq!(raised.height, 0.5, epsilon = 1e-6);
        let corner = field.sample(&extent, vec2(1.0, -1.0)).unwrap();
        assert_abs_diff_eq!(corner.height, 1.0, epsilon = 1e-6);
        // On the diagonal both triangles agree.
        let diagonal = field.sample(&extent, vec2(0.0, 0.0)).unwrap();
        assert_abs_diff_eq!(diagonal.height, 0.0, epsilon = 1e-6);
        assert!(raised.normal.y > 0.0 && raised.normal.x < 0.0 && raised.normal.z > 0.0);

        for outside in [vec2(1.01, 0.0), vec2(0.0, -1.01), vec2(f32::NAN, 0.0)] {
            assert!(field.sample(&extent, outside).is_none());
        }
        let invalid = HeightfieldExtent {
            size: vec2(0.0, 2.0),
            height_scale: 1.0,
        };
        assert!(field.sample(&invalid, Vec2::zeros()).is_none());
        assert!(field.geometry(&invalid).is_none());
    }

    #[test]
    fn geometry_has_a_vertex_per_sample_and_upward_counter_clockwise_triangles() {
        let field = ramp(4, 3);
        let extent = HeightfieldExtent {
            size: vec2(6.0, 4.0),
            height_scale: 2.0,
        };
        let geometry = field.geometry(&extent).unwrap();
        assert_eq!(geometry.positions.len(), 12);
        assert_eq!(geometry.normals.len(), 12);
        assert_eq!(geometry.tangents.len(), 12);
        assert_eq!(geometry.uvs.len(), 12);
        assert_eq!(geometry.indices.len(), 3 * 2 * 6);
        assert_eq!(geometry.positions[0], vec3(-3.0, 0.0, 2.0));
        assert_abs_diff_eq!(
            geometry.positions[11],
            vec3(3.0, (0.3 + 0.1) * 2.0, -2.0),
            epsilon = 1e-6
        );
        assert_eq!(geometry.uvs[0], vec2(0.0, 0.0));
        assert_eq!(geometry.uvs[11], vec2(1.0, 1.0));
        for triangle in geometry.indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|i| geometry.positions[triangle[i] as usize]);
            // Counter-clockwise seen from above, with X right and Z up the
            // screen: the 2D cross product of the XZ edges is positive.
            let (e1, e2) = (b - a, c - a);
            assert!(e1.x * e2.z - e1.z * e2.x > 0.0, "triangle {triangle:?}");
            // Every sample's height and normal lies on the drawn surface.
            for vertex in [a, b, c] {
                let sample = field.sample(&extent, vec2(vertex.x, vertex.z)).unwrap();
                assert_abs_diff_eq!(sample.height, vertex.y, epsilon = 1e-5);
            }
            let centroid = (a + b + c) / 3.0;
            let sample = field.sample(&extent, vec2(centroid.x, centroid.z)).unwrap();
            assert_abs_diff_eq!(sample.height, centroid.y, epsilon = 1e-5);
        }
        for (normal, tangent) in geometry.normals.iter().zip(&geometry.tangents) {
            assert_abs_diff_eq!(normal.norm(), 1.0, epsilon = 1e-5);
            assert!(normal.y > 0.0);
            let tangent = tangent.xyz();
            assert_abs_diff_eq!(tangent.norm(), 1.0, epsilon = 1e-5);
            assert_abs_diff_eq!(tangent.dot(normal), 0.0, epsilon = 1e-5);
        }
    }

    #[test]
    fn a_heightmap_file_loads_at_full_precision() {
        let dir = std::env::temp_dir().join(format!("heightfield-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("height.png");
        ImageBuffer::from_fn(4, 4, |x, y| Luma([(x * 4 + y) as u16 * 4000]))
            .save(&path)
            .unwrap();
        let field = Heightfield::load(&path).unwrap();
        assert_eq!((field.columns(), field.rows()), (4, 4));
        assert_abs_diff_eq!(field.height(3, 2), 56000.0 / 65535.0, epsilon = 1e-7);
        assert!(Heightfield::load(&dir.join("missing.png")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
