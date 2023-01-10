use std::str::FromStr;
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use lite_rpc::{bridge::LiteBridge, cli::Args};
use reqwest::Url;
use simplelog::*;

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    TermLogger::init(
        LevelFilter::Info,
        Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )?;

    let Args {
        rpc_addr,
        ws_addr,
        tx_batch_size,
        lite_rpc_ws_addr,
        lite_rpc_http_addr,
        tx_batch_interval_ms,
        clean_interval_ms,
        fanout_size,
    } = Args::parse();

    let tx_batch_interval_ms = Duration::from_millis(tx_batch_interval_ms);
    let clean_interval_ms = Duration::from_millis(clean_interval_ms);

    let light_bridge =
        LiteBridge::new(Url::from_str(&rpc_addr).unwrap(), &ws_addr, fanout_size).await?;

    let services = light_bridge
        .start_services(
            lite_rpc_http_addr,
            lite_rpc_ws_addr,
            tx_batch_size,
            tx_batch_interval_ms,
            clean_interval_ms,
        )
        .await?;

    let services = futures::future::try_join_all(services);

    let ctrl_c_signal = tokio::signal::ctrl_c();

    tokio::select! {
        services = services => {
            services.context("Some services exited unexpectedly")?;
        }
        _ = ctrl_c_signal => {}
    }

    Ok(())
}
