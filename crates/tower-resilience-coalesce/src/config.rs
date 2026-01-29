//! Configuration for the coalesce layer.

use std::marker::PhantomData;

/// Configuration for the coalesce layer.
#[derive(Debug, Clone)]
pub struct CoalesceConfig<K, F> {
    /// Function to extract a key from a request.
    pub(crate) key_extractor: F,
    /// Optional name for metrics/tracing.
    /// Only used when `metrics` or `tracing` features are enabled.
    #[cfg_attr(not(any(feature = "metrics", feature = "tracing")), allow(dead_code))]
    pub(crate) name: Option<String>,
    /// Marker for the key type.
    pub(crate) _key: PhantomData<K>,
}

impl<K, F> CoalesceConfig<K, F> {
    /// Create a new configuration with the given key extractor.
    pub fn new(key_extractor: F) -> Self {
        Self {
            key_extractor,
            name: None,
            _key: PhantomData,
        }
    }

    /// Create a builder for more configuration options.
    pub fn builder(key_extractor: F) -> CoalesceConfigBuilder<K, F> {
        CoalesceConfigBuilder::new(key_extractor)
    }
}

/// Builder for coalesce configuration.
#[derive(Debug, Clone)]
pub struct CoalesceConfigBuilder<K, F> {
    key_extractor: F,
    name: Option<String>,
    _key: PhantomData<K>,
}

impl<K, F> CoalesceConfigBuilder<K, F> {
    /// Create a new builder with the given key extractor.
    pub fn new(key_extractor: F) -> Self {
        Self {
            key_extractor,
            name: None,
            _key: PhantomData,
        }
    }

    /// Set a name for this coalesce instance (for metrics/tracing).
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_coalesce::CoalesceConfig;
    ///
    /// let config: CoalesceConfig<String, _> = CoalesceConfig::builder(|req: &String| req.clone())
    ///     .name("user-lookup")
    ///     .build();
    /// ```
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Build the configuration.
    pub fn build(self) -> CoalesceConfig<K, F> {
        CoalesceConfig {
            key_extractor: self.key_extractor,
            name: self.name,
            _key: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config: CoalesceConfig<String, _> = CoalesceConfig::builder(|req: &String| req.clone())
            .name("test")
            .build();

        assert_eq!(config.name, Some("test".to_string()));
    }

    #[test]
    fn test_config_new() {
        let config: CoalesceConfig<String, _> = CoalesceConfig::new(|req: &String| req.clone());
        assert!(config.name.is_none());
    }
}
