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

pub struct ResizableBuffer {
    usage: wgpu::BufferUsages,
    size: u64,
    buffer: Option<wgpu::Buffer>
}

impl ResizableBuffer {
    pub fn new(usage: wgpu::BufferUsages) -> Self {
        Self {
            usage,
            size: 0,
            buffer: None
        }
    }

    fn resize(&mut self, device: &wgpu::Device, size: u64) {
        if self.buffer.is_some() && self.size >= size {
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
    }

    pub fn write_buffer<T: bytemuck::Pod>(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, value: &[T]) {
        self.resize(device, std::mem::size_of_val(value) as u64);
        queue.write_buffer(self.buffer.as_ref().unwrap(), 0, bytemuck::cast_slice(value));
    }

    pub fn get_wgpu_buffer(&self) -> &wgpu::Buffer {
        self.buffer.as_ref().unwrap()
    }
}