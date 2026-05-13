#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ThumbnailKey {
    pub asset_id: Uuid,
    pub source_version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThumbnailRequest {
    pub asset_id: Uuid,
    pub asset_type: Uuid,
    pub source_path: Option<PathBuf>,
    pub source_version: u64,
}

impl ThumbnailRequest {
    pub fn key(&self) -> ThumbnailKey {
        ThumbnailKey {
            asset_id: self.asset_id,
            source_version: self.source_version,
        }
    }
}

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
pub struct ThumbnailImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ThumbnailStatus {
    Missing,
    Queued { priority: ThumbnailPriority },
    InProgress,
    Ready(ThumbnailImage),
    Failed { message: String },
}

#[derive(Default)]
pub struct ThumbnailPipeline {
    queued: Vec<ThumbnailJob>,
    statuses: HashMap<ThumbnailKey, ThumbnailStatus>,
}

impl ThumbnailPipeline {
    pub fn request(
        &mut self,
        request: ThumbnailRequest,
        priority: ThumbnailPriority,
    ) -> ThumbnailStatus {
        let key = request.key();
        match self.statuses.get_mut(&key) {
            Some(ThumbnailStatus::Queued { priority: existing }) => {
                if priority > *existing {
                    *existing = priority;
                    if let Some(job) = self.queued.iter_mut().find(|job| job.key() == key) {
                        job.priority = priority;
                    }
                }
            }
            Some(ThumbnailStatus::InProgress | ThumbnailStatus::Ready(_)) => {}
            Some(ThumbnailStatus::Failed { .. } | ThumbnailStatus::Missing) | None => {
                self.queued.push(ThumbnailJob {
                    request,
                    priority,
                    requested_at: Instant::now(),
                });
                self.statuses
                    .insert(key, ThumbnailStatus::Queued { priority });
            }
        }

        self.status(key)
    }

    pub fn status(&self, key: ThumbnailKey) -> ThumbnailStatus {
        self.statuses
            .get(&key)
            .cloned()
            .unwrap_or(ThumbnailStatus::Missing)
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

    pub fn complete(&mut self, key: ThumbnailKey, image: ThumbnailImage) {
        self.statuses.insert(key, ThumbnailStatus::Ready(image));
    }

    pub fn fail(&mut self, key: ThumbnailKey, message: impl Into<String>) {
        self.statuses.insert(
            key,
            ThumbnailStatus::Failed {
                message: message.into(),
            },
        );
    }

    pub fn clear(&mut self, key: ThumbnailKey) {
        self.queued.retain(|job| job.key() != key);
        self.statuses.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(source_version: u64) -> ThumbnailRequest {
        ThumbnailRequest {
            asset_id: Uuid::from_u128(1),
            asset_type: Uuid::from_u128(2),
            source_path: Some(PathBuf::from("assets/texture.png")),
            source_version,
        }
    }

    #[test]
    fn request_deduplicates_and_promotes_priority() {
        let mut pipeline = ThumbnailPipeline::default();
        let request = request(7);
        let key = request.key();

        pipeline.request(request.clone(), ThumbnailPriority::Low);
        pipeline.request(request, ThumbnailPriority::High);

        assert_eq!(pipeline.queued_len(), 1);
        assert_eq!(
            pipeline.status(key),
            ThumbnailStatus::Queued {
                priority: ThumbnailPriority::High
            }
        );
    }

    #[test]
    fn start_next_picks_highest_priority_job() {
        let mut pipeline = ThumbnailPipeline::default();
        let low = request(1);
        let high = request(2);

        pipeline.request(low.clone(), ThumbnailPriority::Low);
        pipeline.request(high.clone(), ThumbnailPriority::High);

        let job = pipeline.start_next().unwrap();

        assert_eq!(job.key(), high.key());
        assert_eq!(pipeline.status(high.key()), ThumbnailStatus::InProgress);
        assert_eq!(
            pipeline.status(low.key()),
            ThumbnailStatus::Queued {
                priority: ThumbnailPriority::Low
            }
        );
    }

    #[test]
    fn complete_and_fail_update_status() {
        let mut pipeline = ThumbnailPipeline::default();
        let request = request(1);
        let key = request.key();
        pipeline.request(request, ThumbnailPriority::Normal);
        pipeline.start_next();

        pipeline.complete(
            key,
            ThumbnailImage {
                width: 1,
                height: 1,
                rgba: vec![255, 0, 255, 255],
            },
        );
        assert!(matches!(pipeline.status(key), ThumbnailStatus::Ready(_)));

        pipeline.fail(key, "regenerate failed");
        assert_eq!(
            pipeline.status(key),
            ThumbnailStatus::Failed {
                message: "regenerate failed".into()
            }
        );
    }
}
