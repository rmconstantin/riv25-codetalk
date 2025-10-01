# Amazon Aurora DSQL: A developer's perspective (DAT401)

In this live coding session, we'll show you how to work with Amazon Aurora DSQL from a developer's perspective. We'll develop a sample application to highlight some of the ways developing for Aurora DSQL is different than PostgreSQL. We'll cover authentication and connection management, optimistic concurrency transaction patterns, primary key selection, analyzing query performance, and best practices.

## Prerequisites

- Rust toolchain
- Node.js and npm
- AWS CDK CLI (`npm install -g aws-cdk`)
- AWS credentials configured

Follow the instructions at
https://docs.aws.amazon.com/lambda/latest/dg/rust-package.html#rust-package-build
for how to build Rust packages on your operating system.

**Note:** After deploying Lambda functions that connect to DSQL (ch03 and ch04), you'll need to add IAM permissions using the `add-dsql-permissions.sh` script:

``` sh
$ ./add-dsql-permissions.sh <function-name>
```

This grants the Lambda function the `dsql:DbConnect` and `dsql:DbConnectAdmin` permissions needed to authenticate with Aurora DSQL.

## Chapter 01

First, we're going to build a Lambda function:

``` sh
$ cargo lambda new ch01
> Is this function an HTTP function? No
> Event type that this function receives serde_json::Value
```

``` sh
cargo lambda deploy ch01
cargo lambda invoke --remote ch01 --data-ascii '{"key": "value"}'
```

Now, update this function to return a greeting. We're going to use `serde`
(`cargo add serde --features derive`) to serialize and deserialize JSON:

``` rust
use lambda_runtime::{Error, LambdaEvent};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct Request {
    name: String,
}

#[derive(Serialize)]
pub struct Response {
    greeting: String,
}

/// This is the main body for the function.
/// Write your code inside it.
/// There are some code example in the following URLs:
/// - https://github.com/awslabs/aws-lambda-rust-runtime/tree/main/examples
/// - https://github.com/aws-samples/serverless-rust-demo/
pub(crate) async fn function_handler(event: LambdaEvent<Request>) -> Result<Response, Error> {
    let name = event.payload.name;

    Ok(Response {
        greeting: format!("hello {name}"),
    })
}
```

Deploy and invoke the function:

``` sh
$ cargo lambda deploy ch01
$ cargo lambda invoke --remote ch01 --data-ascii '{"name": "reinvent"}'
{"greeting":"hello reinvent"}
```

## Chapter 02

Create an Aurora DSQL cluster using CDK:

``` sh
$ mkdir ch02
$ cd ch02
$ cdk init app --language typescript
```

Update the stack to include an Aurora DSQL cluster with no deletion protection:

``` typescript
import * as cdk from 'aws-cdk-lib';
import * as dsql from 'aws-cdk-lib/aws-dsql';
import { Construct } from 'constructs';

export class Ch02Stack extends cdk.Stack {
  constructor(scope: Construct, id: string, props?: cdk.StackProps) {
    super(scope, id, props);

    const cluster = new dsql.CfnCluster(this, 'DsqlCluster', {
      deletionProtectionEnabled: false,
      tags: [{
        key: 'Name',
        value: 'ch02'
      }]
    });

    new cdk.CfnOutput(this, 'ClusterId', {
      value: cluster.attrIdentifier,
      description: 'Aurora DSQL Cluster ID'
    });
  }
}
```

Deploy the stack:

``` sh
$ npx cdk deploy --profile YOUR_AWS_PROFILE --region us-west-2
```

The deployment will output the cluster ID, which you'll need for connecting to the database.

## Chapter 03

We're going to connect to the cluster from our Lambda function. Let's first
connect from the command line:

``` sh
export PGUSER=admin
export PGDATABASE=postgres
export PGHOST=YOUR_CLUSTER_ID.dsql.AWS_REGION.on.aws
export PGPASSWORD=$(aws dsql generate-db-connect-admin-auth-token --host $PGHOST)

psql
```

Make a table called `accounts`:

``` sql
create table accounts (
  id int primary key,
  balance numeric
)

postgres=> \d accounts
                 Table "public.accounts"
 Column  |     Type      | Collation | Nullable | Default
---------+---------------+-----------+----------+---------
 id      | integer       |           | not null |
 balance | numeric(18,6) |           |          |
Indexes:
    "accounts_pkey" PRIMARY KEY, btree_index (id) INCLUDE (balance)

postgres=> insert into accounts (id, balance) values (1, 100);
INSERT 0 1
```

Create a new Lambda function for connecting to Aurora DSQL:

``` sh
$ cargo lambda new --event-type serde_json::Value ch03
```

Add the `tokio-postgres-dsql` library to simplify DSQL connections:

``` sh
$ cd ch03
$ cargo add tokio-postgres-dsql --git https://github.com/marcbowes/tokio-postgres-dsql --features openssl
$ cargo add serde --features derive
$ cargo add rust_decimal --features db-postgres
```

The `tokio-postgres-dsql` library handles authentication and connection management automatically:

``` rust
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
```

Deploy and invoke the function:

``` sh
$ cargo lambda build --release
$ cargo lambda deploy ch03
$ ./add-dsql-permissions.sh ch03
$ cargo lambda invoke --remote ch03 --data-ascii '{"id": 1}'
```

