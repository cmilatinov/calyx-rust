use glm::{Mat4, Vec2, Vec3};

pub struct Mesh {
    indices: Vec<u32>,
    vertices: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: (Vec<Vec2>,
         Vec<Vec2>,
         Vec<Vec2>,
         Vec<Vec2>),
    instances: Vec<Mat4>
}

impl Mesh {
    pub fn new() -> Self {
        todo!()
    }

    pub fn clear(&mut self) {
        self.indices.clear();
        self.vertices.clear();
        self.normals.clear();
        self.uvs.0.clear();
        self.uvs.1.clear();
        self.uvs.2.clear();
        self.uvs.3.clear();
    }
}
