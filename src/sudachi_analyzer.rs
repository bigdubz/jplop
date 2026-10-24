use std::path::Path;
use std::error::Error;
use sudachi::analysis::stateless_tokenizer::StatelessTokenizer;
use sudachi::analysis::{Mode, Tokenize};
use sudachi::config::Config;
use sudachi::dic::dictionary::JapaneseDictionary;

pub struct JapaneseAnalyzer {
    dict: JapaneseDictionary,
}

impl JapaneseAnalyzer {
    pub fn new(dictionary_path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        let config = Config::new(
            None,
            None,
            Some(dictionary_path.as_ref().to_path_buf()),
        )?;

        let dict = JapaneseDictionary::from_cfg(&config)?;

        Ok(Self { dict })
    }

    pub fn analyze(&self, text: &str) -> Result<Vec<Token>, Box<dyn Error>> {
        let tokenizer = StatelessTokenizer::new(&self.dict);
        let result = tokenizer.tokenize(text, Mode::B, false)?;

        let tokens = result
            .iter()
            .map(|m| Token {
                surface: m.surface().to_string(),
                reading: katakana_to_hiragana(m.reading_form()),
                dictionary_form: m.dictionary_form().to_string(),
                part_of_speech: m.part_of_speech().to_vec(),
            })
            .collect();

        Ok(tokens)
    }

    pub fn dissect(&self, text: &str) -> Result<Vec<([usize; 2], Token)>, Box<dyn Error>> {
        let tokens = self.analyze(text)?;

        let mut result: Vec<([usize; 2], Token)> = Vec::new();
        let mut i = 0usize;
        for token in tokens {
            let surface = token.clone().surface;
            let rel_pos = text[i..].find(&surface).ok_or("token not found")?;
            let start = i + rel_pos;
            let end = start + surface.len() - 1;

            result.push(([start, end], token));
            i = start + surface.len();
        }
        Ok(result)
    }
}

pub fn katakana_to_hiragana(text: &str) -> String {
    text.chars()
        .map(|c| {
            if ('ァ'..='ヶ').contains(&c) {
                char::from_u32(c as u32 - 0x60).unwrap()
            } else {
                c
            }
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct Token {
    pub surface: String,
    pub reading: String,
    pub dictionary_form: String,
    pub part_of_speech: Vec<String>,
}