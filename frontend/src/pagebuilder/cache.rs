use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

use anyhow::Error;
use common::timestring::TimeString;
use ulid::Ulid;

use crate::builder::render::CompiledTemplate;

/// Cached compiled template with timestamp for staleness detection
struct CacheEntry {
    template: CompiledTemplate,
    updated: TimeString,
}

/// Global template cache
static TEMPLATE_CACHE: LazyLock<RwLock<HashMap<Ulid, CacheEntry>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Compile a template with caching
///
/// If the template is already cached and the timestamp matches, returns the cached version.
/// Otherwise, compiles the template and stores it in the cache.
pub fn compile_cached(
    page_id: Ulid,
    source: &str,
    updated: &TimeString,
) -> Result<CompiledTemplate, Error> {
    // Check cache first (read lock)
    {
        let cache = TEMPLATE_CACHE.read().unwrap();
        if let Some(entry) = cache.get(&page_id) {
            if entry.updated == *updated {
                return Ok(entry.template.clone());
            }
        }
    }

    // Compile template
    let template = CompiledTemplate::compile(source)?;

    // Store in cache (write lock)
    {
        let mut cache = TEMPLATE_CACHE.write().unwrap();
        cache.insert(
            page_id,
            CacheEntry {
                template: template.clone(),
                updated: updated.clone(),
            },
        );
    }

    Ok(template)
}

/// Invalidate a cached template
pub fn invalidate(page_id: &Ulid) {
    let mut cache = TEMPLATE_CACHE.write().unwrap();
    cache.remove(page_id);
}

/// Clear the entire cache
pub fn clear_all() {
    let mut cache = TEMPLATE_CACHE.write().unwrap();
    cache.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_hit() {
        let page_id = Ulid::new();
        let source = "Hello {$= name $}!";
        let updated = TimeString::now();

        // First compile - cache miss
        let _template1 = compile_cached(page_id, source, &updated).unwrap();

        // Second compile - should hit cache
        let _template2 = compile_cached(page_id, source, &updated).unwrap();

        // Cache should have an entry
        let cache = TEMPLATE_CACHE.read().unwrap();
        assert!(cache.contains_key(&page_id));
    }

    #[test]
    fn test_cache_invalidation_on_timestamp() {
        let page_id = Ulid::new();
        let source = "Hello {$= name $}!";
        let updated1 = TimeString::now();

        // First compile
        let _template1 = compile_cached(page_id, source, &updated1).unwrap();

        // Wait a moment and create a new timestamp
        std::thread::sleep(std::time::Duration::from_millis(10));
        let updated2 = TimeString::now();

        // Compile with new timestamp - should recompile
        let _template2 = compile_cached(page_id, source, &updated2).unwrap();

        // Cache entry should have the new timestamp
        let cache = TEMPLATE_CACHE.read().unwrap();
        assert_eq!(cache.get(&page_id).unwrap().updated, updated2);
    }

    #[test]
    fn test_explicit_invalidation() {
        let page_id = Ulid::new();
        let source = "Test";
        let updated = TimeString::now();

        // Compile and cache
        let _template = compile_cached(page_id, source, &updated).unwrap();

        // Verify it's cached
        {
            let cache = TEMPLATE_CACHE.read().unwrap();
            assert!(cache.contains_key(&page_id));
        }

        // Invalidate
        invalidate(&page_id);

        // Verify it's removed
        {
            let cache = TEMPLATE_CACHE.read().unwrap();
            assert!(!cache.contains_key(&page_id));
        }
    }
}
