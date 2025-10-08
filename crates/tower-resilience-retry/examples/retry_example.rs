use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_retry::{ExponentialBackoff, RetryConfig};

#[derive(Debug, Clone)]
struct TemporaryError;

impl std::fmt::Display for TemporaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "temporary error")
    }
}

impl std::error::Error for TemporaryError {}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Tower Retry-Plus Example");
    println!("========================\n");

    // Track call attempts
    let call_count = Arc::new(AtomicUsize::new(0));

    // Example 1: Fixed backoff with automatic retry
    println!("Example 1: Fixed backoff retry");
    let cc = Arc::clone(&call_count);
    call_count.store(0, Ordering::SeqCst);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            println!("  Service called (attempt {})", count + 1);
            if count < 2 {
                Err(TemporaryError)
            } else {
                Ok(format!("Success: {}", req))
            }
        }
    });

    let retry_config: RetryConfig<TemporaryError> = RetryConfig::builder()
        .max_attempts(5)
        .fixed_backoff(Duration::from_millis(100))
        .on_retry(|attempt, delay| {
            println!("  [RETRY] Attempt {} after {:?}", attempt, delay);
        })
        .on_success(|attempts| {
            println!("  [SUCCESS] After {} total attempts", attempts);
        })
        .build();

    let retry_layer = retry_config.layer();
    let mut service = retry_layer.layer(service);

    let result = service.ready().await?.call("test".to_string()).await?;
    println!("  Result: {}\n", result);

    // Example 2: Exponential backoff
    println!("Example 2: Exponential backoff");
    call_count.store(0, Ordering::SeqCst);
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            println!("  Service called (attempt {})", count + 1);
            if count < 3 {
                Err(TemporaryError)
            } else {
                Ok(format!("Success: {}", req))
            }
        }
    });

    let retry_config: RetryConfig<TemporaryError> = RetryConfig::builder()
        .max_attempts(5)
        .backoff(
            ExponentialBackoff::new(Duration::from_millis(50))
                .multiplier(2.0)
                .max_interval(Duration::from_secs(1)),
        )
        .on_retry(|attempt, delay| {
            println!("  [RETRY] Attempt {} after {:?}", attempt, delay);
        })
        .on_success(|attempts| {
            println!("  [SUCCESS] After {} total attempts", attempts);
        })
        .build();

    let retry_layer = retry_config.layer();
    let mut service = retry_layer.layer(service);

    let result = service.ready().await?.call("test".to_string()).await?;
    println!("  Result: {}\n", result);

    // Example 3: Retry with predicate (selective retry)
    println!("Example 3: Retry predicate (only retry temporary errors)");
    call_count.store(0, Ordering::SeqCst);

    #[derive(Debug, Clone)]
    struct PermanentError;

    impl std::fmt::Display for PermanentError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "permanent error")
        }
    }

    impl std::error::Error for PermanentError {}

    let service = tower::service_fn(|_req: String| async move {
        println!("  Service called");
        Err::<String, _>(PermanentError)
    });

    let retry_config: RetryConfig<PermanentError> = RetryConfig::builder()
        .max_attempts(5)
        .fixed_backoff(Duration::from_millis(50))
        .retry_on(|_: &PermanentError| false) // Never retry permanent errors
        .on_ignored_error(|| {
            println!("  [IGNORED] Error not retryable");
        })
        .build();

    let retry_layer = retry_config.layer();
    let mut service = retry_layer.layer(service);

    let result = service.ready().await?.call("test".to_string()).await;
    println!("  Result: {:?}\n", result);

    // Example 4: Exhausted retries
    println!("Example 4: Exhausted retries");
    call_count.store(0, Ordering::SeqCst);
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            println!("  Service called (attempt {})", count + 1);
            Err::<String, _>(TemporaryError)
        }
    });

    let retry_config: RetryConfig<TemporaryError> = RetryConfig::builder()
        .max_attempts(3)
        .fixed_backoff(Duration::from_millis(50))
        .on_retry(|attempt, _| {
            println!("  [RETRY] Attempt {}", attempt);
        })
        .on_error(|attempts| {
            println!("  [ERROR] Exhausted retries after {} attempts", attempts);
        })
        .build();

    let retry_layer = retry_config.layer();
    let mut service = retry_layer.layer(service);

    let result = service.ready().await?.call("test".to_string()).await;
    println!("  Result: {:?}\n", result);

    Ok(())
}
