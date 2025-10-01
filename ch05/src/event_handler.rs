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
    attempts: usize,
}

fn is_occ_error(error: &tokio_postgres::Error) -> bool {
    error
        .as_db_error()
        .map(|db_err| db_err.code() == &tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE)
        .unwrap_or(false)
}

async fn execute_transfer(
    transaction: &tokio_postgres::Transaction<'_>,
    payer_id: i32,
    payee_id: i32,
    amount: Decimal,
) -> anyhow::Result<Decimal> {
    // Deduct from payer and check balance
    let row = transaction
        .query_one(
            "UPDATE accounts SET balance = balance - $1 WHERE id = $2 RETURNING balance",
            &[&amount, &payer_id],
        )
        .await?;

    let payer_balance: Decimal = row.get(0);
    if payer_balance < Decimal::ZERO {
        anyhow::bail!("Insufficient balance");
    }

    // Add to payee
    let rows_updated = transaction
        .execute(
            "UPDATE accounts SET balance = balance + $1 WHERE id = $2",
            &[&amount, &payee_id],
        )
        .await?;

    if rows_updated != 1 {
        anyhow::bail!("Payee account not found");
    }

    Ok(payer_balance)
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

    // Retry loop for OCC failures
    let mut attempts = 0;
    let payer_balance = loop {
        attempts += 1;
        let transaction = client.transaction().await?;

        let payer_balance = execute_transfer(
            &transaction,
            event.payload.payer_id,
            event.payload.payee_id,
            event.payload.amount,
        )
        .await?;

        match transaction.commit().await {
            Ok(_) => break payer_balance,
            Err(err) => {
                if !is_occ_error(&err) {
                    return Err(err)?;
                }
                // OCC error on commit, continue to retry
            }
        }
    };

    let elapsed = start.elapsed();
    let transaction_time = format!("{:.3}ms", elapsed.as_secs_f64() * 1000.0);

    Ok(Response {
        payer_balance,
        transaction_time,
        attempts,
    })
}
