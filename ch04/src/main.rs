use lambda_runtime::{run, service_fn, tracing, Error};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_postgres_dsql::Opts;

mod event_handler;
use event_handler::function_handler;

const CONNINFO: &str = "host=YOUR_CLUSTER_ENDPOINT user=admin dbname=postgres";

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing::init_default_subscriber();

    let opts = Opts::from_conninfo(CONNINFO).await?;
    let connection = opts.connect_one().await?;
    let connection = Arc::new(Mutex::new(connection));

    run(service_fn(move |event| {
        let connection = Arc::clone(&connection);
        async move { function_handler(connection, event).await }
    }))
    .await
}
