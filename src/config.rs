use clap::Parser;
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(version, about)]
pub struct Config {
    /// Port to listen on
    #[arg(short, long, default_value_t = 9000)]
    pub port: u16,

    /// Interface to bind to
    #[arg(short, long, default_value = "0.0.0.0")]
    pub interface: String,

    /// Path to YAML config file with MAC address mappings
    #[arg(short, long)]
    pub mac_mapping: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Default)]
pub struct MacMapping {
    #[serde(default, flatten)]
    names: HashMap<String, String>,
}

impl MacMapping {
    pub fn lookup(&self, mac: &str) -> Option<&str> {
        self.names.get(mac).map(|s| s.as_str())
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        Ok(serde_yaml::from_reader(reader)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_config(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{}", content).unwrap();
        file
    }

    #[test]
    fn test_default_config() {
        let config = Config::try_parse_from(["program"]).unwrap();
        assert_eq!(config.port, 9000);
        assert_eq!(config.interface, "0.0.0.0");
        assert!(config.mac_mapping.is_none());
    }

    #[test]
    fn test_custom_port_and_interface() {
        let config = Config::try_parse_from(["program", "-p", "8080", "-i", "127.0.0.1"]).unwrap();
        assert_eq!(config.port, 8080);
        assert_eq!(config.interface, "127.0.0.1");
    }

    #[test]
    fn test_custom_mac_mapping() {
        let mac_mapping_content = r#"
            "AA:BB:CC:DD:EE:FF": "Living Room"
            "11:22:33:44:55:66": "Kitchen"
        "#;
        let mac_mapping_path = create_temp_config(mac_mapping_content);
        let config = Config::try_parse_from([
            "program",
            "--mac-mapping",
            mac_mapping_path.path().to_str().unwrap(),
        ])
        .unwrap();

        let mapping = MacMapping::load(config.mac_mapping.unwrap()).unwrap();
        assert_eq!(mapping.lookup("AA:BB:CC:DD:EE:FF"), Some("Living Room"));
        assert_eq!(mapping.lookup("11:22:33:44:55:66"), Some("Kitchen"));
        assert_eq!(mapping.lookup("00:00:00:00:00:00"), None);
    }

    #[test]
    fn test_mac_mapping_parsing() {
        let mac_mapping_content = r#"
            "AA:BB:CC:DD:EE:FF": "Living Room"
            "11:22:33:44:55:66": "Kitchen"
        "#;
        let mac_mapping_path = create_temp_config(mac_mapping_content);

        let mapping = MacMapping::load(mac_mapping_path).unwrap();
        assert_eq!(mapping.lookup("AA:BB:CC:DD:EE:FF"), Some("Living Room"));
        assert_eq!(mapping.lookup("11:22:33:44:55:66"), Some("Kitchen"));
        assert_eq!(mapping.lookup("00:00:00:00:00:00"), None);
    }

    #[test]
    fn test_invalid_mac_mapping_file() {
        let mac_mapping_content = "invalid: yaml: content:";
        let mac_mapping_path = create_temp_config(mac_mapping_content);

        let result = MacMapping::load(&mac_mapping_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_mac_mapping() {
        let mac_mapping_content = "{}";
        let mac_mapping_path = create_temp_config(mac_mapping_content);

        let mapping = MacMapping::load(mac_mapping_path).unwrap();
        assert_eq!(mapping.lookup("any-mac"), None);
    }
}
