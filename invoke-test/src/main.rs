use aws_config::BehaviorVersion;
use aws_sdk_lambda::Client;
use clap::Parser;
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde::Deserialize;
use std::fs;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::Mutex;
use tokio::task::JoinSet;

#[derive(Default)]
struct Stats {
    success_count: usize,
    error_count: usize,
    insufficient_balance_count: usize,
    total_latency_ms: f64,
}

fn print_stats(stats: &Stats, completed_count: usize) {
    println!();
    println!("Completed {} invocations", completed_count);
    println!();
    println!("Results:");
    println!("  Success: {}", stats.success_count);
    println!("  Errors:  {}", stats.error_count);
    println!(
        "  Insufficient balance: {}",
        stats.insufficient_balance_count
    );
    if stats.success_count > 0 {
        let avg_latency = stats.total_latency_ms / stats.success_count as f64;
        println!("  Avg latency: {:.3}ms", avg_latency);
    }
}

#[derive(Deserialize)]
struct SuccessResponse {
    transaction_time: String,
}

#[derive(Parser, Debug)]
#[command(name = "invoke-test")]
#[command(about = "Invoke Lambda function with random account transfers")]
struct Args {
    /// Lambda function name
    function: String,

    /// Number of iterations to run
    #[arg(long, default_value = "1000")]
    iters: usize,

    /// Number of parallel threads
    #[arg(long, default_value = "1")]
    threads: usize,

    /// Number of accounts (1 to N)
    #[arg(long, default_value = "1000")]
    accounts: u32,

    /// Use UUIDs from uuids.txt instead of integer IDs
    #[arg(long)]
    uuids: bool,
}

