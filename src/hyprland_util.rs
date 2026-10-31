use std::process::Command;

pub fn hyprland_window_exists(title: &str) -> bool {
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
            .find(|client| client["title"].as_str() == Some(title))
    }) else {
        return false;
    };

    return true;
}

pub fn resize_hyprland_window(width: u32, height: u32, title: &str) {
    use std::process::Command;

    let command = format!(
        r#"hl.dsp.window.resize({{ x = {width}, y = {height}, relative = false, window = "title:{title}" }})"#
    );

    let _ = Command::new("hyprctl")
        .args(["dispatch", &command])
        .output();
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

pub fn move_hyprland_window(x: i32, y: i32, title: &str) {
    use std::process::Command;

    let command =
        format!(r#"hl.dsp.window.move({{ x = {x}, y = {y}, window = "title:{title}" }})"#);

    let _ = Command::new("hyprctl")
        .args(["dispatch", &command])
        .output();
}

pub fn hyprland_get_window_coordinates_by_pid(pid: i64) -> Option<(i32, i32)> {
    let output = match Command::new("hyprctl").args(["-j", "clients"]).output() {
        Ok(output) if output.status.success() => output,
        _ => return None,
    };

    let clients: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;

    let window = clients
        .as_array()?
        .iter()
        .find(|client| client["pid"].as_i64() == Some(pid))?;

    let at = window["at"].as_array()?;

    let x = i32::try_from(at.first()?.as_i64()?).ok()?;
    let y = i32::try_from(at.get(1)?.as_i64()?).ok()?;

    Some((x, y))
}
