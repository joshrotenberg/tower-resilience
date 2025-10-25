use crate::policy::ReconnectPolicy;
use std::sync::Arc;

/// Determines whether an error should trigger reconnection.
///
/// By default, all errors trigger reconnection. Use this predicate to distinguish
/// connection-level errors (BrokenPipe, ConnectionReset) from application-level errors.
pub type ReconnectPredicate = Arc<dyn Fn(&dyn std::error::Error) -> bool + Send + Sync>;

/// Configuration for reconnection behavior.
pub struct ReconnectConfig {
    /// The reconnection policy determining backoff strategy.
    pub(crate) policy: ReconnectPolicy,

    /// Maximum number of reconnection attempts before giving up.
    /// None means unlimited attempts.
    pub(crate) max_attempts: Option<u32>,

    /// Whether to retry the original command after successful reconnection.
    pub(crate) retry_on_reconnect: bool,

    /// Predicate to determine which errors should trigger reconnection.
    /// None means all errors trigger reconnection (default).
    pub(crate) reconnect_predicate: Option<ReconnectPredicate>,

    /// Optional callback for reconnection events.
    #[cfg(feature = "tracing")]
    pub(crate) on_reconnect: Option<Arc<dyn Fn(u32) + Send + Sync>>,

    /// Optional callback for state transitions.
    #[cfg(feature = "tracing")]
    pub(crate) on_state_change: Option<
        Arc<dyn Fn(crate::state::ConnectionState, crate::state::ConnectionState) + Send + Sync>,
    >,
}

impl Clone for ReconnectConfig {
    fn clone(&self) -> Self {
        Self {
            policy: self.policy.clone(),
            max_attempts: self.max_attempts,
            retry_on_reconnect: self.retry_on_reconnect,
            reconnect_predicate: self.reconnect_predicate.clone(),
            #[cfg(feature = "tracing")]
            on_reconnect: self.on_reconnect.clone(),
            #[cfg(feature = "tracing")]
            on_state_change: self.on_state_change.clone(),
        }
    }
}

impl std::fmt::Debug for ReconnectConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug_struct = f.debug_struct("ReconnectConfig");
        debug_struct
            .field("policy", &self.policy)
            .field("max_attempts", &self.max_attempts)
            .field("retry_on_reconnect", &self.retry_on_reconnect)
            .field("reconnect_predicate", &self.reconnect_predicate.is_some());

        #[cfg(feature = "tracing")]
        debug_struct.field("on_reconnect", &self.on_reconnect.is_some());

        #[cfg(feature = "tracing")]
        debug_struct.field("on_state_change", &self.on_state_change.is_some());

        #[cfg(feature = "tracing")]
        debug_struct.field("on_state_change", &self.on_state_change.is_some());

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

    /// Checks if the given error should trigger reconnection.
    ///
    /// Returns `true` if the error should trigger reconnection, `false` otherwise.
    /// If no predicate is configured, all errors trigger reconnection.
    pub fn should_reconnect(&self, error: &dyn std::error::Error) -> bool {
        if let Some(predicate) = &self.reconnect_predicate {
            predicate(error)
        } else {
            true // Reconnect on all errors by default
        }
    }
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            policy: ReconnectPolicy::default(),
            max_attempts: None,
            retry_on_reconnect: true,
            reconnect_predicate: None,
            #[cfg(feature = "tracing")]
            on_reconnect: None,
            #[cfg(feature = "tracing")]
            on_state_change: None,
        }
    }
}

/// Builder for constructing a `ReconnectConfig`.
pub struct ReconnectConfigBuilder {
    policy: ReconnectPolicy,
    max_attempts: Option<u32>,
    retry_on_reconnect: bool,
    reconnect_predicate: Option<ReconnectPredicate>,
    #[cfg(feature = "tracing")]
    on_reconnect: Option<Arc<dyn Fn(u32) + Send + Sync>>,
    #[cfg(feature = "tracing")]
    on_state_change: Option<
        Arc<dyn Fn(crate::state::ConnectionState, crate::state::ConnectionState) + Send + Sync>,
    >,
}

