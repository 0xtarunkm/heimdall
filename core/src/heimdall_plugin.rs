use anchor_lang::solana_program::clock::Slot;
use anchor_lang::{prelude::*, AnchorDeserialize};
use serde::{Deserialize, Serialize};
use solana_geyser_plugin_interface::geyser_plugin_interface::{
    GeyserPlugin, GeyserPluginError, ReplicaAccountInfoVersions, Result as PluginResult,
};
use sqlx::{postgres::PgPoolOptions, Pool, Postgres};
use std::{error::Error, fs::OpenOptions, io::Read};
use tokio::runtime::Runtime;

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

// reads directly from solana account data
#[derive(Debug, AnchorDeserialize)]
pub struct AnchorListing {
    pub name: String,
    pub seed: u64,
    pub mint: Pubkey,
    pub funding_goal: u64,
    pub pool_mint_supply: u128,
    pub funding_raised: u64,
    pub available_tokens: u128,
    pub base_price: f64,
    pub tokens_sold: u128,
    pub bump: u8,
    pub vault_bump: u8,
    pub mint_bump: u8,
}

// database/JSON operations
#[derive(Debug, Serialize, Deserialize)]
pub struct Listing {
    pub name: String,
    pub seed: u64,
    pub mint: String,
    pub funding_goal: u64,
    pub pool_mint_supply: u128,
    pub funding_raised: u64,
    pub available_tokens: u128,
    pub base_price: f64,
    pub tokens_sold: u128,
    pub bump: u8,
    pub vault_bump: u8,
    pub mint_bump: u8,
}

impl Config {
    pub fn load(config_path: &str) -> std::result::Result<Self, Box<dyn Error>> {
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

    fn on_load(&mut self, config_file: &str, _is_reload: bool) -> PluginResult<()> {
        let config = match Config::load(config_file) {
            Ok(c) => c,
            Err(_e) => {
                return Err(GeyserPluginError::ConfigFileReadError {
                    msg: String::from("Error opening, or reading config file"),
                });
            }
        };

        let rt = Runtime::new().unwrap();
        let pool = rt
            .block_on(async {
                PgPoolOptions::new()
                    .max_connections(5)
                    .connect(&config.database_url)
                    .await
            })
            .map_err(|_e| GeyserPluginError::ConfigFileReadError {
                msg: String::from("Error connecting to local postgres database"),
            })?;

        self.db_pool = Some(pool);

        let rt = Runtime::new().unwrap();
        let pool = self.db_pool.as_ref().unwrap();

        let create_listings_result = rt.block_on(async {
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS listings (
                    account TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    seed BIGINT NOT NULL,
                    mint TEXT NOT NULL,
                    funding_goal BIGINT NOT NULL,
                    pool_mint_supply NUMERIC NOT NULL,
                    funding_raised BIGINT NOT NULL,
                    available_tokens NUMERIC NOT NULL,
                    base_price DOUBLE PRECISION NOT NULL,
                    tokens_sold NUMERIC NOT NULL,
                    bump SMALLINT NOT NULL,
                    vault_bump SMALLINT NOT NULL,
                    mint_bump SMALLINT NOT NULL,
                    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )",
            )
            .execute(pool)
            .await
        });

        match create_listings_result {
            Ok(_) => println!("Listings table created or already exists"),
            Err(e) => println!("Error creating listings table: {:?}", e),
        }

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
            ReplicaAccountInfoVersions::V0_0_2(_) => {
                return Err(GeyserPluginError::AccountsUpdateError {
                    msg: "V2 not supported, please upgrade your Solana CLI version".to_string(),
                })
            }
            ReplicaAccountInfoVersions::V0_0_3(account_info) => account_info,
        };

