//! Utilities for caching the results of HTTP requests, with TTLs

use std::time::{Instant, SystemTime};

use tokio::sync::Mutex;

use crate::sandbox_metadata::SANDBOX_METADATA_TIME_TO_LIVE;

/// Holds a thread-safe item
#[derive(Debug)]
pub struct CacheItem<T>(Mutex<Option<CachedItem<T>>>);

impl<T> Default for CacheItem<T> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<T> CacheItem<T>
where
    T: PartialEq + Clone,
{
    pub fn new(value: T) -> Self {
        Self(Mutex::new(Some(CachedItem::new(value))))
    }

    /// Gets the value inside the cache, optionally using the generator to regenate a new value
    /// if the prior value is not present or has expired
    pub async fn get_value(&self, generator: impl Future<Output = T>) -> T {
        let mut item = self.0.lock().await;
        match item.as_mut() {
            Some(item) => {
                if item.last_updated.elapsed() <= SANDBOX_METADATA_TIME_TO_LIVE {
                    item.inner.clone()
                } else {
                    self.set_value(generator).await
                }
            }
            None => self.set_value(generator).await,
        }
    }

    /// Sets the value of the cache item to the result of the generator, if it's different from the current value
    pub async fn set_value(&self, generator: impl Future<Output = T>) -> T {
        let item = generator.await;
        let item = CachedItem::new(item);
        let mut cached_item = self.0.lock().await;
        let cached_item = if let Some(mut cached_item) = cached_item.take() {
            if cached_item.inner == item.inner {
                cached_item.last_updated = Instant::now();
                cached_item
            } else {
                cached_item
            }
        } else {
            item
        };
        let ret = cached_item.inner.clone();
        *self.0.lock().await = Some(cached_item);
        ret
    }
}

/// Holds one thread-safe item, with a timestamp used for TTLs
#[derive(Debug)]
pub struct CachedItem<T> {
    pub(crate) inner: T,
    pub(crate) creation_time: SystemTime,
    pub(crate) last_updated: Instant,
}

impl<T> CachedItem<T> {
    /// Creates a new cached item, timestamped to the current time
    pub fn new(item: T) -> Self {
        Self {
            inner: item,
            creation_time: SystemTime::now(),
            last_updated: Instant::now(),
        }
    }
}
