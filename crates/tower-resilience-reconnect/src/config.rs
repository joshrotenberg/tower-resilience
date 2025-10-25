use crate::policy::ReconnectPolicy;

#[cfg(feature = "tracing")]
use std::sync::Arc;

/// Configuration for reconnection behavior.
pub struct ReconnectConfig {
    /// The reconnection policy determining backoff strategy.
    pub(crate) policy: ReconnectPolicy,

    /// Maximum number of reconnection attempts before giving up.
    /// None means unlimited attempts.
    pub(crate) max_attempts: Option<u32>,

    /// Whether to retry the original command after successful reconnection.
    pub(crate) retry_on_reconnect: bool,

    /// Optional callback for reconnection events.
    #[cfg(feature = "tracing")]
    pub(crate) on_reconnect: Option<Arc<dyn Fn(u32) + Send + Sync>>,
}

impl Clone for ReconnectConfig {
    fn clone(&self) -> Self {
        Self {
            policy: self.policy.clone(),
            max_attempts: self.max_attempts,
            retry_on_reconnect: self.retry_on_reconnect,
            #[cfg(feature = "tracing")]
            on_reconnect: self.on_reconnect.clone(),
        }
    }
}

impl std::fmt::Debug for ReconnectConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug_struct = f.debug_struct("ReconnectConfig");
        debug_struct
            .field("policy", &self.policy)
            .field("max_attempts", &self.max_attempts)
            .field("retry_on_reconnect", &self.retry_on_reconnect);

        #[cfg(feature = "tracing")]
        debug_struct.field("on_reconnect", &self.on_reconnect.is_some());

        debug_struct.finish()
    }
}

impl ReconnectConfig {
    /// Creates a new builder for configuring reconnection behavior.
    pub fn builder() -> ReconnectConfigBuilder {
        ReconnectConfigBuilder::default()
    }

    /// Returns the reconnection policy.
    pub fn policy(&self) -> &ReconnectPolicy {
        &self.policy
    }

    /// Returns the maximum number of reconnection attempts.
    pub fn max_attempts(&self) -> Option<u32> {
        self.max_attempts
    }

    /// Returns whether commands are retried after reconnection.
    pub fn retry_on_reconnect(&self) -> bool {
        self.retry_on_reconnect
    }
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            policy: ReconnectPolicy::default(),
            max_attempts: None,
            retry_on_reconnect: true,
            #[cfg(feature = "tracing")]
            on_reconnect: None,
        }
    }
}

/// Builder for constructing a `ReconnectConfig`.
pub struct ReconnectConfigBuilder {
    policy: ReconnectPolicy,
    max_attempts: Option<u32>,
    retry_on_reconnect: bool,
    #[cfg(feature = "tracing")]
    on_reconnect: Option<Arc<dyn Fn(u32) + Send + Sync>>,
}

impl std::fmt::Debug for ReconnectConfigBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug_struct = f.debug_struct("ReconnectConfigBuilder");
        debug_struct
            .field("policy", &self.policy)
            .field("max_attempts", &self.max_attempts)
            .field("retry_on_reconnect", &self.retry_on_reconnect);

        #[cfg(feature = "tracing")]
        debug_struct.field("on_reconnect", &self.on_reconnect.is_some());

        debug_struct.finish()
    }
}

impl ReconnectConfigBuilder {
    /// Creates a new builder with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the reconnection policy.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use tower_resilience_reconnect::{ReconnectConfig, ReconnectPolicy};
    ///
    /// let config = ReconnectConfig::builder()
    ///     .policy(ReconnectPolicy::exponential(
    ///         Duration::from_millis(100),
    ///         Duration::from_secs(10),
    ///     ))
    ///     .build();
    /// ```
    pub fn policy(mut self, policy: ReconnectPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Sets the maximum number of reconnection attempts.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_reconnect::ReconnectConfig;
    ///
    /// let config = ReconnectConfig::builder()
    ///     .max_attempts(5)
    ///     .build();
    /// ```
    pub fn max_attempts(mut self, max_attempts: u32) -> Self {
        self.max_attempts = Some(max_attempts);
        self
    }

    /// Sets unlimited reconnection attempts.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_reconnect::ReconnectConfig;
    ///
    /// let config = ReconnectConfig::builder()
    ///     .unlimited_attempts()
    ///     .build();
    /// ```
    pub fn unlimited_attempts(mut self) -> Self {
        self.max_attempts = None;
        self
    }

    /// Sets whether to retry the original command after successful reconnection.
    ///
    /// Default is `true`.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_reconnect::ReconnectConfig;
    ///
    /// let config = ReconnectConfig::builder()
    ///     .retry_on_reconnect(false)
    ///     .build();
    /// ```
    pub fn retry_on_reconnect(mut self, retry: bool) -> Self {
        self.retry_on_reconnect = retry;
        self
    }

    /// Sets a callback to be invoked on each reconnection attempt.
    ///
    /// The callback receives the current attempt number.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_reconnect::ReconnectConfig;
    ///
    /// let config = ReconnectConfig::builder()
    ///     .on_reconnect(|attempt| {
    ///         println!("Reconnection attempt: {}", attempt);
    ///     })
    ///     .build();
    /// ```
    #[cfg(feature = "tracing")]
    pub fn on_reconnect<F>(mut self, callback: F) -> Self
    where
        F: Fn(u32) + Send + Sync + 'static,
    {
        self.on_reconnect = Some(Arc::new(callback));
        self
    }

    /// Builds the `ReconnectConfig`.
    pub fn build(self) -> ReconnectConfig {
        ReconnectConfig {
            policy: self.policy,
            max_attempts: self.max_attempts,
            retry_on_reconnect: self.retry_on_reconnect,
            #[cfg(feature = "tracing")]
            on_reconnect: self.on_reconnect,
        }
    }
}

impl Default for ReconnectConfigBuilder {
    fn default() -> Self {
        Self {
            policy: ReconnectPolicy::default(),
            max_attempts: None,
            retry_on_reconnect: true,
            #[cfg(feature = "tracing")]
            on_reconnect: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_default_config() {
        let config = ReconnectConfig::default();
        assert!(config.retry_on_reconnect());
        assert_eq!(config.max_attempts(), None);
    }

    #[test]
    fn test_builder_default() {
        let config = ReconnectConfig::builder().build();
        assert!(config.retry_on_reconnect());
        assert_eq!(config.max_attempts(), None);
    }

    #[test]
    fn test_builder_max_attempts() {
        let config = ReconnectConfig::builder().max_attempts(5).build();
        assert_eq!(config.max_attempts(), Some(5));
    }

    #[test]
    fn test_builder_unlimited_attempts() {
        let config = ReconnectConfig::builder()
            .max_attempts(5)
            .unlimited_attempts()
            .build();
        assert_eq!(config.max_attempts(), None);
    }

    #[test]
    fn test_builder_retry_on_reconnect() {
        let config = ReconnectConfig::builder().retry_on_reconnect(false).build();
        assert!(!config.retry_on_reconnect());
    }

    #[test]
    fn test_builder_policy() {
        let policy =
            ReconnectPolicy::exponential(Duration::from_millis(200), Duration::from_secs(20));
        let config = ReconnectConfig::builder().policy(policy).build();

        match config.policy() {
            ReconnectPolicy::Exponential(_) => {}
            _ => panic!("Expected exponential policy"),
        }
    }
}
