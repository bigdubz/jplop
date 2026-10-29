use anyhow::Result;
use atspi::proxy::proxy_ext::ProxyExt;
use atspi::{
    connection::P2P,
    events::{FocusEvents, ObjectEvents},
    AccessibilityConnection,
};
use futures_util::stream::StreamExt;

#[tokio::main]
async fn main() -> Result<()> {
    let connection = AccessibilityConnection::new().await?;

    connection.register_event::<ObjectEvents>().await?;
    connection.register_event::<FocusEvents>().await?;

    let mut events = Box::pin(connection.event_stream());
    let mut focused_object: Option<atspi::ObjectRefOwned> = None;

    println!("Listening for focus and text changes...");

    while let Some(event) = events.next().await {
        let Ok(event) = event else {
            continue;
        };

        match event {
            atspi::Event::Object(atspi::events::ObjectEvents::StateChanged(event))
                if event.state == atspi::State::Focused =>
            {
                if event.enabled {
                    focused_object = Some(event.item.clone());

                    match connection.object_as_accessible(&event.item).await {
                        Ok(accessible) => {
                            println!("FOCUSED OBJECT: {:?}", event.item);
                            println!("  name: {:?}", accessible.name().await);
                            println!("  role: {:?}", accessible.get_role().await);
                        }
                        Err(error) => {
                            eprintln!("Could not inspect focused object: {error}");
                        }
                    }
                } else if focused_object.as_ref() == Some(&event.item) {
                    focused_object = None;
                }
            }

            atspi::Event::Object(atspi::events::ObjectEvents::TextChanged(event)) => {
                let Some(focused) = focused_object.as_ref() else {
                    continue;
                };

                // Ignore events from other application connections.
                if event.item.name() != focused.name() {
                    continue;
                }

                // Read the text from the object that actually emitted the event.
                let Ok(accessible) = connection.object_as_accessible(&event.item).await else {
                    continue;
                };

                let Ok(proxies) = accessible.proxies().await else {
                    continue;
                };

                let Ok(text) = proxies.text().await else {
                    continue;
                };

                let Ok(count) = text.character_count().await else {
                    continue;
                };

                match text.get_text(0, count).await {
                    Ok(contents) => {
                        if contents == "\u{feff}\n".to_string() {
                            println!("Current text: \"\"");
                        } else {
                            println!("Current text: {contents:?}");
                        }
                    }
                    Err(error) => eprintln!("Could not read text: {error}"),
                }
            }
            _ => {}
        }
    }

    Ok(())
}

async fn print_ancestry(connection: &atspi::AccessibilityConnection, item: &atspi::ObjectRefOwned) {
    let mut current = item.clone();

    for depth in 0..12 {
        println!("  depth {depth}: {current:?}");

        let Ok(accessible) = connection.object_as_accessible(&current).await else {
            break;
        };

        let Ok(parent) = accessible.parent().await else {
            break;
        };

        if parent == current {
            break;
        }

        current = parent;
    }
}
