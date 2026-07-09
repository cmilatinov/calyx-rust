use super::*;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ThumbnailPriority {
    Low,
    Normal,
    High,
}

impl Default for ThumbnailPriority {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThumbnailJob {
    pub request: ThumbnailRequest,
    pub priority: ThumbnailPriority,
    pub requested_at: Instant,
}

impl ThumbnailJob {
    pub fn key(&self) -> ThumbnailKey {
        self.request.key()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ThumbnailStatus {
    Missing,
    Queued { priority: ThumbnailPriority },
    InProgress,
    Ready,
    Failed { message: String },
}

#[derive(Default)]
pub struct ThumbnailPipeline {
    queued: Vec<ThumbnailJob>,
    statuses: HashMap<ThumbnailKey, ThumbnailStatus>,
    failure_counts: HashMap<ThumbnailKey, u8>,
}

impl ThumbnailPipeline {
    pub fn request(
        &mut self,
        request: ThumbnailRequest,
        priority: ThumbnailPriority,
    ) -> ThumbnailStatus {
        let key = request.key();
        match self.statuses.get(&key).cloned() {
            Some(ThumbnailStatus::Queued { priority: existing }) => {
                if priority > existing {
                    if let Some(ThumbnailStatus::Queued { priority: existing }) =
                        self.statuses.get_mut(&key)
                    {
                        *existing = priority;
                    }
                    if let Some(job) = self.queued.iter_mut().find(|job| job.key() == key) {
                        job.priority = priority;
                    }
                }
            }
            Some(ThumbnailStatus::InProgress | ThumbnailStatus::Ready) => {}
            Some(ThumbnailStatus::Failed { .. })
                if self.failure_count(key) < THUMBNAIL_MAX_FAILURES =>
            {
                self.enqueue(request, priority);
            }
            Some(ThumbnailStatus::Failed { .. }) => {}
            Some(ThumbnailStatus::Missing) | None => {
                self.enqueue(request, priority);
            }
        }

        self.status(key)
    }

    fn enqueue(&mut self, request: ThumbnailRequest, priority: ThumbnailPriority) {
        let key = request.key();
        self.queued.push(ThumbnailJob {
            request,
            priority,
            requested_at: Instant::now(),
        });
        self.statuses
            .insert(key, ThumbnailStatus::Queued { priority });
    }

    pub fn status(&self, key: ThumbnailKey) -> ThumbnailStatus {
        self.statuses
            .get(&key)
            .cloned()
            .unwrap_or(ThumbnailStatus::Missing)
    }

    pub(super) fn failure_count(&self, key: ThumbnailKey) -> u8 {
        self.failure_counts.get(&key).copied().unwrap_or_default()
    }

    pub fn queued_len(&self) -> usize {
        self.queued.len()
    }

    pub fn start_next(&mut self) -> Option<ThumbnailJob> {
        let index = self
            .queued
            .iter()
            .enumerate()
            .max_by_key(|(_, job)| (job.priority, std::cmp::Reverse(job.requested_at)))
            .map(|(index, _)| index)?;
        let job = self.queued.remove(index);
        self.statuses.insert(job.key(), ThumbnailStatus::InProgress);
        Some(job)
    }

    pub fn complete(&mut self, key: ThumbnailKey) -> bool {
        if !matches!(self.statuses.get(&key), Some(ThumbnailStatus::InProgress)) {
            return false;
        }
        self.failure_counts.remove(&key);
        self.statuses.insert(key, ThumbnailStatus::Ready);
        true
    }

    pub fn fail(&mut self, key: ThumbnailKey, message: impl Into<String>) -> bool {
        if !matches!(self.statuses.get(&key), Some(ThumbnailStatus::InProgress)) {
            return false;
        }
        let count = self.failure_counts.entry(key).or_default();
        *count = count.saturating_add(1);
        self.statuses.insert(
            key,
            ThumbnailStatus::Failed {
                message: message.into(),
            },
        );
        true
    }

    pub fn clear(&mut self, key: ThumbnailKey) {
        self.queued.retain(|job| job.key() != key);
        self.statuses.remove(&key);
        self.failure_counts.remove(&key);
    }

    pub fn clear_asset_versions_except(&mut self, asset_id: Uuid, source_version: u64) {
        let stale_keys = self
            .statuses
            .keys()
            .copied()
            .filter(|key| key.asset_id == asset_id && key.source_version != source_version)
            .collect::<Vec<_>>();
        for key in stale_keys {
            self.clear(key);
        }
    }
}
