#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use crate::db::{verify_import, import_jmdict_if_needed};
use crate::local_translator::{load_lfm};
use crate::egui_app::start_ui;
use crate::paths::{db, jm, dict};

mod db;
mod capture_text;
mod egui_app;
mod local_translator;
mod sudachi_analyzer;
mod paths;

use anyhow::Result;

fn main() -> Result<()> {
    paths::init()?;
    let _dict = import_jmdict_if_needed(
        db().to_str().unwrap(),
        jm().to_str().unwrap(),
    );
    let _verify_dict = verify_import(db().to_str().unwrap());
    let analyzer = sudachi_analyzer::JapaneseAnalyzer::new(
        dict(),
    ).unwrap();
    
    // let _start = start_ui(None);
    let model = load_lfm()?;
    // test_translation_quality(model);
    let _start = start_ui(Some(model), analyzer);

    Ok(())
}
