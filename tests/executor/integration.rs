//! Integration tests for the executor layer.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_resilience_executor::{ExecutorError, ExecutorLayer};

#[tokio::test]
async fn basic_request_processing() {
    let svc = tower::service_fn(|req: String| async move {
        Ok::<_, std::io::Error>(format!("hello {}", req))
    });

    let mut svc = ServiceBuilder::new()
        .layer(ExecutorLayer::current())
        .service(svc);

    let resp = svc
        .ready()
        .await
        .unwrap()
        .call("world".into())
        .await
        .unwrap();
    assert_eq!(resp, "hello world");
}

#[tokio::test]
async fn error_propagation() {
    let svc = tower::service_fn(|_req: ()| async move {
        Err::<(), _>(std::io::Error::other("boom"))
    });

    let mut svc = ServiceBuilder::new()
        .layer(ExecutorLayer::current())
        .service(svc);

    let result = svc.ready().await.unwrap().call(()).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        ExecutorError::Service(e) => assert_eq!(e.to_string(), "boom"),
        other => panic!("expected Service error, got {:?}", other),
    }
}

#[tokio::test]
async fn parallel_execution_across_tasks() {
    let concurrent = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));

    let cc = Arc::clone(&concurrent);
    let mc = Arc::clone(&max_concurrent);

    let svc = tower::service_fn(move |_req: ()| {
        let concurrent = Arc::clone(&cc);
        let max_concurrent = Arc::clone(&mc);
        async move {
            let current = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
            max_concurrent.fetch_max(current, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(50)).await;
            concurrent.fetch_sub(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(())
        }
    });

    let svc = ServiceBuilder::new()
        .layer(ExecutorLayer::current())
        .service(svc);

    let mut handles = vec![];
    for _ in 0..10 {
        let mut s = svc.clone();
        handles.push(tokio::spawn(async move {
            s.ready().await.unwrap().call(()).await.unwrap();
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert!(
        max_concurrent.load(Ordering::SeqCst) > 1,
        "expected parallel execution, max concurrent was {}",
        max_concurrent.load(Ordering::SeqCst)
    );
}

#[tokio::test]
async fn with_explicit_handle() {
    let handle = tokio::runtime::Handle::current();

    let svc = tower::service_fn(|req: i32| async move { Ok::<_, std::io::Error>(req * 2) });

    let mut svc = ServiceBuilder::new()
        .layer(ExecutorLayer::new(handle))
        .service(svc);

    let resp = svc.ready().await.unwrap().call(21).await.unwrap();
    assert_eq!(resp, 42);
}

#[tokio::test]
async fn with_dedicated_runtime() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .build()
        .unwrap();

    let svc =
        tower::service_fn(|req: String| async move { Ok::<_, std::io::Error>(req.to_uppercase()) });

    let mut svc = ServiceBuilder::new()
        .layer(ExecutorLayer::new(rt.handle().clone()))
        .service(svc);

    let resp = svc
        .ready()
        .await
        .unwrap()
        .call("test".into())
        .await
        .unwrap();
    assert_eq!(resp, "TEST");

    // Shut down on a blocking thread to avoid "cannot drop runtime in async" panic
    tokio::task::spawn_blocking(move || {
        rt.shutdown_timeout(Duration::from_secs(1));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn multiple_sequential_calls() {
    let count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&count);

    let svc = tower::service_fn(move |req: i32| {
        let c = Arc::clone(&c);
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(req)
        }
    });

    let mut svc = ServiceBuilder::new()
        .layer(ExecutorLayer::current())
        .service(svc);

    for i in 0..10 {
        let resp = svc.ready().await.unwrap().call(i).await.unwrap();
        assert_eq!(resp, i);
    }

    assert_eq!(count.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn builder_pattern_works() {
    let svc = tower::service_fn(|req: i32| async move { Ok::<_, std::io::Error>(req + 1) });

    let mut svc = ServiceBuilder::new()
        .layer(
            ExecutorLayer::<tokio::runtime::Handle>::builder()
                .current()
                .build(),
        )
        .service(svc);

    let resp = svc.ready().await.unwrap().call(41).await.unwrap();
    assert_eq!(resp, 42);
}