async fn run_invocations(
    client: Arc<Client>,
    function_name: String,
    thread_id: usize,
    start: usize,
    end: usize,
    total: usize,
    num_accounts: u32,
    stats: Arc<Mutex<Stats>>,
    uuids: Arc<Vec<String>>,
) {
    let mut rng = StdRng::from_entropy();

    for i in start..=end {
        let (payer_id, payee_id, amount, payer_display, payee_display) = if uuids.is_empty() {
            // Default mode: use integer IDs
            let payer_id = rng.gen_range(1..=num_accounts);
            let mut payee_id = rng.gen_range(1..=num_accounts);
            while payer_id == payee_id {
                payee_id = rng.gen_range(1..=num_accounts);
            }
            let amount: f64 = rng.gen_range(0.01..=10.00);
            let amount = (amount * 100.0).round() / 100.0;
            (
                serde_json::json!(payer_id),
                serde_json::json!(payee_id),
                amount,
                payer_id.to_string(),
                payee_id.to_string(),
            )
        } else {
            // ch06 mode: use UUIDs
            let payer_idx = rng.gen_range(0..uuids.len());
            let mut payee_idx = rng.gen_range(0..uuids.len());
            while payer_idx == payee_idx {
                payee_idx = rng.gen_range(0..uuids.len());
            }
            let amount: f64 = rng.gen_range(0.01..=10.00);
            let amount = (amount * 100.0).round() / 100.0;
            (
                serde_json::json!(&uuids[payer_idx]),
                serde_json::json!(&uuids[payee_idx]),
                amount,
                uuids[payer_idx].clone(),
                uuids[payee_idx].clone(),
            )
        };

        // Create payload
        let payload = serde_json::json!({
            "payer_id": payer_id,
            "payee_id": payee_id,
            "amount": amount
        });

        // Invoke Lambda function
        let result = client
            .invoke()
            .function_name(&function_name)
            .payload(aws_sdk_lambda::primitives::Blob::new(
                serde_json::to_vec(&payload).unwrap(),
            ))
            .send()
            .await;

        match result {
            Ok(response) => {
                let response_payload = response
                    .payload()
                    .map(|b| String::from_utf8_lossy(b.as_ref()).to_string())
                    .unwrap_or_else(|| "No response".to_string());

                // Try to parse the response to extract transaction_time
                let mut is_error = false;
                let mut is_insufficient_balance = false;
                let mut latency_ms = 0.0;

                if let Ok(success_resp) = serde_json::from_str::<SuccessResponse>(&response_payload)
                {
                    // Extract latency from "16.955ms" format
                    if let Some(latency_str) = success_resp.transaction_time.strip_suffix("ms") {
                        if let Ok(latency) = latency_str.parse::<f64>() {
                            latency_ms = latency;
                        }
                    }
                } else {
                    // Check if it's an error response
                    if response_payload.contains("errorType")
                        || response_payload.contains("errorMessage")
                    {
                        is_error = true;
                        if response_payload.contains("Insufficient balance") {
                            is_insufficient_balance = true;
                        }
                    }
                }

                // Update stats
                {
                    let mut stats = stats.lock().await;
                    if is_error {
                        if is_insufficient_balance {
                            stats.insufficient_balance_count += 1;
                        } else {
                            stats.error_count += 1;
                        }
                    } else {
                        stats.success_count += 1;
                        stats.total_latency_ms += latency_ms;
                    }
                }

                println!(
                    "[Thread {}: {}/{}] Transferring {} from account {} to {} => {}",
                    thread_id, i, total, amount, payer_display, payee_display, response_payload
                );
            }
            Err(e) => {
                // Update error count
                {
                    let mut stats = stats.lock().await;
                    stats.error_count += 1;
                }

                eprintln!(
                    "[Thread {}: {}/{}] Error transferring {} from account {} to {}: {}",
                    thread_id, i, total, amount, payer_display, payee_display, e
                );
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    println!(
        "Running {} invocations across {} thread(s)",
        args.iters, args.threads
    );

    // Load UUIDs if --uuids flag is set
    let uuids = if args.uuids {
        let content = fs::read_to_string("uuids.txt").expect("Failed to read uuids.txt");
        Arc::new(
            content
                .lines()
                .filter(|line| !line.is_empty())
                .map(|line| line.trim().to_string())
                .take(args.accounts as usize)
                .collect::<Vec<String>>(),
        )
    } else {
        Arc::new(Vec::new())
    };

    if !uuids.is_empty() {
        println!("Using {} UUIDs from uuids.txt", uuids.len());
    }

    // Create AWS Lambda client
    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let client = Arc::new(Client::new(&config));

    // Create shared stats
    let stats = Arc::new(Mutex::new(Stats::default()));

    // Calculate iterations per thread
    let iters_per_thread = args.iters / args.threads;
    let remainder = args.iters % args.threads;

    let mut tasks = JoinSet::new();

    let total_iters = args.iters;
    let num_accounts = args.accounts;

    let mut start = 1;
    for t in 1..=args.threads {
        let end = if t == args.threads {
            start + iters_per_thread - 1 + remainder
        } else {
            start + iters_per_thread - 1
        };

        let client = Arc::clone(&client);
        let function_name = args.function.clone();
        let stats = Arc::clone(&stats);
        let uuids = Arc::clone(&uuids);

        tasks.spawn(async move {
            run_invocations(
                client,
                function_name,
                t,
                start,
                end,
                total_iters,
                num_accounts,
                stats,
                uuids,
            )
            .await;
        });

        start = end + 1;
    }

    // Wait for all tasks to complete or handle ctrl-c
    let mut completed = 0;

    loop {
        if tasks.is_empty() {
            break;
        }

        tokio::select! {
            result = tasks.join_next() => {
                match result {
                    Some(Ok(_)) => { completed += 1 },
                    Some(Err(err)) => {
                        eprintln!("Task failed: {err}");
                    }
                    None => unreachable!("see break statement"),
                }
            }
            _ = signal::ctrl_c() => {
                // Interrupted by ctrl-c
                println!();
                println!("Interrupted! Aborting remaining tasks...");
                tasks.abort_all();
            }
        }
    }

    // All tasks completed normally
    let stats = stats.lock().await;
    print_stats(&stats, completed);
}
