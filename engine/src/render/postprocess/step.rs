pub enum PostProcessStep {
    Bloom { threshold: f32 },
    DepthOfField { focus_point: f32 },
    MotionBlur { amount: f32 },
    ToneMapping { gamma: f32, exposure: f32 },
    ColorGrading,
}
