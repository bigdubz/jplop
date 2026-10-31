use std::process::Command;

pub fn hyprland_window_exists(name: &str) -> bool {
    let output = match Command::new("hyprctl").args(["-j", "clients"]).output() {
        Ok(output) if output.status.success() => output,
        _ => return false,
    };

    let Ok(clients) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return false;
    };

    let Some(_) = clients.as_array().and_then(|clients| {
        clients
            .iter()
            .find(|client| client["title"].as_str() == Some(name))
    }) else {
        return false;
    };

    return true;
}

pub fn resize_hyprland_window(width: u32, height: u32, name: &str) {
    use std::process::Command;

    let command = format!(
        r#"hl.dsp.window.resize({{ x = {width}, y = {height}, relative = false, window = "title:{name}" }})"#
    );

    let _ = Command::new("hyprctl")
        .args(["dispatch", &command])
        .output();

    println!("o!");
}

pub fn try_set_hyprland_window_floating() -> bool {
    let output = match Command::new("hyprctl").args(["-j", "clients"]).output() {
        Ok(output) if output.status.success() => output,
        _ => return false,
    };

    let Ok(clients) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return false;
    };

    let Some(window) = clients.as_array().and_then(|clients| {
        clients
            .iter()
            .find(|client| client["title"].as_str() == Some("DictOverlay"))
    }) else {
        return false;
    };

    if window["floating"].as_bool() == Some(true) {
        return true;
    }

    let result = Command::new("hyprctl")
        .args([
            "dispatch",
            r#"hl.dsp.window.float({ action = "set", window = "title:DictOverlay" })"#,
        ])
        .output();

    matches!(result, Ok(output) if output.status.success())
}
