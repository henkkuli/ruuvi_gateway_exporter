use hifitime::Epoch;
use ruuvi_sensor_protocol::SensorValues;
use std::collections::HashMap;

use crate::rw_message::{AdMessage, AdMessageIter, TagMessage};

#[derive(Debug)]
pub struct Tag {
    pub last_seen: Epoch,
    pub rssi: i32,
    pub values: SensorValues,
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
