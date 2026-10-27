//! The language model, as the core sees it: something that completes a prompt with JSON matching
//! a schema. On Android it is llama.cpp running on the phone (C++, through the app); on the desktop
//! a local llama-server. The core never sends anything anywhere itself.

use crate::Error;

#[uniffi::export(with_foreign)]
pub trait LanguageModel: Send + Sync {
    /// Completes `user` under `system`, answering with JSON that matches `json_schema`.
    fn complete(&self, system: String, user: String, json_schema: String) -> Result<String, Error>;

    /// A short name for the record ("Qwen2.5-3B-Instruct Q4_K_M").
    fn name(&self) -> String;
}
