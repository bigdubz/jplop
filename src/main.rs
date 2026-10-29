#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use crate::db::{import_jmdict_if_needed, verify_import};
use crate::egui_app::start_ui;
use crate::local_translator::load_lfm;
use crate::paths::{db, dict, jm};

mod cap_text;
#[cfg(target_os = "linux")]
mod cap_text_linux;
#[cfg(windows)]
mod capture_text;
mod db;
mod egui_app;
mod local_translator;
mod paths;
mod sudachi_analyzer;

use anyhow::Result;

fn main() -> Result<()> {
    paths::init()?;
    let _dict = import_jmdict_if_needed(db().to_str().unwrap(), jm().to_str().unwrap());
    let _verify_dict = verify_import(db().to_str().unwrap());

    let analyzer = sudachi_analyzer::JapaneseAnalyzer::new(dict()).unwrap();
    let model = load_lfm()?;
    let _start = start_ui(Some(model), analyzer);

    Ok(())
}
