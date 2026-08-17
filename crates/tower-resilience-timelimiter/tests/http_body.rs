//! HTTP response-phase contracts for TimeLimiter and tower-http 0.7.

use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body::{Body, Frame};
use http_body_util::BodyExt;
use pin_project_lite::pin_project;
use std::collections::VecDeque;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{ready, Context, Poll};
use std::time::Duration;
use tokio::time::{sleep, Sleep};
use tower::{service_fn, Service, ServiceBuilder, ServiceExt};
use tower_http::timeout::{ResponseBodyDeadlineLayer, ResponseBodyTimeoutLayer, TimeoutError};
use tower_resilience_timelimiter::TimeLimiterLayer;

struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

pin_project! {
    struct DelayedBody {
        delays: VecDeque<Duration>,
        #[pin]
        sleep: Option<Sleep>,
        _drop_flag: Option<DropFlag>,
    }
}

impl DelayedBody {
    fn new(delays: impl IntoIterator<Item = Duration>) -> Self {
        Self {
            delays: delays.into_iter().collect(),
            sleep: None,
            _drop_flag: None,
        }
    }

    fn with_drop_flag(
        delays: impl IntoIterator<Item = Duration>,
        dropped: Arc<AtomicBool>,
    ) -> Self {
        Self {
            delays: delays.into_iter().collect(),
            sleep: None,
            _drop_flag: Some(DropFlag(dropped)),
        }
    }
}

impl Body for DelayedBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let mut this = self.project();
        let Some(delay) = this.delays.front().copied() else {
            return Poll::Ready(None);
        };

        let sleep = if let Some(sleep) = this.sleep.as_mut().as_pin_mut() {
            sleep
        } else {
            this.sleep.set(Some(sleep(delay)));
            this.sleep.as_mut().as_pin_mut().unwrap()
        };

        ready!(sleep.poll(cx));
        this.sleep.set(None);
        this.delays.pop_front();
        Poll::Ready(Some(Ok(Frame::data(Bytes::from_static(b"chunk")))))
    }

    fn is_end_stream(&self) -> bool {
        self.delays.is_empty()
    }
}

fn assert_body_timeout(error: &(dyn std::error::Error + Send + Sync + 'static)) {
    assert!(
        error.downcast_ref::<TimeoutError>().is_some(),
        "expected tower-http TimeoutError, got {error}"
    );
}

#[tokio::test(start_paused = true)]
async fn slow_response_future_times_out_and_is_cancelled() {
    let dropped = Arc::new(AtomicBool::new(false));
    let service_dropped = Arc::clone(&dropped);
    let inner = service_fn(move |_request: Request<()>| {
        let service_dropped = Arc::clone(&service_dropped);
        async move {
            let _drop_flag = DropFlag(service_dropped);
            sleep(Duration::from_millis(50)).await;
            Ok::<_, Infallible>(Response::new(DelayedBody::new([])))
        }
    });
    let mut service = ServiceBuilder::new()
        .layer(
            TimeLimiterLayer::builder()
                .timeout_duration(Duration::from_millis(10))
                .build(),
        )
        .service(inner);

    let result = service.ready().await.unwrap().call(Request::new(())).await;
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("slow response future unexpectedly completed"),
    };

    assert!(error.is_timeout());
    assert!(
        dropped.load(Ordering::SeqCst),
        "the slow service future must be dropped on timeout"
    );
}

#[tokio::test(start_paused = true)]
async fn time_limiter_stops_when_the_response_is_produced() {
    let inner = service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(Response::new(DelayedBody::new([Duration::from_millis(50)])))
    });
    let mut service = ServiceBuilder::new()
        .layer(
            TimeLimiterLayer::builder()
                .timeout_duration(Duration::from_millis(10))
                .build(),
        )
        .service(inner);

    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::new(()))
        .await
        .expect("the response value is immediately available");
    let mut body = Box::pin(response.into_body());
    let body_started = tokio::time::Instant::now();

    let frame = body
        .frame()
        .await
        .expect("one delayed frame")
        .expect("body has no errors");
    assert_eq!(frame.into_data().unwrap(), Bytes::from_static(b"chunk"));
    assert_eq!(body_started.elapsed(), Duration::from_millis(50));
}

