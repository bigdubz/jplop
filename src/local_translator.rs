use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use std::{
    net::TcpListener,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::paths::{model, llama};

#[derive(Clone, Copy, Debug)]
pub enum Direction {
    EnglishToJapanese,
    JapaneseToEnglish,
}

pub struct Translator {
    name: String,
    client: Client,
    child: Option<Child>,
    port: u16,
}

impl Translator {

    pub fn start(
        cuda_server_exe: impl AsRef<Path>,
        model_path: impl AsRef<Path>,
    ) -> Result<Self> {
        let cuda_server_exe = cuda_server_exe
            .as_ref()
            .canonicalize()
            .context("Cannot find CUDA llama-server.exe")?;

        let cpu_server_exe = cuda_server_exe
            .parent()
            .and_then(Path::parent)
            .map(|cuda_dir| cuda_dir.join("cpu").join("llama-server.exe"))
            .context("Cannot determine CPU llama-server.exe path")?
            .canonicalize()
            .context("Cannot find CPU llama-server.exe")?;

        let model_path = model_path
            .as_ref()
            .canonicalize()
            .context("Cannot find the GGUF model")?;

        let name = model_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("model")
            .to_string();

        let client = Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(180))
            .build()
            .context("Could not create HTTP client")?;

        // CUDA first, then the separate CPU build.
        let attempts = [
            ("CUDA", cuda_server_exe, "99"),
            ("CPU", cpu_server_exe, "0"),
        ];

        let mut errors = Vec::new();

        for (mode, server_exe, gpu_layers) in attempts {
            eprintln!("Starting {name} using {mode} mode...");

            // Allocate a fresh port for each attempt.
            let listener = TcpListener::bind(("127.0.0.1", 0))
                .context("Could not allocate a local port")?;
            let port = listener.local_addr()?.port();
            drop(listener);

            let port_arg = port.to_string();
            let base_url = format!("http://127.0.0.1:{port}");
            let health_url = format!("{base_url}/health");

            let mut command = Command::new(&server_exe);
            command
                .arg("-m")
                .arg(&model_path)
                .arg("--jinja")
                .args([
                    "-ngl",
                    gpu_layers,
                    "-c",
                    "4096",
                    "--host",
                    "127.0.0.1",
                    "--port",
                    &port_arg,
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());

            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000); // CREATE_NO_WINDOW
            }

            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(error) => {
                    let message = format!("{mode}: failed to spawn server: {error}");
                    eprintln!("{message}");
                    errors.push(message);
                    continue;
                }
            };

            let deadline = Instant::now() + Duration::from_secs(180);
            let mut startup_error: Option<String> = None;

            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        startup_error = Some(format!(
                            "{mode}: server exited during startup: {status}"
                        ));
                        break;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        startup_error = Some(format!(
                            "{mode}: could not check server process: {error}"
                        ));
                        break;
                    }
                }

                let ready = client
                    .get(&health_url)
                    .timeout(Duration::from_secs(2))
                    .send()
                    .is_ok_and(|response| response.status().is_success());

                if ready {
                    eprintln!("{name} is ready using {mode} mode.");

                    return Ok(Self {
                        name,
                        client,
                        child: Some(child),
                        port,
                    });
                }

                if Instant::now() >= deadline {
                    startup_error = Some(format!(
                        "{mode}: timed out waiting for {name} to load"
                    ));
                    break;
                }

                thread::sleep(Duration::from_millis(250));
            }

            // Clean up the failed attempt before trying the next executable.
            let message = startup_error
                .unwrap_or_else(|| format!("{mode}: startup failed"));

            eprintln!("{message}");
            errors.push(message);

            let _ = child.kill();
            let _ = child.wait();

            if mode == "CUDA" {
                eprintln!("CUDA startup failed; falling back to CPU...");
            }
        }

        bail!(
        "Could not start {name} with either CUDA or CPU:\n{}",
        errors.join("\n")
        )
    }


    /// Translates one text using a fresh prompt, with no chat history.

    pub fn translate(&self, text: &str, direction: Direction) -> Result<String> {
        let text = text.trim();
        let name = self.name.clone();
        let dir = match direction {
            Direction::JapaneseToEnglish => "english",
            Direction::EnglishToJapanese => "japanese"
        };
        println!("[info]\t{name} is translating \"{text}\" to {dir}");

        if text.is_empty() {
            return Ok(String::new());
        }

        let system_prompt: &str = match direction {
            Direction::EnglishToJapanese => "Translate to Japanese.",
            Direction::JapaneseToEnglish => "Translate to English."
        };

        let max_tokens = (text.chars().count() * 3 + 32).clamp(64, 512) as u32;

        let request_body = serde_json::json!({
            "messages": [
                {
                    "role": "system",
                    "content": system_prompt
                },
                {
                    "role": "user",
                    "content": text
                }
            ],
            // recommended params and values from https://huggingface.co/LiquidAI/LFM2-350M-ENJP-MT
            "temperature": 0.5,
            "top_p": 1.0,
            "min_p": 0.1,
            "max_tokens": max_tokens,
            "repetition_penalty": 1.05,
        });

        let http_response = self.client
            .post(format!(
                "http://127.0.0.1:{}/v1/chat/completions",
                self.port
            ))
            .json(&request_body)
            .send()
            .context("Failed to contact llama-server")?;

        let status = http_response.status();

        let raw_body = http_response
            .text()
            .context("Could not read translation response")?;

        if !status.is_success() {
            bail!("llama-server returned {status}: {raw_body}");
        }

        let json: serde_json::Value =
            serde_json::from_str(&raw_body)
                .context("Invalid response from llama-server")?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| {
                anyhow::anyhow!("Missing translation in response: {}", json)
            })?;

        let translation = content.trim();

        if translation.is_empty() {
            bail!("{name} returned an empty translation");
        }

        println!("[info]\t{name} translated: \"{translation}\" (supposedly {dir})");
        Ok(translation.to_string())
    }

    pub fn to_japanese(&self, text: &str) -> Result<String> {
        self.translate(
            text,
            Direction::EnglishToJapanese,
        )
    }

    pub fn to_english(&self, text: &str) -> Result<String> {
        self.translate(
            text,
            Direction::JapaneseToEnglish,
        )
    }
}