## Chapter 04

Now we'll implement a money transfer function with transactions. This chapter demonstrates connection reuse and transaction safety.

### Step 1: Reuse the connection

Instead of creating a new connection for each invocation, we'll create the connection once in `main.rs` and reuse it across Lambda invocations. This significantly improves performance by avoiding connection overhead.

Wrap the connection in `Arc<Mutex<SingleConnection>>` and pass it to the handler:

``` rust
let opts = Opts::from_conninfo(CONNINFO).await?;
let connection = opts.connect_one().await?;
let connection = Arc::new(Mutex::new(connection));

run(service_fn(move |event| {
    let connection = Arc::clone(&connection);
    async move { function_handler(connection, event).await }
}))
.await
```

### Step 2: Change the API types

Update the request to accept transfer parameters and the response to return transaction results:

``` rust
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
```

### Step 3: Implement the transaction

The transfer uses a PostgreSQL transaction with safety checks:

``` sql
-- Start transaction
BEGIN;

-- Deduct from payer and return new balance
UPDATE accounts SET balance = balance - $amount WHERE id = $payer_id RETURNING balance;

-- Add to payee
UPDATE accounts SET balance = balance + $amount WHERE id = $payee_id;

-- Commit transaction
COMMIT;
```

**Safety checks:**
- Check that exactly 1 row was updated for the payee (validates payee exists)
- Check that the payer's balance is not negative after the deduction
- If either check fails, return an error and the transaction is automatically rolled back
- Use `tokio::time::Instant` to measure the transaction duration

### Step 4: Populate the database

Use the `setup.sql` script to create test data:

``` sql
DELETE FROM accounts;

INSERT INTO accounts (id, balance)
SELECT generate_series(1, 1000), 100;
```

This creates 1000 accounts (IDs 1-1000), each with a balance of 100.

Run the script:

``` sh
$ psql < ch04/setup.sql
```

Deploy and test:

``` sh
$ cargo lambda build --release --manifest-path ch04/Cargo.toml
$ cargo lambda deploy ch04
$ ./add-dsql-permissions.sh ch04
$ cargo lambda invoke --remote ch04 --data-ascii '{"payer_id": 1, "payee_id": 2, "amount": 10}'
```

### Load testing

Use the `invoke-test` tool to run load tests against the Lambda function:

``` sh
# Run with defaults (1000 iterations, 1 thread, 1000 accounts)
$ cargo run --manifest-path invoke-test/Cargo.toml -- ch04

# Run 500 iterations across 5 threads
$ cargo run --manifest-path invoke-test/Cargo.toml -- ch04 --iters 500 --threads 5

# Run 10,000 iterations across 10 threads with 100 accounts
$ cargo run --manifest-path invoke-test/Cargo.toml -- ch04 --iters 10000 --threads 10 --accounts 100
```

The tool uses the AWS SDK for Rust to invoke the Lambda function directly, making it faster than using `cargo lambda invoke`. The `--accounts` flag controls the range of account IDs to use (1 to N).

## Chapter 05

Chapter 05 extends ch04 by adding automatic retry logic for optimistic concurrency control (OCC) failures. When multiple transactions conflict, DSQL returns a serialization failure error, and the application should retry the transaction.

### Key Changes from Chapter 04

1. **Automatic OCC retry** - Transactions that fail with serialization errors (`T_R_SERIALIZATION_FAILURE`) are automatically retried
2. **Attempts tracking** - The response includes an `attempts` field showing how many tries were needed
3. **Clean separation** - `execute_transfer` function contains transaction logic, retry loop handles OCC errors at commit time

The retry logic uses an infinite loop that only retries on serialization failures:

``` rust
loop {
    attempts += 1;
    let transaction = client.transaction().await?;

    let payer_balance = execute_transfer(&transaction, ...).await?;

    match transaction.commit().await {
        Ok(_) => break payer_balance,
        Err(err) if is_occ_error(&err) => continue,
        Err(err) => return Err(err)?,
    }
}
```

OCC detection uses the proper SQL state constant:

``` rust
fn is_occ_error(error: &tokio_postgres::Error) -> bool {
    error
        .as_db_error()
        .map(|db_err| db_err.code() == &tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE)
        .unwrap_or(false)
}
```

Deploy and test:

``` sh
$ cargo lambda build --release --manifest-path ch05/Cargo.toml
$ cargo lambda deploy ch05
$ ./add-dsql-permissions.sh ch05
$ cargo lambda invoke --remote ch05 --data-ascii '{"payer_id": 1, "payee_id": 2, "amount": 10}'
```

Compare with ch04 under load to see the difference in error rates:

``` sh
# ch04 - no retry, will show OCC errors under contention
$ cargo run --manifest-path invoke-test/Cargo.toml -- ch04 --iters 1000 --threads 10 --accounts 10

# ch05 - automatic retry, should succeed with multiple attempts
$ cargo run --manifest-path invoke-test/Cargo.toml -- ch05 --iters 1000 --threads 10 --accounts 10
```

With low account counts and high concurrency, ch04 will show serialization failure errors while ch05 will retry and succeed, with the `attempts` field showing how many retries were needed.
