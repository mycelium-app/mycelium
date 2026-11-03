use crate::config::Config;
use aes_gcm::aead::{Aead, Generate, Key as CipherKey, KeyInit, Nonce};
use aes_gcm::Aes256Gcm;
use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::postgres::{PgArgumentBuffer, PgTypeInfo, PgValueRef};
use sqlx::{Decode, Encode, PgPool, Postgres, Type};
use std::io::Write;
use std::ops::Deref;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::{OnceLock, RwLock};

const PREFIX: &str = "enc:v1:";
const KEY_FILE: &str = "secret.key";
const NONCE_LENGTH: usize = 12;
const SEALED_SETTINGS: [&str; 1] = ["encryption_password"];
const SEALED_COLUMNS: [(&str, &str, &str); 8] = [
    ("forges", "token", "token = $2"),
    ("model_providers", "api_key", "api_key = $2"),
    ("decision_providers", "api_key", "api_key = $2"),
    ("chat_tool_servers", "bearer_token", "bearer_token = $2"),
    ("chat_tool_servers", "oauth_refresh_token", "oauth_refresh_token = $2"),
    ("chat_tool_servers", "session_key", "session_key = $2"),
    ("chat_tool_servers", "settings->>'encryption_password'", "settings = jsonb_set(settings, '{encryption_password}', to_jsonb($2::text))"),
    ("assistant_secrets", "value", "value = $2"),
];

static INSTALLED: OnceLock<RwLock<KeyRing>> = OnceLock::new();

#[derive(Clone)]
struct Key {
    id: String,
    cipher: Aes256Gcm,
}

#[derive(Clone)]
pub struct KeyRing {
    current: Key,
    previous: Vec<Key>,
}

fn parse_key(encoded: &str) -> Result<Key> {
    let bytes = STANDARD.decode(encoded.trim()).context("A secret key is not valid base64.")?;

    if bytes.len() != 32 {
        bail!("A secret key must be 32 bytes, base64 encoded. Generate one with `openssl rand -base64 32`.");
    }

    let id = Sha256::digest(&bytes).iter().take(4).map(|byte| format!("{byte:02x}")).collect();
    let key = CipherKey::<Aes256Gcm>::try_from(bytes.as_slice()).map_err(|_| anyhow::anyhow!("A secret key must be 32 bytes."))?;

    Ok(Key { id, cipher: Aes256Gcm::new(&key) })
}

pub fn generate_key() -> String {
    STANDARD.encode(rand::random::<[u8; 32]>())
}

impl KeyRing {
    pub fn parse(current: &str, previous: &[String]) -> Result<KeyRing> {
        let previous = previous.iter().map(|key| parse_key(key)).collect::<Result<Vec<_>>>()?;

        Ok(KeyRing { current: parse_key(current)?, previous })
    }

    pub fn seal(&self, plaintext: &str) -> String {
        let nonce = Nonce::<Aes256Gcm>::generate();
        let ciphertext = self.current.cipher.encrypt(&nonce, plaintext.as_bytes()).expect("AES-GCM encrypts any message this short.");
        let mut packed = nonce.to_vec();

        packed.extend(ciphertext);

        format!("{PREFIX}{}:{}", self.current.id, URL_SAFE_NO_PAD.encode(packed))
    }

    pub fn open(&self, stored: &str) -> Result<String> {
        let Some(rest) = stored.strip_prefix(PREFIX) else {
            return Ok(stored.to_string());
        };
        let (key_id, packed) = rest.split_once(':').context("A stored secret is malformed.")?;
        let key = std::iter::once(&self.current).chain(&self.previous).find(|key| key.id == key_id).with_context(|| format!("A stored secret was sealed with key {key_id}, which is not configured. Add it to MYCELIUM_SECRET_KEY_PREVIOUS."))?;
        let packed = URL_SAFE_NO_PAD.decode(packed).context("A stored secret is malformed.")?;

        if packed.len() < NONCE_LENGTH {
            bail!("A stored secret is malformed.");
        }

        let (nonce, ciphertext) = packed.split_at(NONCE_LENGTH);
        let nonce = Nonce::<Aes256Gcm>::try_from(nonce).map_err(|_| anyhow::anyhow!("A stored secret is malformed."))?;
        let plaintext = key.cipher.decrypt(&nonce, ciphertext).map_err(|_| anyhow::anyhow!("A stored secret did not decrypt with key {key_id}."))?;

        Ok(String::from_utf8(plaintext)?)
    }

    fn is_current(&self, stored: &str) -> bool {
        stored.strip_prefix(PREFIX).is_some_and(|rest| rest.split_once(':').is_some_and(|(key_id, _)| key_id == self.current.id))
    }
}

