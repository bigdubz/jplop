use quick_xml::events::Event;
use quick_xml::Reader;
use rusqlite::{params, Connection, Result};
use std::fs::File;
use std::io::BufReader;

use std::path::{Path};
use crate::sudachi_analyzer::Token;


pub fn import_jmdict_if_needed(db_path: &str, xml_path: &str) -> Result<()> {
    if Path::new(db_path).exists() {
        let conn = Connection::open(db_path)?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='entries'",
            [],
            |row| row.get(0),
        )?;

        if count > 0 {
            let row_count: i64 = conn.query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))?;
            if row_count > 0 {
                println!("[info]\tdatabase already populated with {} entries. skipping import.", row_count);
                return Ok(());
            }
        }
    }

    println!("[info]\tdatabase not found or empty. importing...");
    import_jmdict(db_path, xml_path)?;
    Ok(())
}

#[derive(Default, Clone)]
struct JmForm {
    text: String,
    priority_tags: Vec<String>,
}

// Lower priority_class means a more preferred spelling.
// The frequency rank breaks ties when an nfXX tag is available.
fn form_priority(tags: &[String]) -> (i64, i64) {
    let frequency_rank = tags
        .iter()
        .filter_map(|tag| {
            tag.strip_prefix("nf")
                .and_then(|n| n.parse::<i64>().ok())
        })
        .min()
        .unwrap_or(9999);

    let has_top_priority = tags.iter().any(|tag| {
        matches!(
            tag.as_str(),
            "news1" | "ichi1" | "spec1" | "gai1"
        )
    });

    let has_second_tier = tags.iter().any(|tag| {
        matches!(
            tag.as_str(),
            "news2" | "ichi2" | "spec2" | "gai2"
        )
    });

    let class = if has_top_priority {
        0
    } else if frequency_rank != 9999 {
        1
    } else if has_second_tier {
        2
    } else {
        3
    };

    (class, frequency_rank)
}

