//! docket: the desktop command line over the same core as the Android app.
//!
//! Pages come from `docket-ocr` (native/), which writes the OCR of a scan as JSON. The language
//! model is a local llama-server (`--llm http://127.0.0.1:8092`); without one, the rules extract
//! and search answers.
//!
//!   docket extract <ocr.json> [--unchecked]       what a document says (JSON), without storing it
//!   docket init <vault>                           a new vault (passphrase: DOCKET_PASSPHRASE or asked)
//!   docket add <vault> <ocr.json>...              store documents (each JSON is one document)
//!   docket list <vault>                           documents, next deadline first
//!   docket show <vault> <id>
//!   docket reminders <vault> [--today D] [--locale ro]
//!   docket ics <vault> [--today D] > docket.ics
//!   docket search <vault> "query"
//!   docket ask <vault> "question"

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use docket_core::DocketError;
use docket_core::api::{Docket, PageInput};
use docket_core::extract;
use docket_core::llm::LanguageModel;
use docket_core::model::{Line, Page};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Parser)]
#[command(
    name = "docket",
    about = "Your documents and their deadlines, on your own machine."
)]
struct Cli {
    /// llama-server URL (or DOCKET_LLM_URL); without one, rules only.
    #[arg(long, global = true)]
    llm: Option<String>,
    /// Today, YYYY-MM-DD (default: the system date).
    #[arg(long, global = true)]
    today: Option<String>,
    /// Language of reminders and answers: en or ro.
    #[arg(long, global = true, default_value = "en")]
    locale: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Extract {
        ocr: PathBuf,
        /// Print the model's answer as it is, without the checks (for the evaluation).
        #[arg(long)]
        unchecked: bool,
    },
    Init {
        vault: PathBuf,
    },
    Add {
        vault: PathBuf,
        ocr: Vec<PathBuf>,
    },
    List {
        vault: PathBuf,
    },
    Show {
        vault: PathBuf,
        id: String,
    },
    Reminders {
        vault: PathBuf,
    },
    Ics {
        vault: PathBuf,
    },
    Search {
        vault: PathBuf,
        query: String,
        /// The hits as JSON (for the evaluation).
        #[arg(long)]
        json: bool,
    },
    Ask {
        vault: PathBuf,
        question: String,
    },
}

/// What docket-ocr writes for one document.
#[derive(Deserialize)]
struct OcrDoc {
    pages: Vec<OcrPage>,
}

#[derive(Deserialize)]
struct OcrPage {
    /// The corrected page image, relative to the JSON file.
    image: Option<String>,
    text: String,
    #[serde(default)]
    lines: Vec<Line>,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    confidence: f32,
}

/// A llama-server, asked for JSON matching a schema.
struct Http {
    url: String,
    agent: ureq::Agent,
    name: String,
}

impl Http {
    fn new(url: &str) -> Http {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(600)))
            .build()
            .into();
        let url = url.trim_end_matches('/').to_string();
        // The model's name, from the server, for the record.
        let name = agent
            .get(&format!("{url}/v1/models"))
            .call()
            .ok()
            .and_then(|mut r| r.body_mut().read_json::<Value>().ok())
            .and_then(|v| {
                v["data"][0]["id"]
                    .as_str()
                    .map(|s| s.rsplit(['/', '\\']).next().unwrap_or(s).to_string())
            })
            .unwrap_or_else(|| "llama-server".into());
        Http { url, agent, name }
    }
}

impl LanguageModel for Http {
    fn complete(
        &self,
        system: String,
        user: String,
        json_schema: String,
    ) -> Result<String, DocketError> {
        let schema: Value =
            serde_json::from_str(&json_schema).map_err(|e| DocketError::Model(e.to_string()))?;
        let body = json!({
            "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
            "temperature": 0, "seed": 1, "max_tokens": 1024,
            "response_format": {"type": "json_schema", "json_schema": {"name": "answer", "strict": true, "schema": schema}}
        });
        let mut resp = self
            .agent
            .post(&format!("{}/v1/chat/completions", self.url))
            .send_json(&body)
            .map_err(|e| DocketError::Model(format!("{}: {e}", self.url)))?;
        let v: Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| DocketError::Model(e.to_string()))?;
        v["choices"][0]["message"]["content"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| DocketError::Model(format!("unexpected reply: {v}")))
    }

    fn name(&self) -> String {
        self.name.clone()
    }
}

fn read_ocr(path: &Path) -> Result<Vec<(Page, Vec<u8>)>, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc: OcrDoc =
        serde_json::from_slice(&data).map_err(|e| format!("{}: {e}", path.display()))?;
    let base = path.parent().unwrap_or(Path::new("."));
    doc.pages
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            let image = match &p.image {
                Some(rel) => std::fs::read(base.join(rel)).map_err(|e| format!("{rel}: {e}"))?,
                None => Vec::new(),
            };
            let page = Page {
                number: i as u32 + 1,
                text: p.text,
                lines: p.lines,
                width: p.width,
                height: p.height,
                confidence: p.confidence,
            };
            Ok((page, image))
        })
        .collect()
}

