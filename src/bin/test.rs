use anyhow::Result;
use atspi::{events::ObjectEvents, AccessibilityConnection};
use futures_util::StreamExt;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Connecting to AT-SPI...");

    let connection = AccessibilityConnection::new().await?;
    connection.register_event::<ObjectEvents>().await?;

    println!("Listening for accessibility events...");

    let mut events = Box::pin(connection.event_stream());

    while let Some(event) = events.next().await {
        match event {
            Ok(event) => {
                let output = format!("{event:#?}");

                if output.contains("TextChanged")
                    || output.contains("CaretMoved")
                    || output.contains("accessible-name")
                {
                    println!("{output}");
                }
            }
            Err(error) => eprintln!("AT-SPI error: {error}"),
        }
    }

    Ok(())
}
