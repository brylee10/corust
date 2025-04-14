//! Utilities for caching the results of HTTP requests, with TTLs

use std::time::{Duration, Instant, SystemTime};

use tokio::sync::Mutex;

use crate::sandbox_metadata::SANDBOX_METADATA_TIME_TO_LIVE;

/// Holds a thread-safe item
#[derive(Debug)]
pub struct CacheItem<T> {
    inner: Mutex<Option<CachedItem<T>>>,
    ttl: Duration,
}

impl<T> Default for CacheItem<T> {
    fn default() -> Self {
        Self {
            inner: Default::default(),
            ttl: SANDBOX_METADATA_TIME_TO_LIVE,
        }
    }
}

impl<T> CacheItem<T>
where
    T: PartialEq + Clone,
{
    pub fn new(value: T, ttl: Duration) -> Self {
        Self {
            inner: Mutex::new(Some(CachedItem::new(value))),
            ttl,
        }
    }

    /// Gets the value inside the cache, optionally using the generator to regenate a new value
    /// if the prior value is not present or has expired
    pub async fn get_value<E: std::error::Error>(
        &self,
        generator: impl Future<Output = Result<T, E>>,
    ) -> Result<T, E> {
        let data = &mut *self.inner.lock().await;
        match data.as_mut() {
            Some(item) => {
                if item.last_updated.elapsed() <= self.ttl {
                    Ok(item.inner.clone())
                } else {
                    log::trace!("Setting new cached item");
                    Self::set_value(data, generator).await
                }
            }
            None => Self::set_value(data, generator).await,
        }
    }

    /// Sets the value of the cache item to the result of the generator, if it's different from the current value
    pub async fn set_value<E: std::error::Error>(
        data: &mut Option<CachedItem<T>>,
        generator: impl Future<Output = Result<T, E>>,
    ) -> Result<T, E> {
        let item = generator.await?;
        let item = CachedItem::new(item);
        let cached_item = if let Some(mut cached_item) = data.take() {
            if cached_item.inner == item.inner {
                cached_item.last_updated = Instant::now();
                cached_item
            } else {
                item
            }
        } else {
            item
        };
        let ret = cached_item.inner.clone();
        *data = Some(cached_item);
        Ok(ret)
    }
}

/// Holds one thread-safe item, with a timestamp used for TTLs
#[derive(Debug)]
pub struct CachedItem<T> {
    pub(crate) inner: T,
    pub(crate) _creation_time: SystemTime,
    pub(crate) last_updated: Instant,
}

impl<T> CachedItem<T> {
    /// Creates a new cached item, timestamped to the current time
    pub fn new(item: T) -> Self {
        Self {
            inner: item,
            _creation_time: SystemTime::now(),
            last_updated: Instant::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::time::sleep;

    use super::*;
    const TEST_TIMEOUT_SEC: u64 = 2;
    // Lower TTL to make tests run faster for tests
    const TEST_TTL: Duration = Duration::from_millis(100);

    // Times out a test after a certain number of seconds
    trait Timeout: Future + Sized {
        fn with_timeout(self) -> tokio::time::Timeout<Self> {
            tokio::time::timeout(std::time::Duration::from_secs(TEST_TIMEOUT_SEC), self)
        }
    }

    impl<T: Future + Sized> Timeout for T {}

    #[tokio::test]
    async fn test_get_initial_value() {
        let cache = CacheItem::<String>::default();

        let result = cache
            .get_value(async { Ok::<_, std::io::Error>("test_value".to_string()) })
            .with_timeout()
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().unwrap(), "test_value");
    }

    #[tokio::test]
    async fn test_get_cached_value_not_expired() {
        let cache = CacheItem::new("initial_value".to_string(), TEST_TTL);
        let starting_time = cache.inner.lock().await.as_ref().unwrap().last_updated;

        // This should return the cached value, not the new one
        let result = cache
            .get_value(async { Ok::<_, std::io::Error>("new_value".to_string()) })
            .with_timeout()
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().unwrap(), "initial_value");

        // Test that the last updated time is not updated
        let item = &mut *cache.inner.lock().await;
        let ending_time = item.as_ref().unwrap().last_updated;
        assert!(ending_time == starting_time);
    }

    #[tokio::test]
    async fn test_get_expired_value() {
        let cache = CacheItem::new("initial_value".to_string(), TEST_TTL);
        let starting_time = cache.inner.lock().await.as_ref().unwrap().last_updated;

        // Wait for the value to expire
        sleep(TEST_TTL + Duration::from_millis(TEST_TTL.as_millis() as u64 * 2)).await;

        // This should regenerate and return the new value
        let result = cache
            .get_value(async { Ok::<_, std::io::Error>("new_value".to_string()) })
            .with_timeout()
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().unwrap(), "new_value");

        // Test that the last updated time is updated
        let item = &mut *cache.inner.lock().await;
        let ending_time = item.as_ref().unwrap().last_updated;
        assert!(ending_time > starting_time);
    }
}
