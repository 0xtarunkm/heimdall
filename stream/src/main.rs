use futures::Stream;
use serde::Deserialize;
use sqlx::{postgres::PgListener, Pool, Postgres};
use std::pin::Pin;
use tokio::sync::mpsc;
use tonic::{transport::Server, Request, Response, Status};

mod proto {
    tonic::include_proto!("listing_stream");

    pub(crate) const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("listing_stream_descriptor");
}

use proto::listing_stream_server::{ListingStream, ListingStreamServer};

#[derive(Debug, Deserialize)]
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

    async fn start_listener(
        &self,
        tx: mpsc::Sender<Result<proto::Listing, Status>>,
    ) {
        let mut listener = match PgListener::connect_with(&self.pool).await {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("Failed to create listener: {:?}", e);
                return;
            }
        };

        if let Err(e) = listener.listen("account_updates").await {
            eprintln!("Failed to listen to channel: {:?}", e);
            return;
        }

        println!("Listening for account updates...");

        while let Some(notification) = listener.recv().await.ok() {
            match serde_json::from_str::<NotifyPayload>(notification.payload()) {
                Ok(payload) => {
                    match self.fetch_listing(&payload.account).await {
                        Ok(listing) => {
                            if let Err(e) = tx.send(Ok(listing)).await {
                                eprintln!("Failed to send listing update: {:?}", e);
                            }
                        }
                        Err(e) => eprintln!("Failed to fetch listing: {:?}", e),
                    }
                }
                Err(e) => eprintln!("Failed to parse notification payload: {:?}", e),
            }
        }
    }

    async fn fetch_listing(&self, account: &str) -> Result<proto::Listing, sqlx::Error> {
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
        .fetch_one(&self.pool)
        .await?;

        Ok(proto::Listing {
            account: record.account,
            name: record.name,
            seed: record.seed as u64,
            mint: record.mint,
            funding_goal: record.funding_goal as u64,
            pool_mint_supply: record.pool_mint_supply.unwrap_or_default(),
            funding_raised: record.funding_raised as u64,
            available_tokens: record.available_tokens.unwrap_or_default(),
            base_price: record.base_price,
            tokens_sold: record.tokens_sold.unwrap_or_default(),
            bump: record.bump as u32,
            vault_bump: record.vault_bump as u32,
            mint_bump: record.mint_bump as u32,
            updated_at: record.updated_at.unwrap_or_default(),
        })
    }
}

#[tonic::async_trait]
impl ListingStream for ListingStreamService {
    type StreamListingsStream = Pin<Box<dyn Stream<Item = Result<proto::Listing, Status>> + Send + 'static>>;

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

    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");

    let addr = "[::1]:50051".parse()?;
    let service = ListingStreamService::new(&database_url).await?;

    println!("Starting gRPC server on {}", addr);

    let reflection_service = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(proto::FILE_DESCRIPTOR_SET)
        .build()?;

    Server::builder()
        .add_service(ListingStreamServer::new(service))
        .add_service(reflection_service)
        .serve(addr)
        .await?;

    Ok(())
}
