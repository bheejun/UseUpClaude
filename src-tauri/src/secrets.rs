use keyring::Entry;
use thiserror::Error;

const SERVICE: &str = "com.useupclaude.claude-gui";
const ACCOUNT: &str = "anthropic_api_key";

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("keyring error: {0}")]
    Keyring(String),
    #[error("API key is empty")]
    Empty,
}

fn entry() -> Result<Entry, SecretError> {
    Entry::new(SERVICE, ACCOUNT).map_err(|e| SecretError::Keyring(e.to_string()))
}

pub fn save_api_key(key: &str) -> Result<(), SecretError> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err(SecretError::Empty);
    }
    entry()?
        .set_password(trimmed)
        .map_err(|e| SecretError::Keyring(e.to_string()))
}

pub fn load_api_key() -> Result<Option<String>, SecretError> {
    match entry()?.get_password() {
        Ok(p) if !p.is_empty() => Ok(Some(p)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(SecretError::Keyring(e.to_string())),
    }
}

pub fn delete_api_key() -> Result<(), SecretError> {
    match entry()?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(SecretError::Keyring(e.to_string())),
    }
}

pub fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 8 {
        return "••••••••".into();
    }
    let head: String = chars.iter().take(4).collect();
    let tail: String = chars.iter().rev().take(4).rev().collect();
    format!("{head}…{tail}")
}
