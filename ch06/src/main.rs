use lambda_runtime::{run, service_fn, tracing, Error};
use tokio_postgres_dsql::Opts;

mod event_handler;
use event_handler::function_handler;

const CONNINFO: &str = "host=rbtglvixg55cxeimifwa2wqhwa.dsql.us-west-2.on.aws user=admin dbname=postgres";

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing::init_default_subscriber();

    let opts = Opts::from_conninfo(CONNINFO).await?;
    let connection = opts.connect_one().await?;

    run(service_fn(move |event| {
        let connection = connection.clone();
        async move { function_handler(connection, event).await }
    }))
    .await
}
