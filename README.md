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
$ cargo lambda invoke --remote ch03 --data-ascii '{"id": 1}'
```