        self.programs.iter().for_each(|program| {
            if program == account_info.owner {
                let account_pubkey = bs58::encode(account_info.pubkey).into_string();
                let account_data = account_info.data;

                if account_data.len() > 8 {
                    let mut account_data_slice = &account_data[8..];
                    match AnchorListing::deserialize(&mut account_data_slice) {
                        Ok(anchor_listing) => {

                            let listing = Listing {
                                name: anchor_listing.name,
                                seed: anchor_listing.seed,
                                mint: bs58::encode(anchor_listing.mint).into_string(),
                                funding_goal: anchor_listing.funding_goal,
                                pool_mint_supply: anchor_listing.pool_mint_supply,
                                funding_raised: anchor_listing.funding_raised,
                                available_tokens: anchor_listing.available_tokens,
                                base_price: anchor_listing.base_price,
                                tokens_sold: anchor_listing.tokens_sold,
                                bump: anchor_listing.bump,
                                vault_bump: anchor_listing.vault_bump,
                                mint_bump: anchor_listing.mint_bump,
                            };

                            let listing_query = "INSERT INTO listings (
                                account, name, seed, mint, funding_goal, pool_mint_supply,
                                funding_raised, available_tokens, base_price, tokens_sold,
                                bump, vault_bump, mint_bump
                            ) VALUES ($1, $2, $3, $4, $5, CAST($6 AS NUMERIC), $7, CAST($8 AS NUMERIC), $9, CAST($10 AS NUMERIC), $11, $12, $13)
                            ON CONFLICT (account) DO UPDATE SET
                                name = EXCLUDED.name,
                                seed = EXCLUDED.seed,
                                mint = EXCLUDED.mint,
                                funding_goal = EXCLUDED.funding_goal,
                                pool_mint_supply = EXCLUDED.pool_mint_supply,
                                funding_raised = EXCLUDED.funding_raised,
                                available_tokens = EXCLUDED.available_tokens,
                                base_price = EXCLUDED.base_price,
                                tokens_sold = EXCLUDED.tokens_sold,
                                bump = EXCLUDED.bump,
                                vault_bump = EXCLUDED.vault_bump,
                                mint_bump = EXCLUDED.mint_bump,
                                updated_at = CURRENT_TIMESTAMP";

                            let rt = Runtime::new().unwrap();
                            let pool = self.db_pool.as_ref().unwrap();
                            let result = rt.block_on(async {
                                sqlx::query(listing_query)
                                    .bind(&account_pubkey)
                                    .bind(&listing.name)
                                    .bind(listing.seed as i64)
                                    .bind(&listing.mint)
                                    .bind(listing.funding_goal as i64)
                                    .bind(listing.pool_mint_supply.to_string())
                                    .bind(listing.funding_raised as i64)
                                    .bind(listing.available_tokens.to_string())
                                    .bind(listing.base_price)
                                    .bind(listing.tokens_sold.to_string())
                                    .bind(listing.bump as i16)
                                    .bind(listing.vault_bump as i16)
                                    .bind(listing.mint_bump as i16)
                                    .execute(pool)
                                    .await
                            });

                            match result {
                                Ok(_) => {
                                    println!("Successfully inserted/updated listing for account: {}", account_pubkey);
                                    let notify_payload = serde_json::json!({
                                        "account": account_pubkey,
                                        "action": "update"
                                    }).to_string();

                                    let rt = Runtime::new().unwrap();
                                    let pool = self.db_pool.as_ref().unwrap();
                                    
                                    let notify_result = rt.block_on(async {
                                        sqlx::query("SELECT pg_notify('account_updates', $1)")
                                            .bind(&notify_payload)
                                            .execute(pool)
                                            .await
                                    });

                                    if let Err(e) = notify_result {
                                        println!("Failed to send notification: {:?}", e);
                                    }
                                },
                                Err(e) => println!("Error inserting/updating listing: {:?}", e),
                            }
                        },
                        Err(e) => {
                            println!("Failed to deserialize account as Listing: {:?}", e);
                        }
                    }
                }
            }
        });
        Ok(())
    }
}
