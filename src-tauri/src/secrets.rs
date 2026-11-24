//! API keys live in the OS credential store (Windows Credential Manager,
//! macOS Keychain, Linux keyutils), keyed by provider id.

use keyring::{Entry, Error};

const SERVICE: &str = "ai-usage-widget";

fn entry(provider_id: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, provider_id).map_err(|e| e.to_string())
}

pub fn get(provider_id: &str) -> Result<Option<String>, String> {
    match entry(provider_id)?.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn set(provider_id: &str, secret: &str) -> Result<(), String> {
    entry(provider_id)?.set_password(secret).map_err(|e| e.to_string())
}

pub fn delete(provider_id: &str) -> Result<(), String> {
    match entry(provider_id)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
