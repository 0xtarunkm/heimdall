use anyhow::{anyhow, Result};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;

pub struct ProgramMonitor {
    rpc_client: RpcClient,
}

impl ProgramMonitor {
    pub fn new() -> Self {
        let devnet_url = "https://api.devnet.solana.com";
        Self {
            rpc_client: RpcClient::new(devnet_url.to_string()),
        }
    }

    pub fn new_with_endpoint(endpoint: &str) -> Self {
        Self { rpc_client: RpcClient::new(endpoint.to_string()) }
    }

    pub async fn fetch_program_bytecode(&self, program_id: &Pubkey) -> Result<Vec<u8>> {
        let account = self
            .rpc_client
            .get_account(program_id)
            .map_err(|e| anyhow!("Failed to fetch account for program {}: {}", program_id, e))?;

        if !account.executable {
            return Err(anyhow!("Account {} is not an executable program", program_id));
        }
        
        if account.data.is_empty() {
            return Err(anyhow!("Program {} has no bytecode data", program_id));
        }

        println!(
            "Fetched {} bytes of program data for {}",
            account.data.len(),
            program_id
        );

        Ok(account.data)
    }

    pub async fn verify_program_exists(&self, program_id: &Pubkey) -> Result<bool> {
        match self.rpc_client.get_account(program_id) {
            Ok(account) => Ok(account.executable),
            Err(_) => Ok(false)
        }
    }
}

impl Default for ProgramMonitor {
    fn default() -> Self {
        Self::new()
    }
}
