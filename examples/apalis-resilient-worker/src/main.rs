use apalis::prelude::BoxDynError;

#[tokio::main]
async fn main() -> Result<(), BoxDynError> {
    apalis_resilient_worker::run_all().await
}
