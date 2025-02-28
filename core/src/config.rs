use serde::Deserialize;
use std::{error::Error, fs::OpenOptions, io::Read};

#[derive(Debug, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub programs: Option<Vec<String>>,
    pub tracked_users: Option<Vec<String>>,
}

impl Config {
    pub fn load(config_path: &str) -> std::result::Result<Self, Box<dyn Error>> {
        let mut file = OpenOptions::new().read(true).open(config_path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        Ok(serde_json::from_str::<Config>(&contents)?)
    }
}
