use hifitime::Epoch;
use ruuvi_decoders::RuuviData;
use std::collections::HashMap;

use crate::rw_message::{AdMessageIter, TagMessage};

#[derive(Debug)]
pub struct Tag {
    pub last_seen: Epoch,
    pub rssi: i32,
    pub values: RuuviData,
}

pub struct Measurements {
    pub last_update: Epoch,
    pub last_nonce: Option<u64>,
    pub mac: String,
    pub tags: HashMap<String, Tag>,
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
        let msgs = AdMessageIter(&tag.data);

        // Find the last Ruuvi manufacturer-specific data (ad_type 0xff)
        // in case there are multiple advertisements
        let mut found_ruuvi = false;
        for msg in msgs
            .filter_map(Result::ok)
            .filter(|msg| msg.ad_type == 0xff)
        {
            if msg.payload.len() < 2 {
                continue;
            }
            let (manufacturer_id, payload) = msg.payload.split_at(2);
            let manufacturer_id = u16::from_le_bytes([manufacturer_id[0], manufacturer_id[1]]);
            // Ruuvi manufacturer ID is 0x0499
            if manufacturer_id == 0x0499 {
                found_ruuvi = true;
                if let Ok(values) = RuuviData::decode(payload) {
                    self.tags.insert(
                        tag.name.clone(),
                        Tag {
                            last_seen: tag.timestamp,
                            rssi: tag.rssi,
                            values,
                        },
                    );
                } else {
                    eprintln!(
                        "Warning: Could not parse Ruuvi data from tag {}: {}",
                        tag.name,
                        hex::encode_upper(&msg.payload)
                    );
                }
            }
        }

        if !found_ruuvi {
            eprintln!(
                "Warning: No Ruuvi manufacturer data found in advertisement from tag {}",
                tag.name,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hifitime::Epoch;

    #[test]
    fn test_update_tag_with_standard_format() {
        // Standard format: ad_type 1 followed by ad_type 0xff
        let data =
            hex::decode("0201061BFF9904050FE0337CC4ABFC1400340024A5B6EBA544DD1992CB6021").unwrap();
        let tag = TagMessage {
            name: "DD:19:92:CB:60:21".to_string(),
            data,
            timestamp: Epoch::from_unix_seconds(1736885086.0),
            rssi: -50,
        };

        let mut measurements = Measurements::new();
        measurements.update_tag(tag);

        assert_eq!(measurements.tags.len(), 1);
        assert!(measurements.tags.contains_key("DD:19:92:CB:60:21"));
    }

    #[test]
    fn test_update_tag_with_e1_format() {
        // E1 format is now supported by ruuvi-decoders
        let data =
            hex::decode("2BFF9904E110FE408CC53D000300060009000B02560D00FFFFFFFFFFFF001EBEB8FFFFFFFFFFF6BFB2EED156").unwrap();

        let tag = TagMessage {
            name: "E1:67:4C:F5:77:29".to_string(),
            data,
            timestamp: Epoch::from_unix_seconds(1736885086.0),
            rssi: -60,
        };

        let mut measurements = Measurements::new();
        measurements.update_tag(tag);

        // Tag should be added since E1 format is now supported
        assert_eq!(measurements.tags.len(), 1);
        assert!(measurements.tags.contains_key("E1:67:4C:F5:77:29"));

        // Verify it's E1 format
        let tag = measurements.tags.get("E1:67:4C:F5:77:29").unwrap();
        assert!(matches!(tag.values, RuuviData::E1(_)));
    }

    #[test]
    fn test_update_tag_without_manufacturer_data() {
        // Only ad_type 1, no manufacturer-specific data
        let data = hex::decode("020106").unwrap();
        let tag = TagMessage {
            name: "AA:BB:CC:DD:EE:FF".to_string(),
            data,
            timestamp: Epoch::from_unix_seconds(1736885086.0),
            rssi: -50,
        };

        let mut measurements = Measurements::new();
        measurements.update_tag(tag);

        // Tag should not be added since there's no manufacturer data
        assert_eq!(measurements.tags.len(), 0);
    }
}
