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

    /// Returns the cached group when it was built for `key`, else builds it
    /// with `create`, which may fail: then nothing is cached and `None` is
    /// returned, and the next call tries again.
    pub fn get_or_try_create(
        &mut self,
        key: K,
        create: impl FnOnce() -> Option<wgpu::BindGroup>,
    ) -> Option<&wgpu::BindGroup> {
        if self
            .cached
            .as_ref()
            .is_none_or(|(cached, _)| *cached != key)
        {
            self.cached = create().map(|group| (key, group));
        }
        self.cached.as_ref().map(|(_, group)| group)
    }

    /// The cached group, whatever key built it.
    pub fn get(&self) -> Option<&wgpu::BindGroup> {
        self.cached.as_ref().map(|(_, group)| group)
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
mod tests;
