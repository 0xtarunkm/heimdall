use futures::Stream;
use serde::Deserialize;
use sqlx::{postgres::PgListener, Pool, Postgres, Row};
use std::pin::Pin;
use tokio::sync::mpsc;
use tonic::{transport::Server, Request, Response, Status};

mod proto {
    tonic::include_proto!("listing_stream");
}

use proto::listing_stream_server::{ListingStream, ListingStreamServer};

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct NotifyPayload {
    account: String,
    action: String,
}

#[derive(Debug, Clone)]
struct ListingStreamService {
    pool: Pool<Postgres>,
}

impl ListingStreamService {
    async fn new(database_url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let pool = sqlx::PgPool::connect(database_url).await?;
        Ok(Self { pool })
    }

    async fn start_listener(&self, tx: mpsc::Sender<Result<proto::StreamResponse, Status>>) {
        let mut listener = match PgListener::connect_with(&self.pool).await {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("Failed to create listener: {:?}", e);
                return;
            }
        };

        for channel in ["account_updates", "user_updates"] {
            if let Err(e) = listener.listen(channel).await {
                eprintln!("Failed to listen to channel {}: {:?}", channel, e);
                return;
            }
        }

        println!("Listening for updates...");

        while let Some(notification) = listener.recv().await.ok() {
            match serde_json::from_str::<NotifyPayload>(notification.payload()) {
                Ok(payload) => {
                    let result = match payload.action.as_str() {
                        "account_update" => self.fetch_listing(&payload.account).await
                            .map(|opt_listing| opt_listing.map(|l| 
                                proto::StreamResponse {
                                    update: Some(proto::stream_response::Update::Listing(l))
                                }
                            )),
                        "user_update" => self.fetch_user_assets(&payload.account).await
                            .map(|assets| Some(proto::StreamResponse {
                                update: Some(proto::stream_response::Update::UserAssets(assets))
                            })),
                        _ => {
                            eprintln!("Unknown action type: {}", payload.action);
                            Ok(None)
                        }
                    };

                    match result {
                        Ok(Some(response)) => {
                            if let Err(e) = tx.send(Ok(response)).await {
                                eprintln!("Failed to send update: {:?}", e);
                            }
                        }
                        Ok(None) => {
                            eprintln!("No data found for account: {}", payload.account);
                        }
                        Err(e) => eprintln!("Failed to fetch data: {:?}", e),
                    }
                }
                Err(e) => eprintln!("Failed to parse notification payload: {:?}", e),
            }
        }
    }

    async fn fetch_user_assets(&self, account: &str) -> Result<proto::UserAssets, sqlx::Error> {
        let table_name = format!("user_{}", account.replace(&['.' as char, '-' as char][..], "_"));
        
        let query = format!(
            r#"
            SELECT 
                CAST(sol_balance AS DOUBLE PRECISION) as sol_balance,
                token_holdings::text as token_holdings,
                nft_holdings::text as nft_holdings,
                timestamp::text as updated_at
            FROM {}
            ORDER BY timestamp DESC
            LIMIT 1
            "#,
            table_name
        );

        let record = sqlx::query(&query)
            .fetch_one(&self.pool)
            .await?;

        Ok(proto::UserAssets {
            address: account.to_string(),
            sol_balance: record.get("sol_balance"),
            token_holdings: record.get("token_holdings"),
            nft_holdings: record.get("nft_holdings"),
            updated_at: record.get("updated_at"),
        })
    }

    async fn fetch_listing(&self, account: &str) -> Result<Option<proto::Listing>, sqlx::Error> {
        let record = sqlx::query!(
            r#"
            SELECT 
                account,
                name,
                seed as "seed!: i64",
                mint,
                funding_goal as "funding_goal!: i64",
                pool_mint_supply::text,
                funding_raised as "funding_raised!: i64",
                available_tokens::text,
                base_price,
                tokens_sold::text,
                bump as "bump!: i16",
                vault_bump as "vault_bump!: i16",
                mint_bump as "mint_bump!: i16",
                updated_at::text
            FROM listings 
            WHERE account = $1
            "#,
            account
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(record.map(|r| proto::Listing {
            account: r.account,
            name: r.name,
            seed: r.seed as u64,
            mint: r.mint,
            funding_goal: r.funding_goal as u64,
            pool_mint_supply: r.pool_mint_supply.unwrap_or_default(),
            funding_raised: r.funding_raised as u64,
            available_tokens: r.available_tokens.unwrap_or_default(),
            base_price: r.base_price,
            tokens_sold: r.tokens_sold.unwrap_or_default(),
            bump: r.bump as u32,
            vault_bump: r.vault_bump as u32,
            mint_bump: r.mint_bump as u32,
            updated_at: r.updated_at.unwrap_or_default(),
        }))
    }
}

#[tonic::async_trait]
impl ListingStream for ListingStreamService {
    type StreamListingsStream =
        Pin<Box<dyn Stream<Item = Result<proto::StreamResponse, Status>> + Send + 'static>>;

    async fn stream_listings(
        &self,
        _request: Request<proto::StreamRequest>,
    ) -> Result<Response<Self::StreamListingsStream>, Status> {
        let (tx, rx) = mpsc::channel(100);

        let service = self.clone();

        tokio::spawn(async move {
            service.start_listener(tx).await;
        });

        let output_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(output_stream)))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let addr = "[::1]:50051".parse()?;
    let service = ListingStreamService::new(&database_url).await?;

    println!("Starting gRPC server on {}", addr);

    Server::builder()
        .add_service(ListingStreamServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
