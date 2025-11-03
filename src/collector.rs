use crate::config::MacMapping;
use crate::measurements::Measurements;
use crate::metrics::{labelset, metric};
use ruuvi_sensor_protocol::{
    Acceleration, BatteryPotential, Humidity, MeasurementSequenceNumber, MovementCounter, Pressure,
    Temperature, TransmitterPower,
};

pub fn collect_metrics(state: &Measurements, names: &MacMapping) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measurements::Tag;
    use crate::rw_message::TagMessage;
    use hifitime::Epoch;

    #[test]
    fn test_collect_metrics_basic() {
        let mut measurements = Measurements::new();
        measurements.mac = "AA:BB:CC:DD:EE:FF".to_string();
        measurements.last_update = Epoch::from_unix_seconds(1234567890.0);

        let names = MacMapping::default();
        let output = collect_metrics(&measurements, &names);

        assert!(output.contains("ruuvi_gateway_update_timestamp_seconds"));
        assert!(output.contains("gw_mac=\"AA:BB:CC:DD:EE:FF\""));
        assert!(output.contains("1234567890"));
    }

    #[test]
    fn test_collect_metrics_with_tag() {
        let mut measurements = Measurements::new();
        measurements.mac = "AA:BB:CC:DD:EE:FF".to_string();
        measurements.last_update = Epoch::from_unix_seconds(1234567890.0);

        // Add a tag with data
        let data =
            hex::decode("0201061BFF9904050FE0337CC4ABFC1400340024A5B6EBA544DD1992CB6021").unwrap();
        let tag_msg = TagMessage {
            name: "DD:19:92:CB:60:21".to_string(),
            data,
            timestamp: Epoch::from_unix_seconds(1234567890.0),
            rssi: -50,
        };
        measurements.update_tag(tag_msg);

        let names = MacMapping::default();
        let output = collect_metrics(&measurements, &names);

        // Check tag metrics are present
        assert!(output.contains("ruuvi_tag_last_seen_timestamp_seconds"));
        assert!(output.contains("mac=\"DD:19:92:CB:60:21\""));
        assert!(output.contains("ruuvi_tag_temperature_celsius"));
        assert!(output.contains("ruuvi_tag_rssi_dBm"));
    }

    #[test]
    fn test_collect_metrics_with_names() {
        let mut measurements = Measurements::new();
        measurements.mac = "AA:BB:CC:DD:EE:FF".to_string();
        measurements.last_update = Epoch::from_unix_seconds(1234567890.0);

        // Create mapping with names
        let yaml = r#"
            "AA:BB:CC:DD:EE:FF": "Gateway 1"
        "#;
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        use std::io::Write;
        write!(temp_file, "{}", yaml).unwrap();
        let names = MacMapping::load(temp_file.path()).unwrap();

        let output = collect_metrics(&measurements, &names);

        assert!(output.contains("name=\"Gateway 1\""));
    }
}
