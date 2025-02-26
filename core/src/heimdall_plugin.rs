use std::{error::Error, fs::OpenOptions, io::Read};

use serde::Deserialize;
use solana_geyser_plugin_interface::geyser_plugin_interface::GeyserPlugin;
use sqlx::{Pool, Postgres};

#[derive(Debug)]
pub struct Heimdall {
    db_pool: Option<Pool<Postgres>>,
    config: Option<Config>,
    programs: Vec<[u8; 32]>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub programs: Option<Vec<String>>,
}

impl Config {
    pub fn load(config_path: &str) -> Result<Self, Box<dyn Error>> {
        let mut file = OpenOptions::new().read(true).open(config_path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        Ok(serde_json::from_str::<Config>(&contents)?)
    }
}

impl Default for Heimdall {
    fn default() -> Self {
        Heimdall {
            db_pool: None,
            config: None,
            programs: Vec::new(),
        }
    }
}

impl GeyserPlugin for Heimdall {
    fn name(&self) -> &'static str {
        "Heimdall"
    }
}
