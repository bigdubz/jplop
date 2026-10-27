use std::env::current_exe;
use eframe::egui;
use std::sync::{Arc, Mutex};
use std::time::{Instant, Duration};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::LazyLock;

use crate::db::{lookup_word, lookup_word_for_token};
// need to use a common interface, for when hyprland and kde impls are in place
use crate::capture_text::{get_current_language_code_string, start_capturing_text};
use crate::local_translator::{Translator};
use crate::sudachi_analyzer::{JapaneseAnalyzer, Token};
use crate::paths::db;

pub(crate) static SHARED_EGUI_CONTEXT: LazyLock<Mutex<Option<egui::Context>>> =
    LazyLock::new(|| Mutex::new(None));

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // Load a font that supports Japanese characters from the Windows system font directory
    if let Ok(font_data) = std::fs::read("C:\\Windows\\Fonts\\msgothic.ttc") {
        fonts.font_data.insert(
            "ms_gothic".to_owned(),
            egui::FontData::from_owned(font_data),
        );

        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .insert(0, "ms_gothic".to_owned());

        fonts
            .families
            .get_mut(&egui::FontFamily::Monospace)
            .unwrap()
            .push("ms_gothic".to_owned());

        ctx.set_fonts(fonts);
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub struct TargetRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub type SharedTargetRect = Arc<Mutex<Option<TargetRect>>>;

struct DictionaryOverlay {
    query_arc: Arc<Mutex<String>>,
    target_rect: SharedTargetRect,
    result: String,
    db_path: String,

    current_lang: String,
    last_lang_check_time: Instant,

    last_query: String,
    last_change_time: Option<Instant>,
    translation_triggered: bool,
    translation_analysis: TranslationAnalysis,
    query_tx: Sender<(String, String)>,
    result_rx: Receiver<(String, String)>,

    analyzer: JapaneseAnalyzer,
    show_word_popup: bool,
    popup_word: String,
    popup_result: String,
    popup_position: egui::Pos2,
}

struct TranslationAnalysis {
    result: String,
    dissection: Vec<([usize; 2], Token)>,
}

impl TranslationAnalysis {
    fn new() -> Self {
        Self {
            result: String::new(),
            dissection: Vec::new(),
        }
    }
}

impl DictionaryOverlay {
    fn new(db_path: String, query_arc: Arc<Mutex<String>>,
           target_rect_arc: SharedTargetRect, model: Option<Translator>,
            analyzer: JapaneseAnalyzer) -> Self {

        let (query_tx, query_rx): (Sender<(String, String)>, Receiver<(String, String)>) = channel();
        let (result_tx, result_rx) = channel();
        if model.is_some() {
            let some_model = model.unwrap();
            std::thread::spawn(move || {
                while let Ok((query, lang)) = query_rx.recv() {

                    // only process latest query
                    let mut latest_query = query;
                    let mut latest_lang = lang;
                    while let Ok((newer_query, newer_lang)) = query_rx.try_recv() {
                        latest_query = newer_query;
                        latest_lang = newer_lang;
                    }

                    let r = match latest_lang.as_str() {
                        "en" => some_model.to_japanese(&latest_query),
                        "ja" => some_model.to_english(&latest_query),
                        _ => some_model.to_japanese(&latest_query),
                    };
                    if let Ok(res) = r {
                        let _ = result_tx.send((latest_query, res));
                    }
                }
            });
        } else {
            println!("[info]\tmodel not loaded, running overlay without model")
        }

        Self {
            query_arc,
            target_rect: target_rect_arc,
            result: String::new(),
            db_path,
            current_lang: get_current_language_code_string(),
            last_lang_check_time: Instant::now(),
            last_query: String::new(),
            last_change_time: None,
            translation_triggered: false,
            translation_analysis: TranslationAnalysis::new(),
            query_tx,
            result_rx,
            analyzer,
            show_word_popup: false,
            popup_word: String::new(),
            popup_result: String::new(),
            popup_position: egui::Pos2::ZERO,
        }
    }
}

const MIN_OVERLAY_WIDTH: f32 = 200.0;
const MAX_OVERLAY_WIDTH: f32 = 450.0;
const MAX_OVERLAY_HEIGHT: f32 = 600.0;
const OVERLAY_PADDING: f32 = 16.0;

impl eframe::App for DictionaryOverlay {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let current_query = {
            let q = self.query_arc.lock().unwrap();
            q.clone()
        };

        if self.last_lang_check_time.elapsed() >= Duration::from_millis(2000) {
            self.last_lang_check_time = Instant::now();
            self.current_lang = get_current_language_code_string();
        }

        if current_query.trim().is_empty() || current_query.starts_with("https://") ||
            current_query.starts_with("http://") {
            ctx.send_viewport_cmd(
                egui::ViewportCommand::OuterPosition(egui::pos2(-10000.0, -10000.0))
            );

            self.last_query.clear();
            self.last_change_time = None;
            self.translation_triggered = false;
            self.show_word_popup = false;
            self.translation_analysis.result.clear();

            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }

        while let Ok((query_for_res, res)) = self.result_rx.try_recv() {
            if query_for_res == current_query {
                self.translation_analysis.result = res;

                if self.current_lang == "en" {
                    self.translation_analysis.dissection = self.analyzer.dissect(
                        self.translation_analysis.result.as_str()
                    ).unwrap();
                }
            }
        }

        if self.current_lang == "ja" {
            self.translation_analysis.dissection = self.analyzer.dissect(
                current_query.as_str()
            ).unwrap();
        }

        if current_query != self.last_query {
            self.last_query = current_query.clone();
            self.last_change_time = Some(Instant::now());
            self.translation_triggered = false;
        }

        if !self.translation_triggered {
            if let Some(change_time) = self.last_change_time {
                if change_time.elapsed() >= Duration::from_millis(300) {
                    self.translation_triggered = true;
                    let _ = self.query_tx.send((current_query.clone(), self.current_lang.clone()));
                }
            }
        }

        match lookup_word(&self.db_path, &current_query, None) {
            Ok(res) => self.result = res,
            Err(e) => self.result = format!("Error: {}", e),
        }

        let mut content_size = egui::Vec2::ZERO;

        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .fill(egui::Color32::from_black_alpha(200))
                    .inner_margin(OVERLAY_PADDING),
            )
            .show(ctx, |ui| {
                let available_width = MAX_OVERLAY_WIDTH - OVERLAY_PADDING * 2.0;
                ui.set_max_width(available_width);

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(MAX_OVERLAY_HEIGHT - OVERLAY_PADDING * 2.0)
                    .show(ui, |ui| {
                        let mut style = (*ui.ctx().style()).clone();
                        style.interaction.tooltip_delay = 0.05;
                        ui.ctx().set_style(style);
                        ui.set_width(available_width);
                        ui.heading("Dictionary Overlay");
                        ui.label(&current_query);
                        ui.separator();

                        if !self.result.is_empty() {
                            ui.label("Dictionary:");
                            ui.label(&self.result);
                            ui.separator();
                        }

                        if self.current_lang == "en" {
                            ui.label("Translation:");
                            let translation = &self.translation_analysis.result;
                            let dissected = &self.translation_analysis.dissection;
                            render_highlighted_text(
                                ui,
                                translation,
                                dissected,
                                &mut self.show_word_popup,
                                &mut self.popup_word,
                                &mut self.popup_result,
                                &mut self.popup_position,
                            );
                        } else {
                            ui.label("Analysis:");
                            let dissected = &self.translation_analysis.dissection;
                            render_highlighted_text(
                                ui,
                                &current_query,
                                dissected,
                                &mut self.show_word_popup,
                                &mut self.popup_word,
                                &mut self.popup_result,
                                &mut self.popup_position,
                            );
                            ui.label("Translation:");
                            ui.label(self.translation_analysis.result.clone());
                        }

                        content_size = ui.min_size();
                    });
            });

        let desired_width = (content_size.x + OVERLAY_PADDING * 2.0)
            .clamp(MIN_OVERLAY_WIDTH, MAX_OVERLAY_WIDTH);

        let desired_height = (content_size.y + OVERLAY_PADDING * 2.0)
            .clamp(80.0, MAX_OVERLAY_HEIGHT);

        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
            egui::vec2(desired_width, desired_height),
        ));

        let target = {
            *self.target_rect.lock().unwrap()
        };

        if let Some(rect) = target {
            let top = rect.y - desired_height - 12.0;
            let x = rect.x;
            let y = {
                if top > 0.0 {
                    top
                } else {
                    rect.y + rect.height + 6.0
                }
            };

            ctx.send_viewport_cmd(
                egui::ViewportCommand::OuterPosition(
                    egui::pos2(x, y)
                )
            );
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            println!("asdf");
            self.show_word_popup = false;
        }

        if self.show_word_popup {
            let popup_id = egui::ViewportId::from_hash_of("dictionary_popup");

            ctx.show_viewport_immediate(
                popup_id,
                egui::ViewportBuilder::default()
                    .with_title(format!("Dictionary: {}", self.popup_word))
                    .with_inner_size([MIN_OVERLAY_WIDTH, 100.0])
                    .with_min_inner_size([MIN_OVERLAY_WIDTH, 80.0])
                    .with_max_inner_size([MAX_OVERLAY_WIDTH, MAX_OVERLAY_HEIGHT])
                    .with_resizable(true)
                    .with_always_on_top(),
                |popup_ctx, _class| {
                    if popup_ctx.input(|i| i.viewport().close_requested()) {
                        popup_ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                        self.show_word_popup = false;
                        return;
                    }

                    let mut content_size = egui::Vec2::ZERO;

                    egui::CentralPanel::default()
                        .frame(
                            egui::Frame::default()
                                .fill(egui::Color32::from_black_alpha(200))
                                .inner_margin(OVERLAY_PADDING),
                        )
                        .show(popup_ctx, |ui| {
                            let available_width =
                                MAX_OVERLAY_WIDTH - OVERLAY_PADDING * 2.0;

                            ui.set_max_width(available_width);

                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .max_height(
                                    MAX_OVERLAY_HEIGHT - OVERLAY_PADDING * 2.0,
                                )
                                .show(ui, |ui| {
                                    ui.set_width(available_width);

                                    ui.heading(&self.popup_word);
                                    ui.separator();
                                    ui.label(&self.popup_result);

                                    content_size = ui.min_size();
                                });
                        });

                    let desired_width = (content_size.x + OVERLAY_PADDING * 2.0)
                        .clamp(MIN_OVERLAY_WIDTH, MAX_OVERLAY_WIDTH);

                    let desired_height = (content_size.y + OVERLAY_PADDING * 2.0)
                        .clamp(80.0, MAX_OVERLAY_HEIGHT);

                    popup_ctx.send_viewport_cmd(
                        egui::ViewportCommand::InnerSize(
                            egui::vec2(desired_width, desired_height),
                        ),
                    );
                },
            );
        }

        ctx.request_repaint();
    }
}