fn read_key_file(path: &Path) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("Could not read the secret key file {}.", path.display()))?;

    Ok(text.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_string).collect())
}

pub fn load(config: &Config) -> Result<KeyRing> {
    let path = config.data_dir.join(KEY_FILE);

    if let Some(current) = &config.secret_key {
        let mut previous = config.previous_secret_keys.clone();

        if path.exists() {
            previous.extend(read_key_file(&path)?);
        }

        return KeyRing::parse(current, &previous);
    }

    if !path.exists() {
        std::fs::create_dir_all(&config.data_dir)?;

        let created = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path).and_then(|mut file| file.write_all(format!("{}\n", generate_key()).as_bytes()));

        match created {
            Ok(()) => tracing::info!("no MYCELIUM_SECRET_KEY is set, so a secret key was generated at {}. Back this file up: without it the stored secrets cannot be read.", path.display()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error).with_context(|| format!("Could not create the secret key file {}.", path.display())),
        }
    }

    let keys = read_key_file(&path)?;
    let Some((current, retired)) = keys.split_first() else {
        bail!("The secret key file {} is empty.", path.display());
    };
    let mut previous = config.previous_secret_keys.clone();

    previous.extend(retired.iter().cloned());

    KeyRing::parse(current, &previous)
}

pub fn retire_current_key(config: &Config) -> Result<KeyRing> {
    if config.secret_key.is_some() {
        return load(config);
    }

    let path = config.data_dir.join(KEY_FILE);
    let mut keys = if path.exists() { read_key_file(&path)? } else { Vec::new() };

    keys.insert(0, generate_key());
    std::fs::write(&path, format!("{}\n", keys.join("\n")))?;

    load(config)
}

pub fn install(ring: KeyRing) {
    match INSTALLED.get() {
        Some(installed) => {
            if let Ok(mut current) = installed.write() {
                *current = ring;
            }
        }
        None => {
            let _ = INSTALLED.set(RwLock::new(ring));
        }
    }
}

fn installed() -> Result<KeyRing, BoxDynError> {
    let installed = INSTALLED.get().ok_or("The secret key has not been loaded.")?;
    let ring = installed.read().map_err(|_| "The secret key is unavailable.")?;

    Ok(ring.clone())
}

pub fn installed_ring() -> Result<KeyRing> {
    installed().map_err(|error| anyhow::anyhow!("{error}"))
}

pub async fn reseal(pool: &PgPool, ring: &KeyRing) -> Result<u64> {
    let mut changed = 0;

    for (table, read, write) in SEALED_COLUMNS {
        let mut transaction = pool.begin().await?;
        let rows = sqlx::query_as::<_, (String, String)>(sqlx::AssertSqlSafe(format!("select ctid::text, {read} from {table} where {read} is not null for update"))).fetch_all(&mut *transaction).await?;

        for (row, stored) in rows {
            if ring.is_current(&stored) {
                continue;
            }

            let sealed = ring.seal(&ring.open(&stored)?);

            sqlx::query(sqlx::AssertSqlSafe(format!("update {table} set {write} where ctid = $1::text::tid"))).bind(row).bind(sealed).execute(&mut *transaction).await?;
            changed += 1;
        }

        transaction.commit().await?;
    }

    Ok(changed)
}

#[derive(Clone, PartialEq)]
pub struct Sealed(String);

impl Sealed {
    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Debug for Sealed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Sealed(..)")
    }
}

impl Deref for Sealed {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl From<String> for Sealed {
    fn from(value: String) -> Self {
        Sealed(value)
    }
}

impl From<&str> for Sealed {
    fn from(value: &str) -> Self {
        Sealed(value.to_string())
    }
}

impl Type<Postgres> for Sealed {
    fn type_info() -> PgTypeInfo {
        <String as Type<Postgres>>::type_info()
    }

    fn compatible(ty: &PgTypeInfo) -> bool {
        <String as Type<Postgres>>::compatible(ty)
    }
}

impl Encode<'_, Postgres> for Sealed {
    fn encode_by_ref(&self, buffer: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        let sealed = installed()?.seal(&self.0);

        <String as Encode<Postgres>>::encode(sealed, buffer)
    }
}

impl Decode<'_, Postgres> for Sealed {
    fn decode(value: PgValueRef<'_>) -> Result<Self, BoxDynError> {
        let stored = <String as Decode<Postgres>>::decode(value)?;
        let opened = installed()?.open(&stored)?;

        Ok(Sealed(opened))
    }
}

#[derive(Clone, Debug)]
pub struct SealedSettings(pub Value);

