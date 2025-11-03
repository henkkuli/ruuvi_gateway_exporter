use clap::Parser;
use parking_lot::Mutex;
use ruuvi_sensor_protocol::{
    Acceleration, BatteryPotential, Humidity, MeasurementSequenceNumber, MovementCounter, Pressure,
    Temperature, TransmitterPower,
};
use rw_message::GwMessage;
use std::{net::IpAddr, sync::Arc};
use warp::{reply::Reply, Filter};

mod config;
mod measurements;
mod metrics;
mod rw_message;

use config::{Config, MacMapping};
use measurements::Measurements;
use metrics::{labelset, metric};

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

fn collect_metrics(state: &Measurements, names: &MacMapping) -> String {
    let mut metrics = Vec::new();

    // Gateway metrics with optional name
    let mut gw_labels = labelset().label("gw_mac", &state.mac);
    if let Some(name) = names.lookup(&state.mac) {
        gw_labels = gw_labels.label("name", name);
    }

    metrics.push(
        metric("ruuvi_gateway_update_timestamp_seconds")
            .labels(&gw_labels)
            .value(state.last_update.to_unix_seconds())
            .to_string(),
    );

    if let Some(nonce) = state.last_nonce {
        metrics.push(
            metric("ruuvi_gateway_nonce")
                .labels(&gw_labels)
                .value(nonce)
                .to_string(),
        );
    }

    // Tag metrics
    for (mac, tag) in &state.tags {
        let mut labels = labelset().label("mac", mac).label("gw_mac", &state.mac);

        if let Some(name) = names.lookup(mac) {
            labels = labels.label("name", name);
        }

        // Timestamps and sequence numbers
        metrics.push(
            metric("ruuvi_tag_last_seen_timestamp_seconds")
                .labels(&labels)
                .value(tag.last_seen.to_unix_seconds())
                .to_string(),
        );

        if let Some(sequence_number) = tag.values.measurement_sequence_number() {
            metrics.push(
                metric("ruuvi_tag_sequence_number")
                    .labels(&labels)
                    .value(sequence_number)
                    .to_string(),
            );
        }

        // Environmental measurements
        if let Some(temp_mc) = tag.values.temperature_as_millicelsius() {
            metrics.push(
                metric("ruuvi_tag_temperature_celsius")
                    .labels(&labels)
                    .value(f64::from(temp_mc) / 1000.0)
                    .to_string(),
            );
        }

        if let Some(humidity_ppm) = tag.values.humidity_as_ppm() {
            metrics.push(
                metric("ruuvi_tag_humidity_ratio")
                    .labels(&labels)
                    .value(f64::from(humidity_ppm) / 1e6)
                    .to_string(),
            );
        }

        if let Some(pressure) = tag.values.pressure_as_pascals() {
            metrics.push(
                metric("ruuvi_tag_pressure_pascals")
                    .labels(&labels)
                    .value(pressure)
                    .to_string(),
            );
        }
        // Movement and acceleration
        if let Some(moves) = tag.values.movement_counter() {
            metrics.push(
                metric("ruuvi_tag_movement_counter")
                    .labels(&labels)
                    .value(moves)
                    .to_string(),
            );
        }

        if let Some(acceleration) = tag.values.acceleration_vector_as_milli_g() {
            for (axis, value) in [
                ('x', acceleration.0),
                ('y', acceleration.1),
                ('z', acceleration.2),
            ] {
                metrics.push(
                    metric(&format!("ruuvi_tag_acceleration_{}_g", axis))
                        .labels(&labels)
                        .value(f64::from(value) / 1000.0)
                        .to_string(),
                );
            }
        }

        // Device status
        if let Some(battery_mv) = tag.values.battery_potential_as_millivolts() {
            metrics.push(
                metric("ruuvi_tag_battery_volts")
                    .labels(&labels)
                    .value(f64::from(battery_mv) / 1000.0)
                    .to_string(),
            );
        }

        if let Some(tx_power) = tag.values.tx_power_as_dbm() {
            metrics.push(
                metric("ruuvi_tag_tx_power_dBm")
                    .labels(&labels)
                    .value(tx_power)
                    .to_string(),
            );
        }

        // Signal strength
        metrics.push(
            metric("ruuvi_tag_rssi_dBm")
                .labels(&labels)
                .value(tag.rssi)
                .to_string(),
        );
    }

    metrics.join("\n") + "\n"
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