pub fn import_jmdict(db_path: &str, xml_path: &str) -> Result<()> {
    let mut conn = Connection::open(db_path)?;

    conn.execute_batch(
        "DROP TABLE IF EXISTS entries;

     CREATE TABLE entries (
         id INTEGER PRIMARY KEY,
         entry_seq INTEGER NOT NULL,
         kanji TEXT NOT NULL,
         reading TEXT NOT NULL,
         gloss TEXT NOT NULL,
         examples TEXT NOT NULL,
         pos TEXT NOT NULL DEFAULT '',
         kanji_priority_class INTEGER NOT NULL,
         kanji_frequency_rank INTEGER NOT NULL,
         reading_priority_class INTEGER NOT NULL,
         reading_frequency_rank INTEGER NOT NULL
     );

     CREATE INDEX idx_kanji ON entries (kanji);
     CREATE INDEX idx_reading ON entries (reading);
     CREATE INDEX idx_entry_seq ON entries (entry_seq);",
    )?;

    let tx = conn.transaction()?;

    {
        let file = File::open(xml_path).expect("Failed to open JMdict XML file");
        let mut reader = Reader::from_reader(BufReader::new(file));
        reader.config_mut().trim_text(true);

        let mut buf = Vec::new();

        let mut current_entry_seq: i64 = 0;

        let mut current_kanjis: Vec<JmForm> = Vec::new();
        let mut current_readings: Vec<JmForm> = Vec::new();

        let mut current_kanji: Option<JmForm> = None;
        let mut current_reading: Option<JmForm> = None;

        let mut current_glosses: Vec<String> = Vec::new();
        let mut current_examples: Vec<String> = Vec::new();

        let mut current_pos: Vec<String> = Vec::new();

        let mut in_ent_seq = false;
        let mut in_kanji = false;
        let mut in_reading = false;
        let mut in_ke_pri = false;
        let mut in_re_pri = false;
        let mut in_gloss = false;
        let mut in_ex_sent = false;
        let mut in_pos = false;

        let mut current_ex_lang = String::new();
        let mut current_ex_text = String::new();
        let mut current_gloss_text = String::new();
        let mut current_seq_text = String::new();
        let mut current_pos_text = String::new();

        let mut insert_stmt = tx.prepare(
            "INSERT INTO entries (
                    entry_seq,
                    kanji,
                    reading,
                    gloss,
                    examples,
                    pos,
                    kanji_priority_class,
                    kanji_frequency_rank,
                    reading_priority_class,
                    reading_frequency_rank
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
        )?;
        let mut imported_entries: u64 = 0;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => match e.name().as_ref() {
                    "ent_seq" => {
                        in_ent_seq = true;
                        current_seq_text.clear();
                    }

                    "k_ele" => {
                        current_kanji = Some(JmForm::default());
                    }
                    "keb" => in_kanji = true,
                    "ke_pri" => in_ke_pri = true,

                    "r_ele" => {
                        current_reading = Some(JmForm::default());
                    }
                    "reb" => in_reading = true,
                    "re_pri" => in_re_pri = true,

                    "gloss" => {
                        in_gloss = true;
                        current_gloss_text.clear();
                    }

                    "ex_sent" => {
                        in_ex_sent = true;
                        current_ex_text.clear();
                        current_ex_lang.clear();

                        for attr in e.attributes().flatten() {
                            let key = attr.key.as_ref();

                            if key == "xml:lang" || key == "lang" {
                                current_ex_lang =
                                    String::from_utf8_lossy(attr.value.as_ref().as_ref()).into_owned();
                            }
                        }
                    }

                    "pos" => {
                        in_pos = true;
                        current_pos_text.clear();
                    }
                    _ => {}
                },
                Ok(Event::Text(e)) => {
                    let text = e.xml10_content().into_owned();

                    if in_ent_seq {
                        current_seq_text.push_str(&text);
                    }

                    if in_kanji {
                        if let Some(form) = current_kanji.as_mut() {
                            form.text.push_str(&text);
                        }
                    }

                    if in_ke_pri {
                        if let Some(form) = current_kanji.as_mut() {
                            form.priority_tags.push(text.clone());
                        }
                    }

                    if in_reading {
                        if let Some(form) = current_reading.as_mut() {
                            form.text.push_str(&text);
                        }
                    }

                    if in_re_pri {
                        if let Some(form) = current_reading.as_mut() {
                            form.priority_tags.push(text.clone());
                        }
                    }

                    if in_gloss {
                        current_gloss_text.push_str(&text);
                    }

                    if in_ex_sent {
                        current_ex_text.push_str(&text);
                    }

                    if in_pos {
                        current_pos_text.push_str(&text);
                    }
                },
                Ok(Event::End(ref e)) => match e.name().as_ref() {
                    "ent_seq" => {
                        in_ent_seq = false;
                        current_entry_seq = current_seq_text.trim().parse().unwrap_or(0);
                    }

                    "keb" => in_kanji = false,
                    "ke_pri" => in_ke_pri = false,

                    "k_ele" => {
                        if let Some(form) = current_kanji.take() {
                            current_kanjis.push(form);
                        }
                    }

                    "reb" => in_reading = false,
                    "re_pri" => in_re_pri = false,

                    "r_ele" => {
                        if let Some(form) = current_reading.take() {
                            current_readings.push(form);
                        }
                    }

                    "gloss" => {
                        in_gloss = false;
                        current_glosses.push(current_gloss_text.trim().to_owned());
                        current_gloss_text.clear();
                    }

                    "ex_sent" => {
                        in_ex_sent = false;

                        if !current_ex_text.trim().is_empty() {
                            current_examples.push(format!(
                                "{}: {}",
                                current_ex_lang,
                                current_ex_text.trim()
                            ));
                        }

                        current_ex_text.clear();
                        current_ex_lang.clear();
                    }

                    "pos" => {
                        in_pos = false;
                        let pos = current_pos_text.trim();

                        if !pos.is_empty() && !current_pos.contains(&pos.to_string()) {
                            current_pos.push(pos.to_string());
                        }

                        current_pos_text.clear();
                    }

                    "entry" => {
                        if current_kanjis.is_empty() {
                            current_kanjis.push(JmForm::default());
                        }

                        if current_readings.is_empty() {
                            current_readings.push(JmForm::default());
                        }

                        let gloss_str = current_glosses.join("; ");
                        let example_str = current_examples.join("\n");

                        let mut unique_pos = Vec::new();

                        for pos in &current_pos {
                            if !unique_pos.contains(pos) {
                                unique_pos.push(pos.clone());
                            }
                        }

                        let pos_str = unique_pos.join(", ");

                        for kanji in &current_kanjis {
                            for reading in &current_readings {
                                let (kp_class, kp_freq) =
                                    form_priority(&kanji.priority_tags);
                                let (rp_class, rp_freq) =
                                    form_priority(&reading.priority_tags);

                                insert_stmt.execute(rusqlite::params![
                                    current_entry_seq,
                                    kanji.text,
                                    reading.text,
                                    gloss_str,
                                    example_str,
                                    pos_str,
                                    kp_class,
                                    kp_freq,
                                    rp_class,
                                    rp_freq
                                ])?;
                            }
                        }

                        current_entry_seq = 0;
                        current_kanjis.clear();
                        current_readings.clear();
                        current_glosses.clear();
                        current_examples.clear();
                        current_pos.clear();

                        imported_entries += 1;
                        if imported_entries % 10_000 == 0 {
                            eprintln!("[info]\timported {imported_entries} JMdict entries...");
                        }
                    }

                    _ => {}
                },
                Ok(Event::GeneralRef(e)) => {
                    if in_pos {
                        current_pos.push(e.as_ref().to_string());
                    }
                },
                Ok(Event::Eof) => break,
                Err(e) => panic!("Error parsing XML at position {}: {:?}", reader.buffer_position(), e),
                _ => (),
            }
            buf.clear();
        }
        drop(insert_stmt);
    }

    tx.commit()?;
    Ok(())
}