fn passphrase(confirm: bool) -> String {
    if let Ok(p) = std::env::var("DOCKET_PASSPHRASE") {
        return p;
    }
    let ask = |prompt: &str| {
        eprint!("{prompt}");
        io::stderr().flush().ok();
        let mut s = String::new();
        io::stdin().lock().read_line(&mut s).ok();
        s.trim_end_matches(['\r', '\n']).to_string()
    };
    let p = ask("Passphrase: ");
    if confirm && ask("Again: ") != p {
        eprintln!("the passphrases differ");
        std::process::exit(2);
    }
    p
}

fn print(v: &impl serde::Serialize) {
    println!("{}", serde_json::to_string_pretty(v).unwrap());
}

fn main() {
    let cli = Cli::parse();
    let today = cli
        .today
        .clone()
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
    let llm_url = cli
        .llm
        .clone()
        .or_else(|| std::env::var("DOCKET_LLM_URL").ok());
    let model: Option<Arc<dyn LanguageModel>> = llm_url
        .as_deref()
        .map(|u| Arc::new(Http::new(u)) as Arc<dyn LanguageModel>);
    if let Err(e) = run(cli, &today, model) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn open(vault: &Path, model: Option<Arc<dyn LanguageModel>>) -> Result<Arc<Docket>, String> {
    let d = Docket::open_with_passphrase(vault, &passphrase(false)).map_err(|e| e.to_string())?;
    d.set_model(model);
    Ok(d)
}

fn run(cli: Cli, today: &str, model: Option<Arc<dyn LanguageModel>>) -> Result<(), String> {
    let locale = cli.locale;
    match cli.cmd {
        Cmd::Extract { ocr, unchecked } => {
            let pages: Vec<Page> = read_ocr(&ocr)?.into_iter().map(|(p, _)| p).collect();
            if unchecked {
                let m = model.ok_or("--unchecked needs a model (--llm)")?;
                let (system, user, schema) = extract::prompt(&pages, extract::rules(&pages).0);
                let t = std::time::Instant::now();
                let raw = m
                    .complete(system, user, schema)
                    .map_err(|e| e.to_string())?;
                let mut v: Value = serde_json::from_str(&raw)
                    .map_err(|e| format!("{e}; the answer was: {raw}"))?;
                v["extraction"] = json!({"method": m.name(), "seconds": t.elapsed().as_secs_f64(), "rejected": []});
                print(&v);
                return Ok(());
            }
            let e = extract::extract(&pages, model.as_deref());
            print(&json!({
                "kind": e.kind, "title": e.title, "fields": e.fields, "deadlines": e.deadlines, "extraction": e.extraction
            }));
        }
        Cmd::Init { vault } => {
            Docket::create_with_passphrase(&vault, &passphrase(true)).map_err(|e| e.to_string())?;
            eprintln!("created an encrypted vault in {}", vault.display());
        }
        Cmd::Add { vault, ocr } => {
            let d = open(&vault, model)?;
            for path in ocr {
                let pages = read_ocr(&path)?
                    .into_iter()
                    .map(|(p, image)| PageInput {
                        image,
                        text: p.text,
                        lines: p.lines,
                        width: p.width,
                        height: p.height,
                        confidence: p.confidence,
                    })
                    .collect();
                let doc = d
                    .add_document(pages, today.to_string())
                    .map_err(|e| e.to_string())?;
                let next = doc
                    .deadlines
                    .first()
                    .map(|x| format!("{} {}", x.kind.name(), x.date))
                    .unwrap_or_else(|| "no deadline".into());
                println!(
                    "{}  {:<20} {}  ({next})",
                    doc.id,
                    doc.kind.name(),
                    doc.title
                );
            }
        }
        Cmd::List { vault } => {
            for s in open(&vault, model)?.documents(today.to_string()) {
                let next = s
                    .next_deadline
                    .map(|x| format!("{} {}", x.date, x.kind.name()))
                    .unwrap_or_default();
                println!("{}  {:<20} {:<45} {next}", s.id, s.kind.name(), s.title);
            }
        }
        Cmd::Show { vault, id } => print(
            &open(&vault, model)?
                .document(id)
                .map_err(|e| e.to_string())?,
        ),
        Cmd::Reminders { vault } => {
            for r in open(&vault, model)?
                .reminders(today.to_string(), locale)
                .map_err(|e| e.to_string())?
            {
                println!("{}  {:<8?} {} — {}", r.date, r.urgency, r.title, r.body);
            }
        }
        Cmd::Ics { vault } => {
            let now = chrono::Utc::now().to_rfc3339();
            print!(
                "{}",
                open(&vault, model)?
                    .calendar(today.to_string(), now, locale)
                    .map_err(|e| e.to_string())?
            );
        }
        Cmd::Search { vault, query, json } => {
            let hits = open(&vault, model)?.search(query, 5, today.to_string());
            if json {
                print(&hits);
                return Ok(());
            }
            for h in hits {
                println!(
                    "{:.2}  {} p.{}{}  {}",
                    h.score,
                    h.title,
                    h.page,
                    if h.fact { " (fact)" } else { "" },
                    h.text.chars().take(120).collect::<String>()
                );
            }
        }
        Cmd::Ask { vault, question } => {
            print(&open(&vault, model)?.ask(question, locale, today.to_string()))
        }
    }
    Ok(())
}
