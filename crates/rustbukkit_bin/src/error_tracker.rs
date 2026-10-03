use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use tracing::error;

use crate::consts::{ERROR_THRESHOLD, ERROR_WINDOW_SECS};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ErrorKey {
    category:  String,
    semantics: String,
}

impl ErrorKey {
    pub fn new(category: impl Into<String>, semantics: impl Into<String>) -> Self {
        Self {
            category:  category.into(),
            semantics: semantics.into(),
        }
    }
}

#[derive(Debug, Clone)]
struct ErrorEntry {
    count:            usize,
    first_occurrence: Instant,
}

pub struct ErrorTracker {
    errors: Arc<RwLock<HashMap<ErrorKey, ErrorEntry>>>,
}

impl ErrorTracker {
    pub fn new() -> Self {
        Self {
            errors: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn record_error(&self, key: ErrorKey) -> bool {
        let mut errors = self.errors.write();
        let now = Instant::now();

        let entry = errors.entry(key.clone()).or_insert(ErrorEntry {
            count:            0,
            first_occurrence: now,
        });

        entry.count += 1;

        // Check if error occurred more than threshold times within the window
        if now.duration_since(entry.first_occurrence) < ERROR_WINDOW_SECS {
            if entry.count >= ERROR_THRESHOLD {
                error!(
                    "[{}] Error threshold exceeded: {} occurrences of '{}' in {:?}",
                    key.category,
                    entry.count,
                    key.semantics,
                    now.duration_since(entry.first_occurrence)
                );
                return true; // Trigger shutdown
            }
        } else {
            // Reset if outside the window
            entry.count = 1;
            entry.first_occurrence = now;
        }

        false
    }

    pub fn clear(&self) {
        self.errors.write().clear();
    }

    pub fn get_stats(&self) -> HashMap<ErrorKey, (usize, Duration)> {
        let errors = self.errors.read();
        let now = Instant::now();

        errors
            .iter()
            .map(|(key, entry)| {
                let elapsed = now.duration_since(entry.first_occurrence);
                (key.clone(), (entry.count, elapsed))
            })
            .collect()
    }
}

impl Clone for ErrorTracker {
    fn clone(&self) -> Self {
        Self {
            errors: self.errors.clone(),
        }
    }
}

impl Default for ErrorTracker {
    fn default() -> Self {
        Self::new()
    }
}
