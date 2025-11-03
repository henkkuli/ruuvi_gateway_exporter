use crate::config::MacMapping;
use crate::measurements::Measurements;
use crate::metrics::{labelset, metric, LabelSet};

// Helper functions for metric collection
fn add_metric<T: std::fmt::Display>(
    metrics: &mut Vec<String>,
    name: &str,
    labels: &LabelSet,
    value: T,
) {
    metrics.push(metric(name).labels(labels).value(value).to_string());
}

fn add_optional_metric<T: std::fmt::Display>(
    metrics: &mut Vec<String>,
    name: &str,
    labels: &LabelSet,
    value: Option<T>,
) {
    if let Some(v) = value {
        add_metric(metrics, name, labels, v);
    }
}

fn add_common_environmental_metrics(
    metrics: &mut Vec<String>,
    labels: &LabelSet,
    measurement_sequence: Option<u32>,
    temperature: Option<f64>,
    humidity: Option<f64>,
    pressure: Option<f64>,
) {
    add_optional_metric(
        metrics,
        "ruuvi_tag_sequence_number",
        labels,
        measurement_sequence,
    );
    add_optional_metric(
        metrics,
        "ruuvi_tag_temperature_celsius",
        labels,
        temperature,
    );
    add_optional_metric(
        metrics,
        "ruuvi_tag_humidity_ratio",
        labels,
        humidity.map(|h| h / 100.0),
    );
    add_optional_metric(metrics, "ruuvi_tag_pressure_pascals", labels, pressure);
}

fn add_air_quality_metrics(
    metrics: &mut Vec<String>,
    labels: &LabelSet,
    pm2_5: Option<f64>,
    co2: Option<u16>,
    voc_index: Option<u16>,
    nox_index: Option<u16>,
    luminosity: Option<f64>,
) {
    add_optional_metric(metrics, "ruuvi_tag_pm2_5_ugm3", labels, pm2_5);
    add_optional_metric(metrics, "ruuvi_tag_co2_ppm", labels, co2);
    add_optional_metric(metrics, "ruuvi_tag_voc_index", labels, voc_index);
    add_optional_metric(metrics, "ruuvi_tag_nox_index", labels, nox_index);
    add_optional_metric(metrics, "ruuvi_tag_luminosity_lux", labels, luminosity);
}

