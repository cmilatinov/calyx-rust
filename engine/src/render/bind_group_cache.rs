use egui_wgpu::wgpu;

/// One bind group reused across frames while the resources it binds stay the
/// same.
///
/// The key holds the bound resources themselves (wgpu handles compare by
/// identity) and the layout, so a recreated buffer or texture, or a shader
/// hot reload that rebuilds the layout, makes a new group.
pub(crate) struct CachedBindGroup<K> {
    cached: Option<(K, wgpu::BindGroup)>,
}

impl<K> Default for CachedBindGroup<K> {
    fn default() -> Self {
        Self { cached: None }
    }
}

impl<K: PartialEq> CachedBindGroup<K> {
    /// Returns the cached group when it was built for `key`, else builds it
    /// with `create`.
    pub fn get_or_create(
        &mut self,
        key: K,
        create: impl FnOnce() -> wgpu::BindGroup,
    ) -> &wgpu::BindGroup {
        if self
            .cached
            .as_ref()
            .is_none_or(|(cached, _)| *cached != key)
        {
            self.cached = Some((key, create()));
        }
        &self.cached.as_ref().expect("the cache was just filled").1
    }

    /// Whether a group is cached.
    #[cfg(test)]
    pub fn is_cached(&self) -> bool {
        self.cached.is_some()
    }

    /// Drops the cached group and the resources its key holds.
    pub fn clear(&mut self) {
        self.cached = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_render_context;

    fn uniform_buffer(device: &wgpu::Device) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: false,
        })
    }

    fn bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        buffer: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        })
    }

    #[test]
    fn rebuilds_only_when_a_bound_resource_changes() {
        let render_context = test_render_context();
        let device = render_context.device();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let first_buffer = uniform_buffer(device);
        let second_buffer = uniform_buffer(device);
        let mut cache = CachedBindGroup::default();
        let builds = std::cell::Cell::new(0);
        let get = |cache: &mut CachedBindGroup<_>, buffer: &wgpu::Buffer| {
            cache
                .get_or_create((layout.clone(), buffer.clone()), || {
                    builds.set(builds.get() + 1);
                    bind_group(device, &layout, buffer)
                })
                .clone()
        };

        let first = get(&mut cache, &first_buffer);
        assert_eq!(get(&mut cache, &first_buffer), first);
        let second = get(&mut cache, &second_buffer);
        assert_ne!(second, first);
        cache.clear();
        assert_ne!(get(&mut cache, &second_buffer), second);
        assert_eq!(builds.get(), 3);
    }
}
