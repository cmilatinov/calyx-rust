use egui_wgpu::wgpu;

pub fn wgpu_buffer_init_desc<T: bytemuck::Pod>(
    usage: wgpu::BufferUsages,
    contents: &[T]
) -> wgpu::util::BufferInitDescriptor {
    wgpu::util::BufferInitDescriptor {
        label: None,
        usage,
        contents: bytemuck::cast_slice(contents)
    }
}

pub trait BufferLayout {
    const ATTRIBS: &'static [wgpu::VertexAttribute];
    fn layout(step_mode: wgpu::VertexStepMode) -> wgpu::VertexBufferLayout<'static> where Self: Sized {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode,
            attributes: Self::ATTRIBS
        }
    }
}