use futures::StreamExt;
use proto::listing_stream_client::ListingStreamClient;
use std::error::Error;

pub mod proto {
    tonic::include_proto!("listing_stream");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let url = "http://[::1]:50051";
    let mut client = ListingStreamClient::connect(url).await?;

    let request = tonic::Request::new(proto::StreamRequest {
        update_type: "all".to_string(), // or specify the type of updates you want
    });

    let mut stream = client.stream_listings(request).await?.into_inner();

    println!("Connected to stream, waiting for updates...");

    while let Some(response) = stream.next().await {
        match response {
            Ok(stream_response) => match stream_response.update {
                Some(proto::stream_response::Update::Listing(listing)) => {
                    println!("Received listing update:");
                    println!("  Account: {}", listing.account);
                    println!("  Name: {}", listing.name);
                    println!("  Mint: {}", listing.mint);
                    println!("  Funding Goal: {}", listing.funding_goal);
                    println!("  Funding Raised: {}", listing.funding_raised);
                    println!("  Updated At: {}", listing.updated_at);
                    println!("-------------------");
                }
                Some(proto::stream_response::Update::UserAssets(assets)) => {
                    println!("Received user assets update:");
                    println!("  Address: {}", assets.address);
                    println!("  SOL Balance: {}", assets.sol_balance);
                    println!("  Updated At: {}", assets.updated_at);
                    println!("-------------------");
                }
                None => println!("Received empty update"),
            },
            Err(e) => println!("Error receiving update: {:?}", e),
        }
    }

    Ok(())
}
