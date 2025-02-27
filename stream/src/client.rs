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
        program_ids: vec![],
    });

    let mut stream = client.stream_listings(request).await?.into_inner();

    while let Some(listing) = stream.next().await {
        match listing {
            Ok(listing) => println!("Received listing update: {:?}", listing),
            Err(e) => println!("Error receiving listing: {:?}", e),
        }
    }

    Ok(())
}