pub fn start_ui(model: Option<Translator>,
                analyzer: JapaneseAnalyzer) -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([300.0, 200.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_visible(false),
        ..Default::default()
    };

    let path = current_exe().unwrap()
        .parent()
        .unwrap()
        .to_path_buf();

    let db_path = path.join("db.db");
    let query_arc = Arc::new(Mutex::new(String::new()));
    let target_rect_arc: SharedTargetRect = Arc::new(Mutex::new(None));
    start_capturing_text(query_arc.clone(), target_rect_arc.clone());

    eframe::run_native(
        "DictOverlay",
        options,
        Box::new(move |cc| {
            setup_fonts(&cc.egui_ctx);

            if let Ok(mut ctx) = SHARED_EGUI_CONTEXT.lock() {
                *ctx = Some(cc.egui_ctx.clone());
            }

            Ok(Box::new(DictionaryOverlay::new(
                db_path.to_str().unwrap().to_string(),
                query_arc.clone(),
                target_rect_arc.clone(),
                model,
                analyzer
            )))
        }),
    )
}

fn render_highlighted_text(
    ui: &mut egui::Ui,
    text: &str,
    dissected: &[([usize; 2], Token)],
    show_word_popup: &mut bool,
    popup_word: &mut String,
    popup_result: &mut String,
    popup_position: &mut egui::Pos2,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        ui.spacing_mut().item_spacing.y = 0.0;

        for (range, token) in dissected {
            let Some(surface) = text.get(range[0]..range[1] + 1) else {
                continue;
            };


            let response = ui.add(
                egui::Label::new(surface)
                    .sense(egui::Sense::click()),
            );

            if response.hovered() {
                ui.painter().rect_filled(
                    response.rect,
                    2.0,
                    egui::Color32::from_rgba_premultiplied(70, 90, 130, 128),
                );
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }

            let response = response.on_hover_text(
                format!("{} [{}]", token.dictionary_form, token.reading)
            );

            if response.clicked() {
                *popup_word = token.dictionary_form.clone();
                let lookup_result = lookup_word_for_token(
                    db().to_str().unwrap(),
                    token
                ).unwrap_or_else(|e| format!("Lookup failed: {e}"));
                if !lookup_result.is_empty() {
                    *popup_result = lookup_result.clone();

                    *popup_position = response.rect.right_bottom();
                    *show_word_popup = true;
                } else {
                    *popup_result = format!("No JMDict entry found for {}", token.surface);
                }
            }
        }
    });
}
