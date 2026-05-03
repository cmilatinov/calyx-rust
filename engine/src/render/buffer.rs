use egui_wgpu::wgpu;

/// Builds a `BufferInitDescriptor` from typed POD data.
pub fn wgpu_buffer_init_desc<T: bytemuck::Pod>(
    usage: wgpu::BufferUsages,
    contents: &[T],
) -> wgpu::util::BufferInitDescriptor<'_> {
    wgpu::util::BufferInitDescriptor {
        label: None,
        usage,
        contents: bytemuck::cast_slice(contents),
    }
}

/// Trait for vertex-like types that can describe their buffer layout.
pub trait BufferLayout {
    /// Vertex attributes exposed by the type.
    const ATTRIBS: &'static [wgpu::VertexAttribute];
    /// Builds a wgpu vertex buffer layout for this type.
    fn layout(step_mode: wgpu::VertexStepMode) -> wgpu::VertexBufferLayout<'static>
    where
        Self: Sized,
    {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode,
            attributes: Self::ATTRIBS,
        }
    }
}

/// GPU buffer that grows on demand while preserving usage flags.
pub struct ResizableBuffer {
    usage: wgpu::BufferUsages,
    size: u64,
    buffer: Option<wgpu::Buffer>,
}

impl ResizableBuffer {
    /// Creates an empty resizable buffer with the provided `usage`.
    pub fn new(usage: wgpu::BufferUsages) -> Self {
        Self {
            usage,
            size: 0,
            buffer: None,
        }
    }

    /// Ensures the buffer is at least `size` bytes long.
    pub fn resize(&mut self, device: &wgpu::Device, size: u64) {
        if self.size >= size {
            return;
        }
        let new_size = if self.size == 0 {
            size
        } else {
            let mut sz = self.size;
            while sz < size {
                sz *= 2;
            }
            sz
        };
        self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: new_size,
            usage: self.usage,
            mapped_at_creation: false,
        }));
        self.size = new_size;
    }

    /// Writes typed POD data into the buffer, growing it when needed.
    pub fn write_buffer<T: bytemuck::Pod>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        value: &[T],
        offset: Option<wgpu::BufferAddress>,
    ) {
        let offset = offset.unwrap_or(0);
        let size = std::mem::size_of_val(value) as u64;
        self.resize(device, size + offset);
        if size > 0 {
            queue.write_buffer(self.get_wgpu_buffer(), offset, bytemuck::cast_slice(value));
        }
    }

    /// Writes raw bytes into the buffer, growing it when needed.
    pub fn write_buffer_bytes(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        value: &[u8],
        offset: Option<wgpu::BufferAddress>,
    ) {
        let offset = offset.unwrap_or(0);
        let size = std::mem::size_of_val(value) as u64;
        self.resize(device, size + offset);
        if size > 0 {
            queue.write_buffer(self.get_wgpu_buffer(), offset, value);
        }
    }

    /// Returns the backing wgpu buffer.
    pub fn get_wgpu_buffer(&self) -> &wgpu::Buffer {
        self.buffer.as_ref().unwrap()
    }
}
