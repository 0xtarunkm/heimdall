use anchor_lang::solana_program::clock::Slot;
use anchor_lang::{prelude::*, AnchorDeserialize};
use serde::{Deserialize, Serialize};
use solana_geyser_plugin_interface::geyser_plugin_interface::{
    GeyserPlugin, GeyserPluginError, ReplicaAccountInfoVersions, Result as PluginResult,
};
use spl_token::solana_program::program_pack::Pack;
use spl_token::solana_program::pubkey::Pubkey;
use spl_token::state::Account as TokenAccount;
use spl_token::ID as SPL_TOKEN_PROGRAM_ID;
use sqlx::{postgres::PgPoolOptions, Pool, Postgres, Row};
use std::{error::Error, fs::OpenOptions, io::Read};
use tokio::runtime::Runtime;

#[derive(Debug)]
pub struct Heimdall {
    db_pool: Option<Pool<Postgres>>,
    config: Option<Config>,
    programs: Vec<[u8; 32]>,
    runtime: Runtime,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub programs: Option<Vec<String>>,
    pub tracked_users: Option<Vec<String>>,
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
            runtime: Runtime::new().unwrap(),
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
        let pool = self.db_pool.as_ref().unwrap();

        // Create listings table
        let create_listings_result = self.runtime.block_on(async {
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

        if let Err(e) = create_listings_result {
            println!("Error creating listings table: {:?}", e);
        }

        // Create user tables for each tracked user
        if let Some(users) = &config.tracked_users {
            for user in users {
                let create_user_table = format!(
                    "CREATE TABLE IF NOT EXISTS user_{} (
                        timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                        sol_balance NUMERIC NOT NULL,
                        token_holdings JSONB,
                        nft_holdings JSONB
                    )",
                    user.replace(&['.' as char, '-' as char][..], "_")
                );

                let result = self
                    .runtime
                    .block_on(async { sqlx::query(&create_user_table).execute(pool).await });

                if let Err(e) = result {
                    println!("Error creating table for user {}: {:?}", user, e);
                }
            }
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
            ReplicaAccountInfoVersions::V0_0_1(_) | ReplicaAccountInfoVersions::V0_0_2(_) => {
                return Err(GeyserPluginError::AccountsUpdateError {
                    msg: "Unsupported version, please upgrade your Solana CLI version".to_string(),
                })
            }
            ReplicaAccountInfoVersions::V0_0_3(account_info) => account_info,
        };

        let account_pubkey = bs58::encode(account_info.pubkey).into_string();

        // Handle user account updates
        if let Some(tracked_users) = &self.config.as_ref().unwrap().tracked_users {
            if tracked_users.contains(&account_pubkey) {
                self.update_user_sol_balance(&account_pubkey, account_info.lamports)?;
            }

            // Handle token accounts owned by tracked users
            if let Ok(owner_pubkey) = Pubkey::try_from(account_info.owner) {
                if owner_pubkey == SPL_TOKEN_PROGRAM_ID {
                    if let Ok(token_account) = TokenAccount::unpack(&account_info.data) {
                        let owner = bs58::encode(token_account.owner).into_string();
                        if tracked_users.contains(&owner) {
                            let mint = bs58::encode(token_account.mint).into_string();
                            self.update_user_token_holding(&owner, &mint, token_account.amount)?;
                        }
                    }
                }
            }
        }

        // Handle program account updates
        self.programs.iter().for_each(|program| {
            if program == account_info.owner {
                if account_info.data.len() > 8 {
                    let mut account_data_slice = &account_info.data[8..];
                    if let Ok(anchor_listing) = AnchorListing::deserialize(&mut account_data_slice)
                    {
                        self.update_listing(&account_pubkey, anchor_listing);
                    }
                }
            }
        });

        Ok(())
    }
}