impl std::fmt::Debug for ReconnectConfigBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug_struct = f.debug_struct("ReconnectConfigBuilder");
        debug_struct
            .field("policy", &self.policy)
            .field("max_attempts", &self.max_attempts)
            .field("retry_on_reconnect", &self.retry_on_reconnect)
            .field("reconnect_predicate", &self.reconnect_predicate.is_some());

        #[cfg(feature = "tracing")]
        debug_struct.field("on_reconnect", &self.on_reconnect.is_some());

        #[cfg(feature = "tracing")]
        debug_struct.field("on_state_change", &self.on_state_change.is_some());

        #[cfg(feature = "tracing")]
        debug_struct.field("on_state_change", &self.on_state_change.is_some());

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

    /// Sets a predicate to determine which errors should trigger reconnection.
    ///
    /// By default, all errors trigger reconnection. Use this to distinguish
    /// connection-level errors (BrokenPipe, ConnectionReset) from application-level
    /// errors (RateLimited, Timeout) that should be handled by a retry layer.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_reconnect::ReconnectConfig;
    ///
    /// let config = ReconnectConfig::builder()
    ///     .reconnect_predicate(|error| {
    ///         // Only reconnect on connection-level errors
    ///         // Use string matching to identify error types
    ///         let error_str = error.to_string().to_lowercase();
    ///         error_str.contains("broken pipe") ||
    ///         error_str.contains("connection reset") ||
    ///         error_str.contains("connection aborted")
    ///     })
    ///     .build();
    /// ```
    pub fn reconnect_predicate<F>(mut self, predicate: F) -> Self
    where
        F: Fn(&dyn std::error::Error) -> bool + Send + Sync + 'static,
    {
        self.reconnect_predicate = Some(Arc::new(predicate));
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

    /// Sets a callback to be invoked on state transitions.
    ///
    /// The callback receives the old and new connection states.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_reconnect::{ReconnectConfig, ConnectionState};
    ///
    /// let config = ReconnectConfig::builder()
    ///     .on_state_change(|from, to| {
    ///         println!("State changed: {:?} -> {:?}", from, to);
    ///     })
    ///     .build();
    /// ```
    #[cfg(feature = "tracing")]
    pub fn on_state_change<F>(mut self, callback: F) -> Self
    where
        F: Fn(crate::state::ConnectionState, crate::state::ConnectionState) + Send + Sync + 'static,
    {
        self.on_state_change = Some(Arc::new(callback));
        self
    }

    /// Creates a predicate that only triggers reconnection) on common connection-level errors.
    ///
    /// This is a convenience helper that recognizes standard connection errors like:
    /// - BrokenPipe
    /// - ConnectionReset
    /// - ConnectionAborted
    /// - NotConnected
    ///
    /// For other error types, it falls back to string matching on error messages.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_reconnect::ReconnectConfig;
    ///
    /// let config = ReconnectConfig::builder()
    ///     .connection_errors_only()
    ///     .build();
    /// ```
    pub fn connection_errors_only(mut self) -> Self {
        self.reconnect_predicate = Some(Arc::new(|error| {
            // Use string matching to identify connection errors
            let err_str = error.to_string().to_lowercase();
            err_str.contains("broken pipe")
                || err_str.contains("connection reset")
                || err_str.contains("connection aborted")
                || err_str.contains("not connected")
                || err_str.contains("connection refused")
        }));
        self
    }

    /// Builds the `ReconnectConfig`.
    pub fn build(self) -> ReconnectConfig {
        ReconnectConfig {
            policy: self.policy,
            max_attempts: self.max_attempts,
            retry_on_reconnect: self.retry_on_reconnect,
            reconnect_predicate: self.reconnect_predicate,
            #[cfg(feature = "tracing")]
            on_reconnect: self.on_reconnect,
            #[cfg(feature = "tracing")]
            on_state_change: self.on_state_change,
        }
    }
}

impl Default for ReconnectConfigBuilder {
    fn default() -> Self {
        Self {
            policy: ReconnectPolicy::default(),
            max_attempts: None,
            retry_on_reconnect: true,
            reconnect_predicate: None,
            #[cfg(feature = "tracing")]
            on_reconnect: None,
            #[cfg(feature = "tracing")]
            on_state_change: None,
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

    #[test]
    fn test_should_reconnect_default() {
        use std::io::{Error, ErrorKind};

        let config = ReconnectConfig::default();

        // Without a predicate, all errors should trigger reconnection
        let error = Error::new(ErrorKind::BrokenPipe, "test");
        assert!(config.should_reconnect(&error));

        let error = Error::new(ErrorKind::Other, "test");
        assert!(config.should_reconnect(&error));
    }

    #[test]
    fn test_reconnect_predicate() {
        use std::io::{Error, ErrorKind};

        let config = ReconnectConfig::builder()
            .reconnect_predicate(|error| {
                // Use string matching to avoid lifetime issues with downcast_ref
                let error_str = error.to_string().to_lowercase();
                error_str.contains("broken pipe")
                    || error_str.contains("connection reset")
                    || error_str.contains("connection aborted")
            })
            .build();

        // Connection errors should trigger reconnection
        assert!(config.should_reconnect(&Error::new(ErrorKind::BrokenPipe, "broken pipe")));
        assert!(
            config.should_reconnect(&Error::new(ErrorKind::ConnectionReset, "connection reset"))
        );
        assert!(config.should_reconnect(&Error::new(
            ErrorKind::ConnectionAborted,
            "connection aborted"
        )));

        // Other errors should NOT trigger reconnection
        assert!(!config.should_reconnect(&Error::new(ErrorKind::Other, "other error")));
        assert!(!config.should_reconnect(&Error::new(ErrorKind::TimedOut, "timed out")));
        assert!(!config.should_reconnect(&Error::new(
            ErrorKind::PermissionDenied,
            "permission denied"
        )));
    }
}
