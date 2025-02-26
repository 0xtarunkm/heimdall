use anchor_lang::solana_program::clock::Slot;
use sqlx::{postgres::PgPoolOptions, Pool, Postgres};
use solana_geyser_plugin_interface::geyser_plugin_interface::{
    GeyserPlugin, GeyserPluginError, ReplicaAccountInfoVersions, Result as PluginResult,
};
use std::{error::Error, fs::OpenOptions, io::Read};
use tokio::runtime::Runtime;
use serde::Deserialize;

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

    fn on_load(
        &mut self,
        config_file: &str,
        _is_reload: bool,
    ) -> PluginResult<()> {
        println!("config file: {}", config_file);
        let config = match Config::load(config_file) {
            Ok(c) => c,
            Err(_e) => {
                return Err(GeyserPluginError::ConfigFileReadError {
                    msg: String::from("Error opening, or reading config file"),
                });
            }
        };
        println!("Your database url: {:#?}", &config.database_url);

        let rt = Runtime::new().unwrap();
        let pool = rt.block_on(async {
            PgPoolOptions::new()
                .max_connections(5)
                .connect(&config.database_url)
                .await
        })
        .map_err(|_e| GeyserPluginError::ConfigFileReadError {
            msg: String::from("Error connecting to local postgres database"),
        })?;

        self.db_pool = Some(pool);

        if let Some(accounts) = config.programs.as_ref() {
            accounts.iter().for_each(|account| {
                let mut acc_bytes = [0u8; 32];
                acc_bytes.copy_from_slice(&bs58::decode(account).into_vec().unwrap()[0..32]);
                self.programs.push(acc_bytes);
            });
        }
        self.config = Some(config);
        Ok(())
    }

    fn on_unload(&mut self) {}

    fn update_account(
        &self,
        account: ReplicaAccountInfoVersions,
        _slot: Slot,
        _is_startup: bool,
    ) -> PluginResult<()> {
        let account_info = match account {
            ReplicaAccountInfoVersions::V0_0_1(_) => {
                return Err(GeyserPluginError::AccountsUpdateError {
                    msg: "V1 not supported, please upgrade your Solana CLI version".to_string(),
                })
            }
            ReplicaAccountInfoVersions::V0_0_2(account_info) => account_info,
            ReplicaAccountInfoVersions::V0_0_3(_) => {
                return Err(GeyserPluginError::AccountsUpdateError {
                    msg: "V3 not supported, please upgrade your Solana CLI version".to_string(),
                })
            }
        };

        self.programs.iter().for_each(|program| {
            if program == account_info.owner {
                let account_pubkey = bs58::encode(account_info.pubkey).into_string();
                let account_owner = bs58::encode(account_info.owner).into_string();
                let account_data = account_info.data;
                let account_executable = account_info.executable;

                let query = "INSERT INTO accounts (account, owner, data, executable) \
                             VALUES ($1, $2, $3, $4) \
                             ON CONFLICT (account) DO UPDATE SET \
                             owner = EXCLUDED.owner, data = EXCLUDED.data, \
                             executable = EXCLUDED.executable";

                let rt = Runtime::new().unwrap();
                let pool = self.db_pool.as_ref().unwrap();
                let result = rt.block_on(async {
                    sqlx::query(query)
                        .bind(&account_pubkey)
                        .bind(&account_owner)
                        .bind(&account_data)
                        .bind(&account_executable)
                        .execute(pool)
                        .await
                });
                println!("Insert/Upsert result: {:?}", result);
            }
        });
        Ok(())
    }
}
