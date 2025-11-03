use std::{collections::HashMap, fmt};

use hex::FromHexError;
use hifitime::{Duration, Epoch};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TagMessage {
    pub name: String,
    pub data: Vec<u8>,
    pub timestamp: Epoch,
    pub rssi: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "RawGwWrapper")]
pub struct GwMessage {
    pub coordinates: String,
    pub timestamp: Epoch,
    pub nonce: u64,
    pub gw_mac: String,
    pub tags: Vec<TagMessage>,
}

// Raw messages as they are sent over HTTP

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct RawTagMessage {
    pub data: String,
    pub timestamp: u64,
    pub rssi: i32,
}

impl TryFrom<(String, RawTagMessage)> for TagMessage {
    type Error = FromHexError;

    fn try_from((name, msg): (String, RawTagMessage)) -> Result<Self, Self::Error> {
        Ok(TagMessage {
            name,
            data: hex::decode(msg.data)?,
            timestamp: unix_timestamp_to_epoch(msg.timestamp),
            rssi: msg.rssi,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RawGwMessage {
    pub coordinates: String,
    pub timestamp: u64,
    pub nonce: u64,
    pub gw_mac: String,
    pub tags: HashMap<String, RawTagMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RawGwWrapper {
    pub data: RawGwMessage,
}

impl TryFrom<RawGwWrapper> for GwMessage {
    type Error = FromHexError;
    fn try_from(wrapper: RawGwWrapper) -> Result<Self, Self::Error> {
        let data = wrapper.data;

        let tags: Result<Vec<TagMessage>, FromHexError> =
            data.tags.into_iter().map(TryFrom::try_from).collect();

        Ok(GwMessage {
            coordinates: data.coordinates,
            timestamp: unix_timestamp_to_epoch(data.timestamp),
            nonce: data.nonce,
            gw_mac: data.gw_mac,
            tags: tags?,
        })
    }
}

fn unix_timestamp_to_epoch(unix_timestamp: u64) -> Epoch {
    Epoch::from_unix_duration(Duration::compose(1, 0, 0, 0, unix_timestamp, 0, 0, 0))
}

// Parsing of bluetooth advertising data

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AdMessage {
    pub ad_type: u8,
    pub payload: Vec<u8>,
}

#[derive(Error, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AdMessageParseError;

impl fmt::Display for AdMessageParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Failed to parse BLE advertisement")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AdMessageIter<'d>(pub &'d [u8]);

impl Iterator for AdMessageIter<'_> {
    type Item = Result<AdMessage, AdMessageParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0 {
            [] => None,
            &[len, ad_type, ..] => {
                let tail = &self.0[1..]; // Skip len byte
                if tail.len() < len as usize {
                    // Message is shorter than requested
                    self.0 = &[];
                    Some(Err(AdMessageParseError))
                } else {
                    let (payload, tail) = tail[1..].split_at((len - 1) as usize);
                    self.0 = tail;
                    Some(Ok(AdMessage {
                        ad_type,
                        payload: payload.to_vec(),
                    }))
                }
            }
            _ => Some(Err(AdMessageParseError)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::rw_message::{AdMessage, AdMessageIter};

    use super::GwMessage;

    #[test]
    fn gw_message_parsing() {
        // Example message captured from Ruuvi Gateway
        let raw = r#"{"data":{"coordinates":"","gw_mac":"FF:81:4E:A5:22:E7","nonce":3267643756,"tags":{"DD:19:92:CB:60:21":{"data":"0201061BFF9904050FE0337CC4ABFC1400340024A5B6EBA544DD1992CB6021","rssi":-50,"timestamp":1736885086},"DE:4F:BC:29:EC:B5":{"data":"0201061BFF9904050FF33391C47D0008FFF403F8837637EE6EDE4FBC29ECB5","rssi":-63,"timestamp":1736885085}},"timestamp":1736885086}}"#;
        let _: GwMessage = serde_json::from_str(raw).unwrap();
    }

    #[test]
    fn ad_message_iter() {
        let data =
            hex::decode("0201061BFF9904050FE0337CC4ABFC1400340024A5B6EBA544DD1992CB6021").unwrap();
        let mut iter = AdMessageIter(&data);
        println!("{iter:?}");
        assert_eq!(
            iter.next(),
            Some(Ok(AdMessage {
                ad_type: 1,
                payload: vec![6]
            }))
        );
        println!("{iter:?}");
        assert_eq!(
            iter.next(),
            Some(Ok(AdMessage {
                ad_type: 0xff,
                payload: hex::decode("9904050FE0337CC4ABFC1400340024A5B6EBA544DD1992CB6021")
                    .unwrap()
            }))
        );
        assert_eq!(iter.next(), None);
    }
}
