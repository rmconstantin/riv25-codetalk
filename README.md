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

    new dsql.CfnCluster(this, 'DsqlCluster', {
      deletionProtectionEnabled: false,
      tags: [{
        key: 'Name',
        value: 'ch02'
      }]
    });
  }
}
```

Deploy the stack:

``` sh
$ npx cdk deploy --profile YOUR_AWS_PROFILE --region us-west-2
```