impl Heimdall {
    fn update_user_sol_balance(&self, user_pubkey: &str, lamports: u64) -> PluginResult<()> {
        let user_table = format!(
            "user_{}",
            user_pubkey.replace(&['.' as char, '-' as char][..], "_")
        );
        let sol_balance = lamports as f64 / 1_000_000_000.0;

        let query = format!(
            "INSERT INTO {} (sol_balance, token_holdings, nft_holdings) 
             VALUES ($1, 
                    COALESCE((SELECT token_holdings FROM {} ORDER BY timestamp DESC LIMIT 1), '[]'::jsonb),
                    COALESCE((SELECT nft_holdings FROM {} ORDER BY timestamp DESC LIMIT 1), '[]'::jsonb))",
            user_table, user_table, user_table
        );

        let _ = self.runtime.block_on(async {
            sqlx::query(&query)
                .bind(sol_balance)
                .execute(self.db_pool.as_ref().unwrap())
                .await
        });

        let notify_payload = serde_json::json!({
            "account": user_pubkey,
            "action": "user_update"
        })
        .to_string();

        let notify_result = self.runtime.block_on(async {
            sqlx::query("SELECT pg_notify('user_updates', $1)")
                .bind(&notify_payload)
                .execute(self.db_pool.as_ref().unwrap())
                .await
        });

        if let Err(e) = notify_result {
            println!("Failed to send user update notification: {:?}", e);
        }

        Ok(())
    }

    fn update_user_token_holding(
        &self,
        user_pubkey: &str,
        mint: &str,
        amount: u64,
    ) -> PluginResult<()> {
        let user_table = format!(
            "user_{}",
            user_pubkey.replace(&['.' as char, '-' as char][..], "_")
        );

        let query = format!(
            "SELECT token_holdings, nft_holdings FROM {} ORDER BY timestamp DESC LIMIT 1",
            user_table
        );

        let result = self.runtime.block_on(async {
            sqlx::query(&query)
                .fetch_optional(self.db_pool.as_ref().unwrap())
                .await
        });

        let (mut token_holdings, nft_holdings) = match result {
            Ok(Some(row)) => {
                let tokens: serde_json::Value = row
                    .try_get(0)
                    .map_err(|e| GeyserPluginError::Custom(Box::new(e)))?;
                let nfts: serde_json::Value = row
                    .try_get(1)
                    .map_err(|e| GeyserPluginError::Custom(Box::new(e)))?;
                (tokens, nfts)
            }
            _ => (serde_json::json!([]), serde_json::json!([])),
        };

        if let serde_json::Value::Array(ref mut tokens) = token_holdings {
            tokens.retain(|t| t["mint"] != mint);

            if amount > 0 {
                tokens.push(serde_json::json!({
                    "mint": mint,
                    "amount": amount,
                }));
            }
        }

        let update_query = format!(
            "INSERT INTO {} (sol_balance, token_holdings, nft_holdings) 
             VALUES (COALESCE((SELECT sol_balance FROM {} ORDER BY timestamp DESC LIMIT 1), 0),
                    $1, $2)",
            user_table, user_table
        );

        let result = self.runtime.block_on(async {
            sqlx::query(&update_query)
                .bind(token_holdings)
                .bind(nft_holdings)
                .execute(self.db_pool.as_ref().unwrap())
                .await
        });

        if let Err(e) = result {
            println!("Error updating token holdings: {:?}", e);
        } else {
            let notify_payload = serde_json::json!({
                "account": user_pubkey,
                "action": "user_update"
            })
            .to_string();

            let notify_result = self.runtime.block_on(async {
                sqlx::query("SELECT pg_notify('user_updates', $1)")
                    .bind(&notify_payload)
                    .execute(self.db_pool.as_ref().unwrap())
                    .await
            });

            if let Err(e) = notify_result {
                println!("Failed to send user update notification: {:?}", e);
            }
        }

        Ok(())
    }

    fn update_listing(&self, account_pubkey: &str, anchor_listing: AnchorListing) {
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

        let result = self.runtime.block_on(async {
            sqlx::query(listing_query)
                .bind(account_pubkey)
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
                .execute(self.db_pool.as_ref().unwrap())
                .await
        });

        match result {
            Ok(_) => {
                let notify_payload = serde_json::json!({
                    "account": account_pubkey,
                    "action": "account_update"
                })
                .to_string();

                let notify_result = self.runtime.block_on(async {
                    sqlx::query("SELECT pg_notify('account_updates', $1)")
                        .bind(&notify_payload)
                        .execute(self.db_pool.as_ref().unwrap())
                        .await
                });

                if let Err(e) = notify_result {
                    println!("Failed to send account update notification: {:?}", e);
                }
            }
            Err(e) => println!("Error inserting/updating listing: {:?}", e),
        }
    }
}
