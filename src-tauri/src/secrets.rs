use keyring::Entry;
use thiserror::Error;

const SERVICE: &str = "com.useupclaude.claude-gui";
const ACCOUNT_API: &str = "anthropic_api_key";
/// Left behind by the removed Admin cost lookup. Deleted on startup, never read.
const LEGACY_ADMIN_ACCOUNT: &str = "anthropic_admin_api_key";

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("keyring error: {0}")]
    Keyring(String),
    #[error("API key is empty")]
    Empty,
    #[error("Admin API keys are not used for claude -p")]
    AdminKeyNotForCli,
}

fn entry(account: &str) -> Result<Entry, SecretError> {
    Entry::new(SERVICE, account).map_err(|e| SecretError::Keyring(e.to_string()))
}

fn read_secret(account: &str) -> Result<Option<String>, SecretError> {
    match entry(account)?.get_password() {
        Ok(password) if !password.is_empty() => Ok(Some(password)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(SecretError::Keyring(e.to_string())),
    }
}

fn write_secret(account: &str, secret: &str) -> Result<(), SecretError> {
    entry(account)?
        .set_password(secret)
        .map_err(|e| SecretError::Keyring(e.to_string()))
}

fn delete_secret(account: &str) -> Result<(), SecretError> {
    match entry(account)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(SecretError::Keyring(e.to_string())),
    }
}

/// Admin keys use the `sk-ant-admin` prefix. Chat keys use `sk-ant-api`.
pub fn is_admin_api_key(key: &str) -> bool {
    key.trim().starts_with("sk-ant-admin")
}

pub fn save_api_key(key: &str) -> Result<(), SecretError> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err(SecretError::Empty);
    }
    if is_admin_api_key(trimmed) {
        return Err(SecretError::AdminKeyNotForCli);
    }
    write_secret(ACCOUNT_API, trimmed)
}

pub fn load_api_key() -> Result<Option<String>, SecretError> {
    read_secret(ACCOUNT_API)
}

pub fn delete_api_key() -> Result<(), SecretError> {
    delete_secret(ACCOUNT_API)
}

pub fn forget_admin_api_key() -> Result<(), SecretError> {
    delete_secret(LEGACY_ADMIN_ACCOUNT)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_admin_keys_from_chat_keys() {
        assert!(is_admin_api_key("sk-ant-admin01-example"));
        assert!(is_admin_api_key("  sk-ant-admin01-example  "));
        assert!(!is_admin_api_key("sk-ant-api03-example"));
        assert!(!is_admin_api_key("sk-ant-example"));
    }
}