pub fn verify_import(db_path: &str) -> Result<()> {
    let conn = Connection::open(db_path)?;
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))?;
    println!("[info]\tDictionary imported with total entries imported: {}", count);
    Ok(())
}

pub fn lookup_word(db_path: &str, query: &str, preferred_pos: Option<&str>) -> Result<String> {
    let conn = Connection::open(db_path)?;

    let mut stmt = conn.prepare(
    "WITH matched_entries AS (
            SELECT
                entry_seq,
                MIN(
                    CASE
                        WHEN ?2 IS NOT NULL
                         AND (',' || REPLACE(pos, ' ', '') || ',')
                             LIKE '%,' || ?2 || ',%'
                        THEN 0
                        ELSE 1
                    END
                ) AS pos_priority
            FROM entries
            WHERE kanji = ?1 OR reading = ?1
            GROUP BY entry_seq
        )
        SELECT
            e.entry_seq,
            e.kanji,
            e.reading,
            e.gloss,
            e.examples,
            e.pos,
            e.kanji_priority_class,
            e.kanji_frequency_rank,
            e.reading_priority_class,
            e.reading_frequency_rank
        FROM entries AS e
        JOIN matched_entries AS m
            ON e.entry_seq = m.entry_seq
        ORDER BY
            m.pos_priority,
            e.entry_seq,
            e.kanji_priority_class,
            e.kanji_frequency_rank,
            e.reading_priority_class,
            e.reading_frequency_rank,
            e.kanji,
            e.reading;"
    )?;

    let mut rows = stmt.query(params![query, preferred_pos])?;
    let mut result = String::new();

    let mut prev_key: Option<(i64, String, String)> = None;
    let mut prev_kanji = String::new();
    let mut prev_reading = String::new();
    let mut alternatives_kanji: Vec<String> = Vec::new();
    let mut alternatives_reading: Vec<String> = Vec::new();

    while let Some(row) = rows.next()? {
        let entry_seq: i64 = row.get(0)?;
        let kanji: String = row.get(1)?;
        let reading: String = row.get(2)?;
        let gloss: String = row.get(3)?;
        let examples: String = row.get(4)?;
        let pos: String = row.get(5)?;
        let pos_conv = match_pos(&pos);

        let key = (entry_seq, gloss.clone(), examples.clone());

        if prev_key.as_ref() == Some(&key) {
            if !kanji.is_empty()
                && kanji != prev_kanji
                && !alternatives_kanji.contains(&kanji)
            {
                alternatives_kanji.push(kanji);
            }

            if !reading.is_empty()
                && reading != prev_reading
                && !alternatives_reading.contains(&reading)
            {
                alternatives_reading.push(reading);
            }

            continue;
        }

        if prev_key.is_some() {
            append_alternatives(
                &mut result,
                &alternatives_kanji,
                &alternatives_reading,
            );

            result.push_str("\n\n────────────────────\n\n");
        }

        alternatives_kanji.clear();
        alternatives_reading.clear();

        prev_kanji.clone_from(&kanji);
        prev_reading.clone_from(&reading);
        prev_key = Some(key);

        if kanji.is_empty() {
            result.push_str(&format!("{reading}\n"))
        } else {
            result.push_str(&format!("{kanji}  [{reading}]\n"));
        }
        result.push_str(&format!("{pos_conv}\n\n"));
        result.push_str(&gloss);

        if !examples.trim().is_empty() {
            result.push_str("\n\nExamples\n");

            for example in examples.split('\n').filter(|s| !s.trim().is_empty()) {
                result.push_str("  ");
                result.push_str(example.trim());
                result.push('\n');
            }
        }
    }

    // Finish the last entry.
    append_alternatives(
        &mut result,
        &alternatives_kanji,
        &alternatives_reading,
    );

    Ok(result)
}