impl Drop for Translator {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub fn load_lfm() -> Result<Translator> {
    println!("[info]\tloading LFM2-f16...");
    let llama_server_path = llama();
    let model_path = model();
    let result = Translator::start(llama_server_path, model_path);
    if result.is_ok() {
        println!("[info]\tLFM2-f16 loaded");
    }
    result
}

#[allow(dead_code)]
pub fn test_translation_quality(model: Translator) {
    model.to_japanese("I ate sushi yesterday.").unwrap();
    model.to_japanese("She is reading a book in her room.").unwrap();
    model.to_japanese("I have never been to Japan.").unwrap();
    model.to_japanese("He was running when it started to rain.").unwrap();
    model.to_japanese("I want to learn Japanese because I love Japanese culture.").unwrap();

    model.to_japanese("I gave my friend a book.").unwrap();
    model.to_japanese("My friend gave me a book.").unwrap();
    model.to_japanese("I received a present from my teacher.").unwrap();
    model.to_japanese("Could you please teach me how to use this?").unwrap();
    model.to_japanese("Even though I was tired, I kept working.").unwrap();

    model.to_japanese("I thought he had already left.").unwrap();
    model.to_japanese("If I had known, I would have helped you.").unwrap();
    model.to_japanese("You don't have to come tomorrow.").unwrap();
    model.to_japanese("I was told not to open the door.").unwrap();
    model.to_japanese("She seems to have forgotten about the meeting.").unwrap();

    model.to_japanese("The more I study, the more interesting Japanese becomes.").unwrap();
    model.to_japanese("I ended up buying something I didn't need.").unwrap();
    model.to_japanese("I tried eating natto for the first time.").unwrap();
    model.to_japanese("He stopped smoking three years ago.").unwrap();
    model.to_japanese("I forgot to bring my umbrella.").unwrap();

    model.to_japanese("The beautiful flowers in the garden have begun to bloom.").unwrap();
    model.to_japanese("She quickly ran across the busy street.").unwrap();
    model.to_japanese("Although the movie was long, it was never boring.").unwrap();
    model.to_japanese("I will probably stay home unless the weather improves.").unwrap();
    model.to_japanese("Neither my brother nor my sister knows the answer.").unwrap();

    model.to_japanese("Let's meet at Tokyo Station at 8:30 tomorrow morning.").unwrap();
    model.to_japanese("Wait, you actually bought all of these?!").unwrap();
    model.to_japanese("Mr. Tanaka said, \"I'll be back in a few minutes.\"").unwrap();
    model.to_japanese("I like cats, dogs, birds, and other animals.").unwrap();
    model.to_japanese("Please open the window.").unwrap();

    model.to_english("昨日、寿司を食べました。").unwrap();
    model.to_english("彼女は今、自分の部屋で本を読んでいます。").unwrap();
    model.to_english("私は日本に行ったことがありません。").unwrap();
    model.to_english("雨が降り始めたとき、彼は走っていました。").unwrap();
    model.to_english("日本の文化が好きなので、日本語を勉強したいです。").unwrap();

    model.to_english("友達に本をあげました。").unwrap();
    model.to_english("友達が私に本をくれました。").unwrap();
    model.to_english("先生からプレゼントをいただきました。").unwrap();
    model.to_english("これの使い方を教えていただけますか？").unwrap();
    model.to_english("疲れていたにもかかわらず、仕事を続けました。").unwrap();

    model.to_english("彼はもう出て行ったと思っていました。").unwrap();
    model.to_english("もし知っていたら、あなたを助けたのに。").unwrap();
    model.to_english("明日は来なくても大丈夫です。").unwrap();
    model.to_english("ドアを開けないように言われました。").unwrap();
    model.to_english("彼女は会議のことを忘れてしまったようです。").unwrap();

    model.to_english("勉強すればするほど、日本語が面白くなります。").unwrap();
    model.to_english("結局、必要のないものを買ってしまいました。").unwrap();
    model.to_english("初めて納豆を食べてみました。").unwrap();
    model.to_english("彼は3年前にタバコをやめました。").unwrap();
    model.to_english("傘を持ってくるのを忘れました。").unwrap();

    model.to_english("庭の美しい花々が咲き始めました。").unwrap();
    model.to_english("彼女は人通りの多い通りを急いで走り抜けました。").unwrap();
    model.to_english("その映画は長かったけれど、一度も退屈しませんでした。").unwrap();
    model.to_english("天気が良くならない限り、たぶん家にいると思います。").unwrap();
    model.to_english("兄も妹も、その答えを知りません。").unwrap();

    model.to_english("明日の朝8時30分に東京駅で会いましょう。").unwrap();
    model.to_english("えっ、本当にこれを全部買ったの！？").unwrap();
    model.to_english("田中さんは「数分後に戻ります」と言いました。").unwrap();
    model.to_english("私は猫も犬も鳥も、ほかの動物も好きです。").unwrap();
    model.to_english("すみませんが、最寄りの駅がどこにあるか教えていただけますか？").unwrap();

    model.to_english("行かなくてもいいと言われたけれど、行かないほうがいいとは言われていません。").unwrap();
    model.to_english("彼が来るとは思っていなかったわけではありません。").unwrap();
    model.to_english("雨が降っていたら、試合は中止になっていたでしょう。").unwrap();
    model.to_english("彼女はまだ帰っていないはずです。").unwrap();
    model.to_english("この薬を飲んだからといって、すぐに治るとは限りません。").unwrap();
    model.to_english("彼に本当のことを言わないでおいた。").unwrap();
    model.to_english("もう少しで電車に乗り遅れるところでした。").unwrap();
    model.to_english("彼は忙しいと言いながら、毎日遊びに出かけています。").unwrap();
    model.to_english("私が頼んだのは、資料を送ってほしいということだけです。").unwrap();
    model.to_english("別に嫌いなわけじゃないけど、今は食べたくない。").unwrap();
}

