use std::{sync::Arc, sync::Mutex, thread};

use atspi::{
    connection::P2P,
    events::{FocusEvents, ObjectEvents},
    proxy::proxy_ext::ProxyExt,
    AccessibilityConnection, CoordType, ObjectRefOwned, State,
};
use futures_util::stream::StreamExt;

use crate::egui_app::{SharedTargetRect, TargetRect};
use crate::hyprland_util::hyprland_get_window_coordinates_by_pid;

pub fn start_capturing_text(query_arc: Arc<Mutex<String>>, target_rect_arc: SharedTargetRect) {
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

        if let Err(error) = runtime.block_on(listen_for_text_changes(query_arc, target_rect_arc)) {
            eprintln!("AT-SPI listener stopped: {error}");
        }
    });
}

async fn listen_for_text_changes(
    query_arc: Arc<Mutex<String>>,
    target_rect_arc: SharedTargetRect,
) -> anyhow::Result<()> {
    let connection = AccessibilityConnection::new().await?;

    connection.register_event::<ObjectEvents>().await?;
    connection.register_event::<FocusEvents>().await?;

    let mut events = Box::pin(connection.event_stream());
    let mut focused_object: Option<ObjectRefOwned> = None;

    println!("Listening for focus and text changes...");

    let mut last_rect_object: Option<ObjectRefOwned> = None;
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
                if last_rect_object.as_ref() != Some(&event.item) {
                    update_target_rect(&connection, &event.item, &target_rect_arc).await;
                    last_rect_object = Some(event.item.clone());
                }
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

async fn update_target_rect(
    connection: &AccessibilityConnection,
    object: &ObjectRefOwned,
    target_rect_arc: &SharedTargetRect,
) {
    let Ok(accessible) = connection.object_as_accessible(object).await else {
        return;
    };

    let Ok(proxies) = accessible.proxies().await else {
        return;
    };

    let Ok(component) = proxies.component().await else {
        return;
    };

    let Ok((x, y, width, height)) = component.get_extents(CoordType::Screen).await else {
        return;
    };

    if width <= 0 || height <= 0 {
        return;
    }

    let window_coords: Option<(i32, i32)>;
    match get_accessible_pid(&connection, &object).await {
        Ok(pid) => window_coords = hyprland_get_window_coordinates_by_pid(pid as i64),
        Err(error) => {
            window_coords = None;
            eprintln!("Failed to get accessible PID: {error}");
        }
    }

    let (x, y) = window_coords
        .map(|(wx, wy)| (x + wx, y + wy))
        .unwrap_or((x, y));

    if let Ok(mut target) = target_rect_arc.lock() {
        *target = Some(TargetRect {
            x: x as f32,
            y: y as f32,
            width: width as f32,
            height: height as f32,
        });
    }
}

async fn get_accessible_pid(
    connection: &AccessibilityConnection,
    object: &ObjectRefOwned,
) -> anyhow::Result<u32> {
    use zbus::fdo::DBusProxy;
    use zbus::names::BusName;

    let bus_name = object
        .name()
        .ok_or_else(|| anyhow::anyhow!("Accessible object has no bus name"))?
        .to_string();

    let dbus = DBusProxy::new(connection.connection()).await?;
    let bus_name = BusName::try_from(bus_name.as_str())?;
    let pid = dbus.get_connection_unix_process_id(bus_name).await?;

    Ok(pid)
}