pub fn collect_metrics(state: &Measurements, names: &MacMapping) -> String {
    let mut metrics = Vec::new();

    // Gateway metrics with optional name
    let mut gw_labels = labelset().label("gw_mac", &state.mac);
    if let Some(name) = names.lookup(&state.mac) {
        gw_labels = gw_labels.label("name", name);
    }

    add_metric(
        &mut metrics,
        "ruuvi_gateway_update_timestamp_seconds",
        &gw_labels,
        state.last_update.to_unix_seconds(),
    );

    add_optional_metric(
        &mut metrics,
        "ruuvi_gateway_nonce",
        &gw_labels,
        state.last_nonce,
    );

    // Tag metrics - iterate in sorted order for consistent output
    let mut sorted_tags: Vec<_> = state.tags.iter().collect();
    sorted_tags.sort_by_key(|(mac, _)| *mac);

    for (mac, tag) in sorted_tags {
        let mut labels = labelset().label("mac", mac).label("gw_mac", &state.mac);

        if let Some(name) = names.lookup(mac) {
            labels = labels.label("name", name);
        }

        // Timestamps
        add_metric(
            &mut metrics,
            "ruuvi_tag_last_seen_timestamp_seconds",
            &labels,
            tag.last_seen.to_unix_seconds(),
        );

        // Extract data based on format
        match &tag.values {
            ruuvi_decoders::RuuviData::V5(data) => {
                add_common_environmental_metrics(
                    &mut metrics,
                    &labels,
                    data.measurement_sequence.map(|s| s as u32),
                    data.temperature,
                    data.humidity,
                    data.pressure, // TODO: The doc says that it should be in hPa, but in actuality is it in Pa.
                );

                // Movement and acceleration
                add_optional_metric(
                    &mut metrics,
                    "ruuvi_tag_movement_counter",
                    &labels,
                    data.movement_counter,
                );

                if let (Some(x), Some(y), Some(z)) = (
                    data.acceleration_x,
                    data.acceleration_y,
                    data.acceleration_z,
                ) {
                    for (axis, value) in [('x', x), ('y', y), ('z', z)] {
                        add_metric(
                            &mut metrics,
                            &format!("ruuvi_tag_acceleration_{}_g", axis),
                            &labels,
                            f64::from(value) / 1000.0,
                        );
                    }
                }

                // Device status
                add_optional_metric(
                    &mut metrics,
                    "ruuvi_tag_battery_volts",
                    &labels,
                    data.battery_voltage.map(|v| f64::from(v) / 1000.0),
                );

                add_optional_metric(
                    &mut metrics,
                    "ruuvi_tag_tx_power_dBm",
                    &labels,
                    data.tx_power,
                );
            }
            ruuvi_decoders::RuuviData::V6(data) => {
                add_common_environmental_metrics(
                    &mut metrics,
                    &labels,
                    data.measurement_sequence.map(|s| s as u32),
                    data.temperature,
                    data.humidity,
                    data.pressure.map(|p| p * 100.0),
                );

                add_air_quality_metrics(
                    &mut metrics,
                    &labels,
                    data.pm2_5,
                    data.co2,
                    data.voc_index,
                    data.nox_index,
                    data.luminosity,
                );
            }
            ruuvi_decoders::RuuviData::E1(data) => {
                add_common_environmental_metrics(
                    &mut metrics,
                    &labels,
                    data.measurement_sequence,
                    data.temperature,
                    data.humidity,
                    data.pressure.map(|p| p * 100.0),
                );

                // E1-specific PM metrics
                add_optional_metric(&mut metrics, "ruuvi_tag_pm1_0_ugm3", &labels, data.pm1_0);
                add_optional_metric(&mut metrics, "ruuvi_tag_pm4_0_ugm3", &labels, data.pm4_0);
                add_optional_metric(&mut metrics, "ruuvi_tag_pm10_0_ugm3", &labels, data.pm10_0);

                add_air_quality_metrics(
                    &mut metrics,
                    &labels,
                    data.pm2_5,
                    data.co2,
                    data.voc_index,
                    data.nox_index,
                    data.luminosity,
                );
            }
        }

        // Signal strength
        add_metric(&mut metrics, "ruuvi_tag_rssi_dBm", &labels, tag.rssi);
    }

    metrics.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn test_collect_metrics_full_output() {
        // This test validates the complete output format to ensure refactoring
        // doesn't accidentally change the behavior of the system
        let mut measurements = Measurements::new();
        measurements.mac = "AA:BB:CC:DD:EE:FF".to_string();
        measurements.last_update = Epoch::from_unix_seconds(1609459200.0); // 2021-01-01 00:00:00 UTC
        measurements.last_nonce = Some(42);

        // Add a tag with complete data using RuuviTag format v5
        // Data format: 0x05 | temp | humidity | pressure | accel_x | accel_y | accel_z | battery+power | movement | sequence
        let data =
            hex::decode("0201061BFF9904050FE0337CC4ABFC1400340024A5B6EBA544DD1992CB6021").unwrap();
        let tag_msg = TagMessage {
            name: "DD:19:92:CB:60:21".to_string(),
            data,
            timestamp: Epoch::from_unix_seconds(1609459210.0), // 10 seconds after gateway
            rssi: -55,
        };
        measurements.update_tag(tag_msg);

        // Add an E1 sensor with air quality data
        let e1_data =
            hex::decode("2BFF9904E1170C5668C79E0065007004BD11CA00C90A0213E0ACFFFFFFDECDEE10FFFFFFFFFFCBB8334C884F").unwrap();
        let e1_tag_msg = TagMessage {
            name: "CB:B8:33:4C:88:4F".to_string(),
            data: e1_data,
            timestamp: Epoch::from_unix_seconds(1609459220.0), // 20 seconds after gateway
            rssi: -65,
        };
        measurements.update_tag(e1_tag_msg);

        // Create mapping with names
        let yaml = r#"
            "AA:BB:CC:DD:EE:FF": "Test Gateway"
            "DD:19:92:CB:60:21": "Living Room"
            "CB:B8:33:4C:88:4F": "Office"
        "#;
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        use std::io::Write;
        write!(temp_file, "{}", yaml).unwrap();
        let names = MacMapping::load(temp_file.path()).unwrap();

        let output = collect_metrics(&measurements, &names);

        // Expected output (order and exact format matter for this test)
        let expected = r#"ruuvi_gateway_update_timestamp_seconds{gw_mac="AA:BB:CC:DD:EE:FF",name="Test Gateway"} 1609459200
ruuvi_gateway_nonce{gw_mac="AA:BB:CC:DD:EE:FF",name="Test Gateway"} 42
ruuvi_tag_last_seen_timestamp_seconds{mac="CB:B8:33:4C:88:4F",gw_mac="AA:BB:CC:DD:EE:FF",name="Office"} 1609459220
ruuvi_tag_sequence_number{mac="CB:B8:33:4C:88:4F",gw_mac="AA:BB:CC:DD:EE:FF",name="Office"} 14601710
ruuvi_tag_temperature_celsius{mac="CB:B8:33:4C:88:4F",gw_mac="AA:BB:CC:DD:EE:FF",name="Office"} 29.5
ruuvi_tag_humidity_ratio{mac="CB:B8:33:4C:88:4F",gw_mac="AA:BB:CC:DD:EE:FF",name="Office"} 0.553
ruuvi_tag_pressure_pascals{mac="CB:B8:33:4C:88:4F",gw_mac="AA:BB:CC:DD:EE:FF",name="Office"} 101102
ruuvi_tag_pm1_0_ugm3{mac="CB:B8:33:4C:88:4F",gw_mac="AA:BB:CC:DD:EE:FF",name="Office"} 10.100000000000001
ruuvi_tag_pm4_0_ugm3{mac="CB:B8:33:4C:88:4F",gw_mac="AA:BB:CC:DD:EE:FF",name="Office"} 121.30000000000001
ruuvi_tag_pm10_0_ugm3{mac="CB:B8:33:4C:88:4F",gw_mac="AA:BB:CC:DD:EE:FF",name="Office"} 455.40000000000003
ruuvi_tag_pm2_5_ugm3{mac="CB:B8:33:4C:88:4F",gw_mac="AA:BB:CC:DD:EE:FF",name="Office"} 11.200000000000001
ruuvi_tag_co2_ppm{mac="CB:B8:33:4C:88:4F",gw_mac="AA:BB:CC:DD:EE:FF",name="Office"} 201
ruuvi_tag_voc_index{mac="CB:B8:33:4C:88:4F",gw_mac="AA:BB:CC:DD:EE:FF",name="Office"} 20
ruuvi_tag_nox_index{mac="CB:B8:33:4C:88:4F",gw_mac="AA:BB:CC:DD:EE:FF",name="Office"} 4
ruuvi_tag_luminosity_lux{mac="CB:B8:33:4C:88:4F",gw_mac="AA:BB:CC:DD:EE:FF",name="Office"} 13027
ruuvi_tag_rssi_dBm{mac="CB:B8:33:4C:88:4F",gw_mac="AA:BB:CC:DD:EE:FF",name="Office"} -65
ruuvi_tag_last_seen_timestamp_seconds{mac="DD:19:92:CB:60:21",gw_mac="AA:BB:CC:DD:EE:FF",name="Living Room"} 1609459210
ruuvi_tag_sequence_number{mac="DD:19:92:CB:60:21",gw_mac="AA:BB:CC:DD:EE:FF",name="Living Room"} 42308
ruuvi_tag_temperature_celsius{mac="DD:19:92:CB:60:21",gw_mac="AA:BB:CC:DD:EE:FF",name="Living Room"} 20.32
ruuvi_tag_humidity_ratio{mac="DD:19:92:CB:60:21",gw_mac="AA:BB:CC:DD:EE:FF",name="Living Room"} 0.3295
ruuvi_tag_pressure_pascals{mac="DD:19:92:CB:60:21",gw_mac="AA:BB:CC:DD:EE:FF",name="Living Room"} 100347
ruuvi_tag_movement_counter{mac="DD:19:92:CB:60:21",gw_mac="AA:BB:CC:DD:EE:FF",name="Living Room"} 235
ruuvi_tag_acceleration_x_g{mac="DD:19:92:CB:60:21",gw_mac="AA:BB:CC:DD:EE:FF",name="Living Room"} -1.004
ruuvi_tag_acceleration_y_g{mac="DD:19:92:CB:60:21",gw_mac="AA:BB:CC:DD:EE:FF",name="Living Room"} 0.052
ruuvi_tag_acceleration_z_g{mac="DD:19:92:CB:60:21",gw_mac="AA:BB:CC:DD:EE:FF",name="Living Room"} 0.036
ruuvi_tag_battery_volts{mac="DD:19:92:CB:60:21",gw_mac="AA:BB:CC:DD:EE:FF",name="Living Room"} 2.925
ruuvi_tag_tx_power_dBm{mac="DD:19:92:CB:60:21",gw_mac="AA:BB:CC:DD:EE:FF",name="Living Room"} 4
ruuvi_tag_rssi_dBm{mac="DD:19:92:CB:60:21",gw_mac="AA:BB:CC:DD:EE:FF",name="Living Room"} -55
"#;

        assert_eq!(output, expected, "Output format has changed!");
    }
}
