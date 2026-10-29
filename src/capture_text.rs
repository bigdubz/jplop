use crate::egui_app::{SharedTargetRect, TargetRect};
use std::sync::{Arc, Mutex};
use uiautomation::events::{CustomFocusChangedEventHandler, CustomPropertyChangedEventHandler};
use uiautomation::types::{TreeScope, UIProperty};
use uiautomation::{Result, UIAutomation, UIElement};
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

static mut SHARED_QUERY: Option<Arc<Mutex<String>>> = None;
static mut SHARED_TARGET_RECT: Option<Arc<Mutex<Option<TargetRect>>>> = None;

fn get_current_keyboard_language_id() -> u16 {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return 0x0409;
        }

        let thread_id = GetWindowThreadProcessId(hwnd, None);
        if thread_id == 0 {
            return 0x0409;
        }

        let hkl = GetKeyboardLayout(thread_id);

        let lang_id = (hkl.0 as usize & 0xFFFF) as u16;
        lang_id
    }
}

pub fn get_current_language_code_string() -> String {
    let lang_id = get_current_keyboard_language_id();

    match lang_id {
        0x0409 => "en".to_string(),
        0x0411 => "ja".to_string(),
        _ => format!("{:04X}", lang_id),
    }
}

struct TextChangeHandler;

impl CustomPropertyChangedEventHandler for TextChangeHandler {
    fn handle(
        &self,
        sender: &UIElement,
        property: UIProperty,
        value: uiautomation::variants::Variant,
    ) -> Result<()> {
        if property == UIProperty::ValueValue {
            if let Ok(text) = value.get_string() {
                if text.starts_with("https://") || text.starts_with("http://") {
                    return Ok(());
                }
                unsafe {
                    if let Some(ref query_arc) = SHARED_QUERY {
                        if let Ok(mut query) = query_arc.lock() {
                            *query = text;
                        }
                    }
                    if let Some(ref target_rect_arc) = SHARED_TARGET_RECT {
                        if let Ok(mut target_rect) = target_rect_arc.lock() {
                            if let Ok(rect) = sender.get_bounding_rectangle() {
                                *target_rect = Some(TargetRect {
                                    x: rect.get_left() as f32,
                                    y: rect.get_top() as f32,
                                    width: (rect.get_right() - rect.get_left()) as f32,
                                    height: (rect.get_bottom() - rect.get_top()) as f32,
                                })
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

struct FocusHandler {
    automation: UIAutomation,
}

impl CustomFocusChangedEventHandler for FocusHandler {
    fn handle(&self, sender: &UIElement) -> Result<()> {
        if let Ok(name) = sender.get_name() {
            println!("[info]\tswitched to {name}");
            if name.starts_with("Dict") || name == "Address and search bar" {
                return Ok(());
            }
        }

        if let Ok(val_var) = sender.get_property_value(UIProperty::ValueValue) {
            if let Ok(text) = val_var.get_string() {
                unsafe {
                    if let Some(ref query_arc) = SHARED_QUERY {
                        if let Ok(mut query) = query_arc.lock() {
                            *query = text;
                        }
                    }
                }
            }
        }

        let text_handler = TextChangeHandler;
        let event_handler = uiautomation::events::UIPropertyChangedEventHandler::from(text_handler);

        let _ = self.automation.add_property_changed_event_handler(
            sender,
            TreeScope::Element,
            None,
            &event_handler,
            &[UIProperty::ValueValue],
        );
        Ok(())
    }
}

pub fn start_capturing_text(query_arc: Arc<Mutex<String>>, target_rect_arc: SharedTargetRect) {
    unsafe {
        SHARED_QUERY = Some(query_arc);
        SHARED_TARGET_RECT = Some(target_rect_arc);
    }

    std::thread::spawn(move || {
        let automation = match UIAutomation::new() {
            Ok(a) => a,
            Err(_) => return,
        };

        let handler = FocusHandler {
            automation: automation.clone(),
        };
        let event_handler = uiautomation::events::UIFocusChangedEventHandler::from(handler);

        if automation
            .add_focus_changed_event_handler(None, &event_handler)
            .is_err()
        {
            return;
        }

        let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
        unsafe {
            while windows::Win32::UI::WindowsAndMessaging::GetMessageW(&mut msg, None, 0, 0).into()
            {
                let _ = windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
                let _ = windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
            }
        }
    });
}