impl Deref for SealedSettings {
    type Target = Value;

    fn deref(&self) -> &Value {
        &self.0
    }
}

impl Type<Postgres> for SealedSettings {
    fn type_info() -> PgTypeInfo {
        <sqlx::types::Json<Value> as Type<Postgres>>::type_info()
    }

    fn compatible(ty: &PgTypeInfo) -> bool {
        <sqlx::types::Json<Value> as Type<Postgres>>::compatible(ty)
    }
}

impl Encode<'_, Postgres> for SealedSettings {
    fn encode_by_ref(&self, buffer: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        let mut settings = self.0.clone();

        for field in SEALED_SETTINGS {
            if let Some(plaintext) = settings[field].as_str().map(str::to_string) {
                settings[field] = Value::String(installed()?.seal(&plaintext));
            }
        }

        <sqlx::types::Json<Value> as Encode<Postgres>>::encode(sqlx::types::Json(settings), buffer)
    }
}

impl Decode<'_, Postgres> for SealedSettings {
    fn decode(value: PgValueRef<'_>) -> Result<Self, BoxDynError> {
        let sqlx::types::Json(mut settings) = <sqlx::types::Json<Value> as Decode<Postgres>>::decode(value)?;

        for field in SEALED_SETTINGS {
            if let Some(stored) = settings[field].as_str().map(str::to_string) {
                settings[field] = Value::String(installed()?.open(&stored)?);
            }
        }

        Ok(SealedSettings(settings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::os::unix::fs::PermissionsExt;

    fn config_in(directory: &Path) -> Config {
        let values: HashMap<String, String> = [("MYCELIUM_DATABASE_URL", "postgres://x"), ("MYCELIUM_PUBLIC_URL", "http://127.0.0.1:8080"), ("MYCELIUM_DEV_LOGIN", "dev@example.com"), ("MYCELIUM_DATA_DIR", directory.to_str().unwrap())].iter().map(|(key, value)| (key.to_string(), value.to_string())).collect();

        Config::from_lookup(&|name| values.get(name).cloned()).unwrap()
    }

    #[test]
    fn a_sealed_value_opens_to_what_was_sealed_and_never_shows_it() {
        let ring = KeyRing::parse(&generate_key(), &[]).unwrap();
        let sealed = ring.seal("glpat-example");

        assert!(sealed.starts_with(PREFIX) && !sealed.contains("glpat-example"));
        assert_eq!(ring.open(&sealed).unwrap(), "glpat-example");
    }

    #[test]
    fn a_value_that_was_never_sealed_opens_as_it_is() {
        let ring = KeyRing::parse(&generate_key(), &[]).unwrap();

        assert_eq!(ring.open("plain-token").unwrap(), "plain-token");
    }

    #[test]
    fn a_value_sealed_under_a_retired_key_opens_while_that_key_is_listed_and_not_after() {
        let old = generate_key();
        let sealed = KeyRing::parse(&old, &[]).unwrap().seal("old-secret");
        let rotated = KeyRing::parse(&generate_key(), std::slice::from_ref(&old)).unwrap();
        let forgotten = KeyRing::parse(&generate_key(), &[]).unwrap();

        assert_eq!(rotated.open(&sealed).unwrap(), "old-secret");
        assert!(!rotated.is_current(&sealed));
        assert!(forgotten.open(&sealed).unwrap_err().to_string().contains("MYCELIUM_SECRET_KEY_PREVIOUS"));
    }

    #[test]
    fn a_key_that_is_not_thirty_two_bytes_is_refused() {
        assert!(KeyRing::parse(&STANDARD.encode([1u8; 16]), &[]).is_err());
    }

    #[test]
    fn without_a_configured_key_one_is_generated_into_the_data_directory_readable_only_by_its_owner() {
        let directory = tempfile::tempdir().unwrap();
        let config = config_in(directory.path());

        let first = load(&config).unwrap();
        let second = load(&config).unwrap();
        let mode = std::fs::metadata(directory.path().join(KEY_FILE)).unwrap().permissions().mode() & 0o777;

        assert_eq!(mode, 0o600);
        assert_eq!(second.open(&first.seal("kept")).unwrap(), "kept");
    }

    #[test]
    fn retiring_the_generated_key_keeps_the_old_one_readable() {
        let directory = tempfile::tempdir().unwrap();
        let config = config_in(directory.path());
        let sealed = load(&config).unwrap().seal("kept");

        let rotated = retire_current_key(&config).unwrap();

        assert!(!rotated.is_current(&sealed));
        assert_eq!(rotated.open(&sealed).unwrap(), "kept");
    }
}
