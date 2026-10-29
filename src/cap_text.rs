use crate::egui_app::SharedTargetRect;
use std::sync::{Arc, Mutex};

#[cfg(target_os = "linux")]
use crate::cap_text_linux;

#[cfg(windows)]
use crate::capture_text;

#[cfg(windows)]
pub fn start_capturing_text(query_arc: Arc<Mutex<String>>, target_rect_arc: SharedTargetRect) {
    capture_text::start_capturing_text(query_arc, target_rect_arc);
}

#[cfg(windows)]
pub fn get_current_language_code_string() -> String {
    capture_text::get_current_language_code_string()
}

#[cfg(target_os = "linux")]
pub fn start_capturing_text(query_arc: Arc<Mutex<String>>, target_rect_arc: SharedTargetRect) {
    cap_text_linux::start_capturing_text(query_arc, target_rect_arc);
}

#[cfg(target_os = "linux")]
pub fn get_current_language_code_string() -> String {
    cap_text_linux::get_current_language_code_string()
}