fn append_alternatives(
    result: &mut String,
    kanji: &[String],
    readings: &[String],
) {
    if !kanji.is_empty() {
        result.push_str("\n\nOther spellings: ");
        result.push_str(&kanji.join(" · "));
    }

    if !readings.is_empty() {
        result.push_str("\nOther readings: ");
        result.push_str(&readings.join(" · "));
    }
}

pub fn lookup_word_for_token(db_path: &str, token: &Token) -> Result<String> {
    let preferred_pos = preferred_jmdict_pos(&token.part_of_speech);
    lookup_word(db_path, &token.dictionary_form, preferred_pos)
}

fn preferred_jmdict_pos(sudachi_pos: &[String]) -> Option<&'static str> {
    let pos = |index: usize| sudachi_pos.get(index).map(String::as_str);

    match pos(0) {
        // Particles
        Some("助詞") => Some("prt"),

        // Auxiliary verbs, e.g. だ, です, ます
        Some("助動詞") => Some("aux-v"),

        // Verbs
        Some("動詞") => match (pos(4), pos(5)) {
            // Sudachi's conjugation type
            (Some("上一段-カ行"), _)
            | (Some("上一段-ガ行"), _)
            | (Some("上一段-タ行"), _)
            | (Some("上一段-ダ行"), _)
            | (Some("上一段-ナ行"), _)
            | (Some("上一段-ハ行"), _)
            | (Some("上一段-バ行"), _)
            | (Some("上一段-マ行"), _)
            | (Some("上一段-ラ行"), _)
            | (Some("下一段-ア行"), _)
            | (Some("下一段-カ行"), _)
            | (Some("下一段-ガ行"), _)
            | (Some("下一段-サ行"), _)
            | (Some("下一段-ザ行"), _)
            | (Some("下一段-タ行"), _)
            | (Some("下一段-ダ行"), _)
            | (Some("下一段-ナ行"), _)
            | (Some("下一段-ハ行"), _)
            | (Some("下一段-バ行"), _)
            | (Some("下一段-マ行"), _)
            | (Some("下一段-ラ行"), _)
            | (Some("下一段-ワ行"), _) => Some("v1"),

            (Some("五段-カ行"), _) => Some("v5k"),
            (Some("五段-ガ行"), _) => Some("v5g"),
            (Some("五段-サ行"), _) => Some("v5s"),
            (Some("五段-タ行"), _) => Some("v5t"),
            (Some("五段-ナ行"), _) => Some("v5n"),
            (Some("五段-バ行"), _) => Some("v5b"),
            (Some("五段-マ行"), _) => Some("v5m"),
            (Some("五段-ラ行"), _) => Some("v5r"),
            (Some("五段-ワア行"), _) => Some("v5u"),

            (Some("サ行変格"), _) => Some("vs"),
            (Some("カ行変格"), _) => Some("vk"),

            // Fall back to the general verb category when the conjugation
            // type isn't recognized.
            _ => Some("v-unspec"),
        },

        // Adjectives
        Some("形容詞") => Some("adj-i"),

        // Adjectival nouns and other noun-like categories
        Some("名詞") => match pos(1) {
            Some("普通名詞") => Some("n"),
            Some("固有名詞") => Some("n"),
            Some("数詞") => Some("num"),
            Some("代名詞") => Some("pn"),
            _ => Some("n"),
        },

        // Adverbs
        Some("副詞") => Some("adv"),

        // Conjunctions
        Some("接続詞") => Some("conj"),

        // Interjections
        Some("感動詞") => Some("int"),

        // Prefixes and suffixes
        Some("接頭辞") => Some("pref"),
        Some("接尾辞") => Some("suf"),

        // Expressions and other symbols
        Some("補助記号") => None,
        Some("記号") => None,
        Some("空白") => None,

        _ => None,
    }
}

