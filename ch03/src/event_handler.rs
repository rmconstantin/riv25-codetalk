use lambda_runtime::{Error, LambdaEvent};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tokio_postgres_dsql::Opts;

#[derive(Deserialize)]
pub struct Request {
    id: i32,
}

#[derive(Serialize)]
pub struct Response {
    balance: Decimal,
}

const CONNINFO: &str = "host=YOUR_CLUSTER_ENDPOINT user=admin dbname=postgres";

pub(crate) async fn function_handler(event: LambdaEvent<Request>) -> Result<Response, Error> {
    let opts = Opts::from_conninfo(CONNINFO).await?;
    let mut connection = opts.connect_one().await?;
    let client = connection.borrow().await?;

    let row = client
        .query_one(
            "SELECT balance FROM accounts WHERE id = $1",
            &[&event.payload.id],
        )
        .await?;

    let balance: Decimal = row.get(0);

    Ok(Response { balance })
}
