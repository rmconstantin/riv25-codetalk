use aws_config::{BehaviorVersion, Region};
use aws_sdk_dsql::auth_token::{AuthTokenGenerator, Config};
use lambda_runtime::{Error, LambdaEvent};
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
use postgres_openssl::MakeTlsConnector;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct Request {
    id: i32,
}

#[derive(Serialize)]
pub struct Response {
    balance: Decimal,
}

const ENDPOINT: &str = "rbtglvixg55cxeimifwa2wqhwa.dsql.us-west-2.on.aws";
const REGION: &str = "us-west-2";

async fn generate_token(hostname: String, region: String) -> String {
    let sdk_config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let signer = AuthTokenGenerator::new(
        Config::builder()
            .hostname(&hostname)
            .region(Region::new(region))
            .build()
            .unwrap(),
    );

    let token = signer
        .db_connect_admin_auth_token(&sdk_config)
        .await
        .unwrap();
    token.to_string()
}

pub(crate) async fn function_handler(event: LambdaEvent<Request>) -> Result<Response, Error> {
    let token = generate_token(ENDPOINT.to_string(), REGION.to_string()).await;
    let mut builder = SslConnector::builder(SslMethod::tls())?;
    // XXX: NOT FOR PRODUCTION USE. I'm uploading binaries from my laptop, and
    // am too lazy to use openssl-probe to detect the root CAs at runtime.
    builder.set_verify(SslVerifyMode::NONE);
    let connector = MakeTlsConnector::new(builder.build());

    let (client, connection) = tokio_postgres::connect(
        &format!("host={ENDPOINT} user=admin password={token} dbname=postgres sslmode=require"),
        connector,
    )
    .await?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });

    let row = client
        .query_one(
            "SELECT balance FROM accounts WHERE id = $1",
            &[&event.payload.id],
        )
        .await?;

    let balance: Decimal = row.get(0);

    Ok(Response { balance })
}
