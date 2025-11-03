use clap::Parser;
use parking_lot::Mutex;
use rw_message::GwMessage;
use std::{net::IpAddr, sync::Arc};
use warp::{reply::Reply, Filter};

mod collector;
mod config;
mod measurements;
mod metrics;
mod rw_message;

use collector::collect_metrics;
use config::{Config, MacMapping};
use measurements::Measurements;

fn post_measurements(
    data: GwMessage,
    sensor_state: Arc<parking_lot::lock_api::Mutex<parking_lot::RawMutex, Measurements>>,
) -> impl Reply {
    let mut state = sensor_state.lock();
    state.last_update = data.timestamp;
    state.last_nonce = Some(data.nonce);
    state.mac = data.gw_mac;
    for tag in data.tags {
        state.update_tag(tag);
    }
    drop(state);

    warp::reply::with_header("", "X-Ruuvi-Gateway-Rate", "1")
}

fn metrics(
    sensor_state: Arc<parking_lot::lock_api::Mutex<parking_lot::RawMutex, Measurements>>,
    names: Arc<MacMapping>,
) -> impl Reply {
    let state = sensor_state.lock();
    collect_metrics(&state, &names)
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let config = Config::parse();

    // Load MAC address mappings if config file is specified, otherwise use empty mapping
    let names = config.mac_mapping.map_or_else(
        || MacMapping::default(),
        |path| MacMapping::load(&path).expect("Failed to load MAC mapping file"),
    );
    let names = Arc::new(names);

    let sensor_state = Arc::new(Mutex::new(Measurements::new()));

    let post_measurements = warp::post()
        .and(warp::path::end())
        .and(warp::body::content_length_limit(1024 * 1024)) // 1 MB should be plenty for sensor data
        .and(warp::body::json())
        .and(warp::any().map({
            let sensor_state = sensor_state.clone();
            move || sensor_state.clone()
        }))
        .map(post_measurements);

    let metrics = warp::get()
        .and(warp::path!("metrics"))
        .and(warp::any().map({
            let sensor_state = sensor_state.clone();
            move || sensor_state.clone()
        }))
        .and(warp::any().map({
            let names = names.clone();
            move || names.clone()
        }))
        .map(metrics);

    println!("Starting server on {}:{}", config.interface, config.port);
    warp::serve(post_measurements.or(metrics))
        .run((config.interface.parse::<IpAddr>().unwrap(), config.port))
        .await;
}
