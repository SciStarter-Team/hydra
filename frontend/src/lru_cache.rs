use std::time::{Duration, Instant};

#[derive(Clone)]
struct CacheEntry<T> {
    key: String,
    value: T,
    expires_at: Instant,
}

pub struct LruCache<T> {
    entries: Vec<CacheEntry<T>>,
    max_size: usize,
    ttl: Duration,
}

impl<T: Clone> LruCache<T> {
    pub fn new(max_size: usize, ttl: Duration) -> Self {
        Self {
            entries: Vec::new(),
            max_size,
            ttl,
        }
    }

    pub fn get(&mut self, key: &str) -> Option<T> {
        if let Some(pos) = self.entries.iter().position(|e| e.key == key) {
            let entry = &mut self.entries[pos];

            // Check if expired
            if entry.expires_at < Instant::now() {
                self.entries.swap_remove(pos);
                return None;
            }

            // Update expiration time on cache hit
            entry.expires_at = Instant::now() + self.ttl;

            Some(entry.value.clone())
        } else {
            None
        }
    }

    pub fn insert(&mut self, key: String, value: T) {
        // Check if entry already exists
        if let Some(pos) = self.entries.iter().position(|e| e.key == key) {
            // Update existing entry
            self.entries[pos].value = value;
            self.entries[pos].expires_at = Instant::now() + self.ttl;
            return;
        }

        // If at capacity, remove entry with earliest expiration (least recently used)
        if self.entries.len() >= self.max_size {
            if let Some(min_pos) = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.expires_at)
                .map(|(pos, _)| pos)
            {
                self.entries.swap_remove(min_pos);
            }
        }

        self.entries.push(CacheEntry {
            key,
            value,
            expires_at: Instant::now() + self.ttl,
        });
    }

    /// Remove all expired entries from the cache
    pub fn clean(&mut self) {
        let now = Instant::now();
        self.entries.retain(|entry| entry.expires_at >= now);
    }
}
