use lambda_runtime::{Error, LambdaEvent};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Instant;
use tokio_postgres_dsql::SingleConnection;

#[derive(Deserialize)]
pub struct Request {
    payer_id: i32,
    payee_id: i32,
    amount: Decimal,
}

#[derive(Serialize)]
pub struct Response {
    payer_balance: Decimal,
    transaction_time: String,
}

pub(crate) async fn function_handler(
    connection: Arc<Mutex<SingleConnection>>,
    event: LambdaEvent<Request>,
) -> Result<Response, Error> {
    let start = Instant::now();

    if event.payload.payer_id == event.payload.payee_id {
        return Err("Payer and payee must be different accounts".into());
    }

    let mut connection = connection.lock().await;
    let client = connection.borrow().await?;

    // Start transaction
    let transaction = client.transaction().await?;

    // Deduct from payer and check balance
    let row = transaction
        .query_one(
            "UPDATE accounts SET balance = balance - $1 WHERE id = $2 RETURNING balance",
            &[&event.payload.amount, &event.payload.payer_id],
        )
        .await?;

    let payer_balance: Decimal = row.get(0);
    if payer_balance < Decimal::ZERO {
        return Err("Insufficient balance: {payer_balance}".into());
    }

    // Add to payee
    let rows_updated = transaction
        .execute(
            "UPDATE accounts SET balance = balance + $1 WHERE id = $2",
            &[&event.payload.amount, &event.payload.payee_id],
        )
        .await?;

    if rows_updated != 1 {
        return Err("Payee account not found".into());
    }

    // Commit transaction
    transaction.commit().await?;

    let elapsed = start.elapsed();
    let transaction_time = format!("{:.3}ms", elapsed.as_secs_f64() * 1000.0);

    Ok(Response {
        transaction_time,
        payer_balance,
    })
}
