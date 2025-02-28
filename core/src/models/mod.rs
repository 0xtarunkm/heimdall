use anchor_lang::{prelude::*, AnchorDeserialize};
use serde::{Deserialize, Serialize};

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