#[tokio::test(start_paused = true)]
async fn idle_body_timeout_preserves_response_parts_and_cancels_the_body() {
    let dropped = Arc::new(AtomicBool::new(false));
    let body_dropped = Arc::clone(&dropped);
    let inner = service_fn(move |_request: Request<()>| {
        let body_dropped = Arc::clone(&body_dropped);
        async move {
            Ok::<_, Infallible>(
                Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header("x-stream", "timed")
                    .body(DelayedBody::with_drop_flag(
                        [Duration::from_millis(50)],
                        body_dropped,
                    ))
                    .unwrap(),
            )
        }
    });
    let mut service = ServiceBuilder::new()
        .layer(ResponseBodyTimeoutLayer::new(Duration::from_millis(10)))
        .layer(
            TimeLimiterLayer::builder()
                .timeout_duration(Duration::from_millis(100))
                .build(),
        )
        .service(inner);

    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::new(()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()["x-stream"], "timed");

    {
        let mut body = Box::pin(response.into_body());
        let error = body.frame().await.unwrap().unwrap_err();
        assert_body_timeout(error.as_ref());
    }
    assert!(
        dropped.load(Ordering::SeqCst),
        "dropping a timed-out response body must release the inner body"
    );
}

#[tokio::test(start_paused = true)]
async fn idle_body_timeout_resets_after_each_frame() {
    let inner = service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(Response::new(DelayedBody::new([
            Duration::from_millis(6),
            Duration::from_millis(6),
            Duration::from_millis(6),
        ])))
    });
    let mut service = ServiceBuilder::new()
        .layer(ResponseBodyTimeoutLayer::new(Duration::from_millis(10)))
        .layer(
            TimeLimiterLayer::builder()
                .timeout_duration(Duration::from_millis(100))
                .build(),
        )
        .service(inner);
    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::new(()))
        .await
        .unwrap();
    let mut body = Box::pin(response.into_body());
    let mut frames = 0;

    while let Some(frame) = body.frame().await {
        frame.unwrap();
        frames += 1;
    }

    assert_eq!(frames, 3);
}

#[tokio::test(start_paused = true)]
async fn absolute_body_deadline_expires_despite_steady_frames() {
    let inner = service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(Response::new(DelayedBody::new([
            Duration::from_millis(6),
            Duration::from_millis(6),
            Duration::from_millis(6),
        ])))
    });
    let mut service = ServiceBuilder::new()
        .layer(ResponseBodyDeadlineLayer::new(Duration::from_millis(15)))
        .layer(
            TimeLimiterLayer::builder()
                .timeout_duration(Duration::from_millis(100))
                .build(),
        )
        .service(inner);
    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::new(()))
        .await
        .unwrap();
    let mut body = Box::pin(response.into_body());

    body.frame().await.unwrap().unwrap();
    body.frame().await.unwrap().unwrap();
    let error = body.frame().await.unwrap().unwrap_err();
    assert_body_timeout(error.as_ref());
}

#[tokio::test]
async fn response_body_layers_preserve_inner_service_errors() {
    let inner = service_fn(|_request: Request<()>| async {
        Err::<Response<DelayedBody>, _>("service error")
    });
    let mut service = ServiceBuilder::new()
        .layer(ResponseBodyTimeoutLayer::new(Duration::from_secs(1)))
        .layer(TimeLimiterLayer::standard().build())
        .service(inner);

    let result = service.ready().await.unwrap().call(Request::new(())).await;
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("inner service error was unexpectedly replaced by a response"),
    };

    assert_eq!(error.into_inner(), Some("service error"));
}
