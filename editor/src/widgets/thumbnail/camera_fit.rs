use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) struct Bounds {
    pub(super) min: Vec3,
    pub(super) max: Vec3,
}

impl Default for Bounds {
    fn default() -> Self {
        Self::unit()
    }
}

impl Bounds {
    pub(super) fn unit() -> Self {
        Self {
            min: vec3(-0.5, -0.5, -0.5),
            max: vec3(0.5, 0.5, 0.5),
        }
    }

    fn from_points(points: &[Vec3]) -> Option<Self> {
        let mut points = points.iter();
        let first = *points.next()?;
        let mut bounds = Self {
            min: first,
            max: first,
        };
        for point in points {
            bounds.include(*point);
        }
        Some(bounds)
    }

    fn include(&mut self, point: Vec3) {
        self.min.x = self.min.x.min(point.x);
        self.min.y = self.min.y.min(point.y);
        self.min.z = self.min.z.min(point.z);
        self.max.x = self.max.x.max(point.x);
        self.max.y = self.max.y.max(point.y);
        self.max.z = self.max.z.max(point.z);
    }

    pub(super) fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub(super) fn radius(&self) -> f32 {
        let extents = self.max - self.min;
        extents.norm().max(0.5) * 0.5
    }

    pub(super) fn corners(&self) -> [Vec3; 8] {
        [
            vec3(self.min.x, self.min.y, self.min.z),
            vec3(self.max.x, self.min.y, self.min.z),
            vec3(self.min.x, self.max.y, self.min.z),
            vec3(self.max.x, self.max.y, self.min.z),
            vec3(self.min.x, self.min.y, self.max.z),
            vec3(self.max.x, self.min.y, self.max.z),
            vec3(self.min.x, self.max.y, self.max.z),
            vec3(self.max.x, self.max.y, self.max.z),
        ]
    }
}

#[derive(Clone, Debug)]
pub(super) struct CameraFit {
    pub(super) bounds: Bounds,
    pub(super) points: Vec<Vec3>,
}

impl Default for CameraFit {
    fn default() -> Self {
        Self::from_bounds(Bounds::unit())
    }
}

impl CameraFit {
    fn from_bounds(bounds: Bounds) -> Self {
        Self {
            bounds,
            points: bounds.corners().to_vec(),
        }
    }

    pub(super) fn from_mesh(mesh: &Mesh, transform: &Transform) -> Option<Self> {
        let points = mesh
            .vertices
            .iter()
            .map(|vertex| transform.transform_position(vertex))
            .collect::<Vec<_>>();
        Self::from_points(points)
    }

    pub(super) fn from_points(points: Vec<Vec3>) -> Option<Self> {
        let bounds = Bounds::from_points(&points)?;
        Some(Self { bounds, points })
    }

    pub(super) fn center(&self) -> Vec3 {
        self.bounds.center()
    }

    pub(super) fn radius(&self) -> f32 {
        let center = self.center();
        self.points
            .iter()
            .map(|point| (*point - center).norm())
            .fold(0.5, f32::max)
    }
}

pub(super) fn scene_mesh_camera_fit(
    context: &ReadOnlyAssetContext,
    scene: &Scene,
    root: GameObject,
) -> Option<CameraFit> {
    let mut points = Vec::new();
    for game_object in std::iter::once(root).chain(scene.descendants(root)) {
        let transform = scene.world_transform(game_object);
        if let Some(mesh_ref) = scene
            .read_component::<ComponentMesh, _, _>(game_object, |component| {
                component.mesh.get_ref(&context.registries)
            })
            .flatten()
        {
            let mesh = mesh_ref.read();
            append_transformed_mesh_points(&mut points, &mesh, &transform);
        }
        if let Some(mesh_ref) = scene
            .read_component::<ComponentSkinnedMesh, _, _>(game_object, |component| {
                component.mesh.get_ref(&context.registries)
            })
            .flatten()
        {
            let mesh = mesh_ref.read();
            append_transformed_mesh_points(&mut points, &mesh, &transform);
        }
    }
    CameraFit::from_points(points)
}

fn append_transformed_mesh_points(points: &mut Vec<Vec3>, mesh: &Mesh, transform: &Transform) {
    points.reserve(mesh.vertices.len());
    for vertex in &mesh.vertices {
        points.push(transform.transform_position(vertex));
    }
}
pub(super) fn camera_for_bounds(bounds: Bounds, frame_margin: f32) -> (Camera, Transform) {
    camera_for_fit(&CameraFit::from_bounds(bounds), frame_margin)
}

pub(super) fn camera_for_fit(camera_fit: &CameraFit, frame_margin: f32) -> (Camera, Transform) {
    const ASPECT: f32 = 1.0;
    const FOV_X: f32 = 40.0f32.to_radians();
    const MIN_DISTANCE: f32 = 0.01;

    let frame_margin = ThumbnailRenderSettings::with_frame_margin(frame_margin).frame_margin;
    let mut center = camera_fit.center();
    let radius = camera_fit.radius();
    let view_direction = vec3(0.9, 0.55, -0.85).normalize();
    let screen_fit = (1.0 - frame_margin * 2.0).max(0.01);
    let fit_camera = Camera::new(ASPECT, FOV_X, MIN_DISTANCE, 100_000.0);
    let mut distance = fit_camera_distance(
        camera_fit,
        &fit_camera,
        center,
        view_direction,
        screen_fit,
        radius,
    );

    for _ in 0..8 {
        let camera_transform = thumbnail_camera_transform(center, view_direction, distance);
        let Some(local_shift) = screen_centering_shift(camera_fit, &fit_camera, &camera_transform)
        else {
            break;
        };
        if local_shift.x.abs().max(local_shift.y.abs()) <= 0.0001 {
            break;
        }
        center += camera_transform.transform_direction(&local_shift);
        distance = fit_camera_distance(
            camera_fit,
            &fit_camera,
            center,
            view_direction,
            screen_fit,
            radius,
        );
    }

    let camera_transform = thumbnail_camera_transform(center, view_direction, distance);
    let max_depth = camera_fit
        .points
        .iter()
        .copied()
        .into_iter()
        .map(|corner| camera_transform.inverse_transform_position(&corner).z)
        .fold(distance, f32::max);
    let camera = Camera::new(ASPECT, FOV_X, 0.01, max_depth + radius.max(1.0));
    (camera, camera_transform)
}

