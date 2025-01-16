use std::{cell::RefCell, collections::HashMap, sync::Arc};

use hifitime::Epoch;
use parking_lot::Mutex;
use ruuvi_sensor_protocol::{
    Acceleration, BatteryPotential, Humidity, MeasurementSequenceNumber, MovementCounter, Pressure,
    SensorValues, Temperature, TransmitterPower,
};
use rw_message::{AdMessage, AdMessageIter, GwMessage, TagMessage};
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
    tags: HashMap<String, Tag>,
}

impl Measurements {
    pub fn new() -> Self {
        Self {
            last_update: hifitime::UNIX_REF_EPOCH, // Hopefully far enough in the history
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
    for tag in data.tags {
        state.update_tag(tag);
    }
    drop(state);

    warp::reply::with_header("", "X-Ruuvi-Gateway-Rate", "1")
}

fn metrics(
    sensor_state: Arc<parking_lot::lock_api::Mutex<parking_lot::RawMutex, Measurements>>,
) -> impl Reply {
    use std::fmt::Write;
    let mut res = String::new();

    let state = sensor_state.lock();
    for (name, tag) in &state.tags {
        let labels = labelset().label("mac", name);

        // Sequence number
        if let Some(sequence_number) = tag.values.measurement_sequence_number() {
            writeln!(
                &mut res,
                "{}",
                metric("ruuvi_sequence_number")
                    .labels(&labels)
                    .value(sequence_number)
            )
            .unwrap();
        }

        // Temperature (convert from millikelvin to celsius)
        if let Some(temp_mk) = tag.values.temperature_as_millikelvins() {
            let temp_celsius = (f64::from(temp_mk) / 1000.0) - 273.15;
            writeln!(
                &mut res,
                "{}",
                metric("ruuvi_temperature_celsius")
                    .labels(&labels)
                    .value(temp_celsius)
            )
            .unwrap();
        }

        // Humidity (convert from ppm to percentage)
        if let Some(humidity_ppm) = tag.values.humidity_as_ppm() {
            let humidity_percent = f64::from(humidity_ppm) / 1e6;
            writeln!(
                &mut res,
                "{}",
                metric("ruuvi_humidity_ratio")
                    .labels(&labels)
                    .value(humidity_percent)
            )
            .unwrap();
        }

        // Pressure (keep pascal)
        if let Some(pressure) = tag.values.pressure_as_pascals() {
            writeln!(
                &mut res,
                "{}",
                metric("ruuvi_pressure_pascals")
                    .labels(&labels)
                    .value(pressure)
            )
            .unwrap();
        }

        // Battery (convert from mV to V)
        if let Some(battery_mv) = tag.values.battery_potential_as_millivolts() {
            let battery_v = f64::from(battery_mv) / 1000.0;
            writeln!(
                &mut res,
                "{}",
                metric("ruuvi_battery_volts")
                    .labels(&labels)
                    .value(battery_v)
            )
            .unwrap();
        }

        // Tx Power (keep dBm)
        if let Some(tx_power) = tag.values.tx_power_as_dbm() {
            writeln!(
                &mut res,
                "{}",
                metric("ruuvi_tx_power_dBm").labels(&labels).value(tx_power)
            )
            .unwrap();
        }

        // Movement counter (unitless)
        if let Some(moves) = tag.values.movement_counter() {
            writeln!(
                &mut res,
                "{}",
                metric("ruuvi_movement_counter")
                    .labels(&labels)
                    .value(moves)
            )
            .unwrap();
        }

        // Acceleration
        if let Some(acceleration) = tag.values.acceleration_vector_as_milli_g() {
            writeln!(
                &mut res,
                "{}",
                metric("ruuvi_acceleration_x_g")
                    .labels(&labels)
                    .value(f64::from(acceleration.0) / 1000.0)
            )
            .unwrap();
            writeln!(
                &mut res,
                "{}",
                metric("ruuvi_acceleration_y_g")
                    .labels(&labels)
                    .value(f64::from(acceleration.1) / 1000.0)
            )
            .unwrap();
            writeln!(
                &mut res,
                "{}",
                metric("ruuvi_acceleration_z_g")
                    .labels(&labels)
                    .value(f64::from(acceleration.2) / 1000.0)
            )
            .unwrap();
        }

        // RSSI (keep dBm)
        writeln!(
            &mut res,
            "{}",
            metric("ruuvi_rssi_dBm").labels(&labels).value(tag.rssi)
        )
        .unwrap();
    }
    drop(state);

    res
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
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

    warp::serve(post_measurements.or(metrics))
        .run(([0, 0, 0, 0], 9000))
        .await;
}
