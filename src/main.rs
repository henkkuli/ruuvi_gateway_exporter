use clap::Parser;
use hifitime::Epoch;
use parking_lot::Mutex;
use ruuvi_sensor_protocol::{
    Acceleration, BatteryPotential, Humidity, MeasurementSequenceNumber, MovementCounter, Pressure,
    SensorValues, Temperature, TransmitterPower,
};
use rw_message::{AdMessage, AdMessageIter, GwMessage, TagMessage};
use std::{collections::HashMap, net::IpAddr, sync::Arc};
use warp::{reply::Reply, Filter};

mod metrics;
mod rw_message;

use metrics::{labelset, metric};

#[derive(Debug)]
struct Tag {
    last_seen: Epoch,
    rssi: i32,
    values: SensorValues,
}

struct Measurements {
    last_update: Epoch,
    last_nonce: Option<u64>,
    mac: String,
    tags: HashMap<String, Tag>,
}

impl Measurements {
    pub fn new() -> Self {
        Self {
            last_update: hifitime::UNIX_REF_EPOCH, // Hopefully far enough in the history
            last_nonce: None,
            mac: String::new(),
            tags: Default::default(),
        }
    }

    pub fn update_tag(&mut self, tag: TagMessage) {
        let mut msgs = AdMessageIter(&tag.data);
        assert_eq!(
            msgs.next(),
            Some(Ok(AdMessage {
                ad_type: 1,
                payload: vec![6]
            }))
        );
        let data = msgs.next().unwrap().unwrap();
        assert_eq!(data.ad_type, 0xff);
        assert_eq!(msgs.next(), None);
        let (manufacturer_id, payload) = data.payload.split_at(2);
        let manufacturer_id = u16::from_le_bytes([manufacturer_id[0], manufacturer_id[1]]);
        let values =
            SensorValues::from_manufacturer_specific_data(manufacturer_id, payload).unwrap(); // TODO: Don'tag unwrap

        let t = Tag {
            last_seen: tag.timestamp,
            rssi: tag.rssi,
            values,
        };

        self.tags.insert(tag.name, t);
    }
}

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

fn collect_metrics(state: &Measurements) -> String {
    let mut metrics = Vec::new();

    // Gateway metrics
    metrics.push(
        metric("ruuvi_gateway_update_timestamp_seconds")
            .label("gw_mac", &state.mac)
            .value(state.last_update.to_unix_seconds())
            .to_string(),
    );

    if let Some(nonce) = state.last_nonce {
        metrics.push(
            metric("ruuvi_gateway_nonce")
                .label("gw_mac", &state.mac)
                .value(nonce)
                .to_string(),
        );
    }

    // Tag metrics
    for (name, tag) in &state.tags {
        let labels = labelset().label("mac", name).label("gw_mac", &state.mac);

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
        if let Some(temp_mk) = tag.values.temperature_as_millikelvins() {
            metrics.push(
                metric("ruuvi_tag_temperature_celsius")
                    .labels(&labels)
                    .value((f64::from(temp_mk) / 1000.0) - 273.15)
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
) -> impl Reply {
    let state = sensor_state.lock();
    collect_metrics(&state)
}

#[derive(Parser)]
#[command(version, about)]
struct Config {
    /// Port to listen on
    #[arg(short, long, default_value_t = 9000)]
    port: u16,

    /// Interface to bind to
    #[arg(short, long, default_value = "0.0.0.0")]
    interface: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let config = Config::parse();
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
        .map(metrics);

    println!("Starting server on {}:{}", config.interface, config.port);
    warp::serve(post_measurements.or(metrics))
        .run((config.interface.parse::<IpAddr>().unwrap(), config.port))
        .await;
}