fn fit_camera_distance(
    camera_fit: &CameraFit,
    camera: &Camera,
    center: Vec3,
    view_direction: Vec3,
    screen_fit: f32,
    radius: f32,
) -> f32 {
    const MIN_DISTANCE: f32 = 0.01;

    let mut distance = radius.max(MIN_DISTANCE);
    while distance < 100_000.0
        && !bounds_fit_screen_space(
            camera_fit,
            camera,
            &thumbnail_camera_transform(center, view_direction, distance),
            screen_fit,
        )
    {
        distance *= 2.0;
    }
    if !bounds_fit_screen_space(
        camera_fit,
        camera,
        &thumbnail_camera_transform(center, view_direction, distance),
        screen_fit,
    ) {
        distance = 100_000.0;
    }

    let mut near = MIN_DISTANCE;
    let mut far = distance;
    for _ in 0..32 {
        let mid = (near + far) * 0.5;
        if bounds_fit_screen_space(
            camera_fit,
            camera,
            &thumbnail_camera_transform(center, view_direction, mid),
            screen_fit,
        ) {
            far = mid;
        } else {
            near = mid;
        }
    }
    far.max(MIN_DISTANCE)
}

fn bounds_fit_screen_space(
    camera_fit: &CameraFit,
    camera: &Camera,
    camera_transform: &Transform,
    screen_fit: f32,
) -> bool {
    camera_fit.points.iter().copied().all(|corner| {
        project_corner(camera, camera_transform, corner).is_some_and(|projected| {
            projected.x.abs() <= screen_fit && projected.y.abs() <= screen_fit
        })
    })
}

fn screen_centering_shift(
    camera_fit: &CameraFit,
    camera: &Camera,
    camera_transform: &Transform,
) -> Option<Vec3> {
    let view_points = camera_fit
        .points
        .iter()
        .map(|point| camera_transform.inverse_transform_position(point))
        .filter(|point| point.z > camera.near_plane)
        .collect::<Vec<_>>();
    if view_points.is_empty() {
        return None;
    }

    Some(vec3(
        solve_screen_axis_shift(&view_points, camera.projection[(0, 0)], |point| point.x),
        solve_screen_axis_shift(&view_points, camera.projection[(1, 1)], |point| point.y),
        0.0,
    ))
}

fn solve_screen_axis_shift(
    view_points: &[Vec3],
    projection_scale: f32,
    axis: impl Fn(&Vec3) -> f32,
) -> f32 {
    let (mut min_axis, mut max_axis) = (f32::INFINITY, f32::NEG_INFINITY);
    for point in view_points {
        let value = axis(point);
        min_axis = min_axis.min(value);
        max_axis = max_axis.max(value);
    }

    let range = (max_axis - min_axis).abs().max(1.0);
    let mut low = min_axis - range * 4.0;
    let mut high = max_axis + range * 4.0;
    for _ in 0..40 {
        let mid = (low + high) * 0.5;
        let center = projected_axis_center(view_points, projection_scale, &axis, mid);
        if center > 0.0 {
            low = mid;
        } else {
            high = mid;
        }
    }
    (low + high) * 0.5
}

fn projected_axis_center(
    view_points: &[Vec3],
    projection_scale: f32,
    axis: impl Fn(&Vec3) -> f32,
    shift: f32,
) -> f32 {
    let (mut min_projected, mut max_projected) = (f32::INFINITY, f32::NEG_INFINITY);
    for point in view_points {
        let projected = projection_scale * (axis(point) - shift) / point.z;
        min_projected = min_projected.min(projected);
        max_projected = max_projected.max(projected);
    }
    (min_projected + max_projected) * 0.5
}

pub(super) fn project_corner(
    camera: &Camera,
    camera_transform: &Transform,
    corner: Vec3,
) -> Option<Vec3> {
    let view = camera_transform.inverse_matrix() * vec4(corner.x, corner.y, corner.z, 1.0);
    if view.z <= camera.near_plane {
        return None;
    }
    let clip = camera.projection * view;
    if clip.w.abs() <= f32::EPSILON {
        return None;
    }
    Some(vec3(clip.x / clip.w, clip.y / clip.w, clip.z / clip.w))
}

fn thumbnail_camera_transform(center: Vec3, view_direction: Vec3, distance: f32) -> Transform {
    let camera_position = center + view_direction * distance;
    let target_direction = (center - camera_position).normalize();
    Transform::from_components(
        camera_position,
        UnitQuaternion::face_towards(&target_direction, &Vec3::y_axis()),
        vec3(1.0, 1.0, 1.0),
    )
}
