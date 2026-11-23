//! Generation tokens for cancellation-safe asynchronous work.
//!
//! A backend must check a token immediately before publishing a newly-created
//! process/stream/worker. Cancelling increments the generation even when no
//! process is registered yet, so Stop defeats work that is still being
//! scheduled, decoding or opening an output device.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationKind {
    Audio,
    Script,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OperationKey {
    pub kind: OperationKind,
    pub key: String,
}

impl OperationKey {
    pub fn new(kind: OperationKind, key: impl Into<String>) -> Self {
        Self {
            kind,
            key: key.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationToken {
    pub key: OperationKey,
    pub generation: u64,
}

/// A monotonic cancellation registry. This type is intentionally synchronous;
/// the application actor owns it and hands tokens to worker tasks.
#[derive(Debug, Default)]
pub struct GenerationTracker {
    generations: HashMap<OperationKey, u64>,
}

impl GenerationTracker {
    /// Begin a new operation, invalidating any earlier operation with this key.
    pub fn begin(&mut self, key: OperationKey) -> OperationToken {
        let generation = self.generations.entry(key.clone()).or_insert(0);
        *generation = generation.saturating_add(1);
        let current = *generation;
        OperationToken {
            key,
            generation: current,
        }
    }

    /// Invalidate a key even if its worker has not been started or registered.
    pub fn cancel(&mut self, key: OperationKey) -> OperationToken {
        let generation = self.generations.entry(key.clone()).or_insert(0);
        *generation = generation.saturating_add(1);
        OperationToken {
            key,
            generation: *generation,
        }
    }

    /// Invalidate every previously known operation in one domain.
    pub fn cancel_kind(&mut self, kind: OperationKind) -> Vec<OperationToken> {
        let keys: Vec<OperationKey> = self
            .generations
            .keys()
            .filter(|key| key.kind == kind)
            .cloned()
            .collect();

        keys.into_iter().map(|key| self.cancel(key)).collect()
    }

    /// Invalidate all currently known operations.
    pub fn cancel_all(&mut self) -> Vec<OperationToken> {
        let keys: Vec<OperationKey> = self.generations.keys().cloned().collect();
        keys.into_iter().map(|key| self.cancel(key)).collect()
    }

    pub fn is_current(&self, token: &OperationToken) -> bool {
        self.generations.get(&token.key).copied() == Some(token.generation)
    }

    pub fn generation(&self, key: &OperationKey) -> u64 {
        self.generations.get(key).copied().unwrap_or(0)
    }

    pub fn known_operations(&self) -> usize {
        self.generations.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beginning_new_work_invalidates_previous_work() {
        let mut tracker = GenerationTracker::default();
        let key = OperationKey::new(OperationKind::Audio, "0:17");
        let first = tracker.begin(key.clone());
        let second = tracker.begin(key);

        assert!(!tracker.is_current(&first));
        assert!(tracker.is_current(&second));
        assert!(second.generation > first.generation);
    }

    #[test]
    fn stop_invalidates_work_before_it_is_registered() {
        let mut tracker = GenerationTracker::default();
        let key = OperationKey::new(OperationKind::Audio, "0:17");
        let pending = tracker.begin(key.clone());
        let _stop = tracker.cancel(key);

        assert!(!tracker.is_current(&pending));
    }

    #[test]
    fn stop_all_invalidates_every_domain_but_does_not_cross_reuse_tokens() {
        let mut tracker = GenerationTracker::default();
        let audio = tracker.begin(OperationKey::new(OperationKind::Audio, "0:1"));
        let script = tracker.begin(OperationKey::new(OperationKind::Script, "0:2"));
        let unrelated = tracker.begin(OperationKey::new(OperationKind::Audio, "1:9"));

        let cancelled = tracker.cancel_all();
        assert_eq!(cancelled.len(), 3);
        assert!(!tracker.is_current(&audio));
        assert!(!tracker.is_current(&script));
        assert!(!tracker.is_current(&unrelated));
    }

    #[test]
    fn audio_and_script_generations_are_independent() {
        let mut tracker = GenerationTracker::default();
        let audio = tracker.begin(OperationKey::new(OperationKind::Audio, "0:1"));
        let script = tracker.begin(OperationKey::new(OperationKind::Script, "0:1"));

        tracker.cancel_kind(OperationKind::Audio);
        assert!(!tracker.is_current(&audio));
        assert!(tracker.is_current(&script));
    }
}