fn match_pos(pos: &str) -> String {
    let mut result: Vec<&str> = Vec::new();

    for tag in pos.split(',').map(str::trim).filter(|tag| !tag.is_empty()) {
        if !result.contains(&tag) {
            result.push(tag);
        }
    }

    result
        .iter()
        .map(|tag| match *tag {
            // Adjectives
            "adj-f"     => "Noun or verb acting prenominally",
            "adj-i"     => "い-adjective",
            "adj-ix"    => "い-adjective (良い class)",
            "adj-ku"    => "く-adjective",
            "adj-na"    => "な-adjective",
            "adj-nari"  => "なり-adjective",
            "adj-no"    => "の-adjective",
            "adj-pn"    => "Pre-noun adjectival",
            "adj-shiku" => "しく-adjective",
            "adj-t"     => "と-adjective",

            // Adverbs
            "adv"       => "Adverb",
            "adv-to"    => "と-adverb",

            // Auxiliary words and grammar
            "aux"       => "Auxiliary",
            "aux-adj"   => "Auxiliary adjective",
            "aux-v"     => "Auxiliary verb",
            "conj"      => "Conjunction",
            "cop"       => "Copula",
            "prt"       => "Particle",

            // Nouns and related tags
            "n"         => "Noun",
            "n-pref"    => "Noun used as a prefix",
            "n-suf"     => "Noun used as a suffix",
            "num"       => "Numeric",
            "pn"        => "Pronoun",
            "pref"      => "Prefix",
            "suf"       => "Suffix",
            "ctr"       => "Counter",
            "exp"       => "Expression",
            "int"       => "Interjection",
            "unc"       => "Unclassified",

            // Verbs: general
            "v-unspec"  => "Verb (unspecified class)",
            "vi"        => "Intransitive verb",
            "vt"        => "Transitive verb",
            "vn"        => "Noun or participle with the auxiliary する",
            "vk"        => "くる verb",
            "vr"        => "る verb (irregular)",
            "vs"        => "する verb",
            "vs-c"      => "する verb (compound)",
            "vs-i"      => "する verb (intransitive)",
            "vs-s"      => "する verb (special class)",
            "vz"        => "ずる verb",

            // Ichidan verbs
            "v1"        => "Ichidan verb",
            "v1-s"      => "Ichidan verb (くれる class)",

            // Nidan verbs (historical)
            "v2a-s"     => "Nidan verb (下二段, あ行)",
            "v2b-k"     => "Nidan verb (上二段, ば行)",
            "v2d-s"     => "Nidan verb (下二段, だ行)",
            "v2g-k"     => "Nidan verb (上二段, が行)",
            "v2g-s"     => "Nidan verb (下二段, が行)",
            "v2h-k"     => "Nidan verb (上二段, は行)",
            "v2h-s"     => "Nidan verb (下二段, は行)",
            "v2k-k"     => "Nidan verb (上二段, か行)",
            "v2k-s"     => "Nidan verb (下二段, か行)",
            "v2m-s"     => "Nidan verb (下二段, ま行)",
            "v2n-s"     => "Nidan verb (下二段, な行)",
            "v2r-k"     => "Nidan verb (上二段, ら行)",
            "v2r-s"     => "Nidan verb (下二段, ら行)",
            "v2s-s"     => "Nidan verb (下二段, さ行)",
            "v2t-k"     => "Nidan verb (上二段, た行)",
            "v2t-s"     => "Nidan verb (下二段, た行)",
            "v2w-s"     => "Nidan verb (下二段, わ行)",
            "v2y-k"     => "Nidan verb (上二段, や行)",
            "v2y-s"     => "Nidan verb (下二段, や行)",
            "v2z-s"     => "Nidan verb (下二段, ざ行)",

            // Yodan verbs (historical)
            "v4b"       => "Yodan verb (ば行)",
            "v4g"       => "Yodan verb (が行)",
            "v4h"       => "Yodan verb (は行)",
            "v4k"       => "Yodan verb (か行)",
            "v4m"       => "Yodan verb (ま行)",
            "v4r"       => "Yodan verb (ら行)",
            "v4s"       => "Yodan verb (さ行)",
            "v4t"       => "Yodan verb (た行)",

            // Godan verbs
            "v5aru"     => "Godan verb - ある special class",
            "v5b"       => "Godan verb (ぶ)",
            "v5g"       => "Godan verb (ぐ)",
            "v5k"       => "Godan verb (く)",
            "v5k-s"     => "Godan verb (く, special class)",
            "v5m"       => "Godan verb (む)",
            "v5n"       => "Godan verb (ぬ)",
            "v5r"       => "Godan verb (る)",
            "v5r-i"     => "Godan verb (る, irregular)",
            "v5s"       => "Godan verb (す)",
            "v5t"       => "Godan verb (つ)",
            "v5u"       => "Godan verb (う)",
            "v5u-s"     => "Godan verb (う, special class)",

            // Keep unrecognized tags visible instead of silently losing them
            other       => other,
        })
        .collect::<Vec<_>>()
        .join(", ")
}
