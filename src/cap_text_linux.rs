use std::{sync::Arc, sync::Mutex, thread};

use atspi::{
    connection::P2P,
    events::{FocusEvents, ObjectEvents},
    proxy::proxy_ext::ProxyExt,
    AccessibilityConnection, ObjectRefOwned, State,
};
use futures_util::stream::StreamExt;

use crate::egui_app::{SharedTargetRect, TargetRect};

pub fn start_capturing_text(query_arc: Arc<Mutex<String>>, _target_rect_arc: SharedTargetRect) {
    thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("Failed to create Tokio runtime: {error}");
                return;
            }
        };

        if let Err(error) = runtime.block_on(listen_for_text_changes(query_arc)) {
            eprintln!("AT-SPI listener stopped: {error}");
        }
    });
}
async fn listen_for_text_changes(query_arc: Arc<Mutex<String>>) -> anyhow::Result<()> {
    let connection = AccessibilityConnection::new().await?;

    connection.register_event::<ObjectEvents>().await?;
    connection.register_event::<FocusEvents>().await?;

    let mut events = Box::pin(connection.event_stream());
    let mut focused_object: Option<ObjectRefOwned> = None;

    println!("Listening for focus and text changes...");

    while let Some(event) = events.next().await {
        let Ok(event) = event else {
            continue;
        };

        match event {
            atspi::Event::Object(ObjectEvents::StateChanged(event))
                if event.state == State::Focused =>
            {
                if event.enabled {
                    focused_object = Some(event.item.clone());
                } else if focused_object.as_ref() == Some(&event.item) {
                    focused_object = None;
                }
            }

            atspi::Event::Object(ObjectEvents::TextChanged(event)) => {
                let Some(focused) = focused_object.as_ref() else {
                    continue;
                };

                // Ignore text changes from other application connections.
                if event.item.name() != focused.name() {
                    continue;
                }

                // Read the full current text from the object that emitted the event.
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

                let Ok(contents) = text.get_text(0, count).await else {
                    continue;
                };

                update_query(&query_arc, contents);
            }

            _ => {}
        }
    }

    Ok(())
}

pub fn get_current_language_code_string() -> String {
    use std::process::Command;

    let output = Command::new("fcitx5-remote").arg("-n").output();

    let Ok(output) = output else {
        return "en".to_string();
    };

    if !output.status.success() {
        return "en".to_string();
    }

    let input_method = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_lowercase();

    match input_method.as_str() {
        "mozc" | "anthy" | "skk" => "ja".to_string(),
        name if name.starts_with("keyboard-") => "en".to_string(),
        _ => "en".to_string(),
    }
}
fn update_query(query_arc: &Arc<Mutex<String>>, contents: String) {
    let contents: String = contents
        .chars()
        .filter(|&c| c != '\u{FFFC}' && c != '\u{FEFF}')
        .collect();

    let contents = contents.trim_end_matches('\n').to_string();

    if contents.starts_with("https://") || contents.starts_with("http://") {
        return;
    }

    if let Ok(mut query) = query_arc.lock() {
        *query = contents;
    }
}
