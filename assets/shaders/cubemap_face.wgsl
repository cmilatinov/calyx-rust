struct CubemapFace {
    forward: vec3f,
    up: vec3f,
    right: vec3f,
}

const CUBEMAP_FACES: array<CubemapFace, 6> = array(
    CubemapFace(
        vec3(1.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0),
        vec3(0.0, 0.0, -1.0),
    ),
    CubemapFace (
        vec3(-1.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0),
        vec3(0.0, 0.0, 1.0),
    ),
    CubemapFace (
        vec3(0.0, -1.0, 0.0),
        vec3(0.0, 0.0, 1.0),
        vec3(1.0, 0.0, 0.0),
    ),
    CubemapFace (
        vec3(0.0, 1.0, 0.0),
        vec3(0.0, 0.0, -1.0),
        vec3(1.0, 0.0, 0.0),
    ),
    CubemapFace (
        vec3(0.0, 0.0, 1.0),
        vec3(0.0, 1.0, 0.0),
        vec3(1.0, 0.0, 0.0),
    ),
    CubemapFace (
        vec3(0.0, 0.0, -1.0),
        vec3(0.0, 1.0, 0.0),
        vec3(-1.0, 0.0, 0.0),
    ),
);