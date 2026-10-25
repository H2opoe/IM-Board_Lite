//! Read the official CLI's managed credential formats without migrating/writing them.
use super::{Failure, Result};
use base64::Engine;
use ring::aead;
use serde::Deserialize;
use std::{fs, io::Read, path::Path};
use zeroize::Zeroizing;

pub(super) struct Bot {
    pub id: String,
    pub secret: Zeroizing<String>,
}
#[derive(Deserialize)]
struct StoredBot {
    id: String,
    secret: String,
}
#[derive(Deserialize)]
struct StoredCredentials {
    bot: Option<StoredBot>,
}
fn bounded_file(path: &Path, max: u64) -> Result<Zeroizing<Vec<u8>>> {
    let info = fs::symlink_metadata(path).map_err(|_| Failure::auth())?;
    if !info.is_file() || info.file_type().is_symlink() || info.len() > max {
        return Err(Failure::auth());
    }
    let mut bytes = Zeroizing::new(Vec::new());
    fs::File::open(path)
        .map_err(|_| Failure::auth())?
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Failure::auth())?;
    if bytes.len() as u64 > max {
        return Err(Failure::auth());
    }
    Ok(bytes)
}
pub(super) fn load(config: &Path, expected_bot_id: Option<&str>) -> Result<Bot> {
    let encoded = bounded_file(&config.join(".encryption_key"), 4096)?;
    let text = std::str::from_utf8(&encoded).map_err(|_| Failure::auth())?;
    let key = Zeroizing::new(
        base64::engine::general_purpose::STANDARD
            .decode(text.trim())
            .map_err(|_| Failure::auth())?,
    );
    let key = aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_256_GCM, &key).map_err(|_| Failure::auth())?,
    );
    // A corrupt current file must not fall back to a potentially stale legacy account.
    let current = config
        .join("credentials.enc")
        .try_exists()
        .map_err(|_| Failure::auth())?;
    let mut bytes = bounded_file(
        &config.join(if current {
            "credentials.enc"
        } else {
            "bot.enc"
        }),
        1024 * 1024,
    )?;
    if bytes.len() < 28 {
        return Err(Failure::auth());
    }
    let nonce =
        aead::Nonce::try_assume_unique_for_key(&bytes[..12]).map_err(|_| Failure::auth())?;
    let plain = key
        .open_in_place(nonce, aead::Aad::empty(), &mut bytes[12..])
        .map_err(|_| Failure::auth())?;
    let stored = if current {
        serde_json::from_slice::<StoredCredentials>(plain)
            .map_err(|_| Failure::auth())?
            .bot
            .ok_or_else(Failure::auth)?
    } else {
        serde_json::from_slice::<StoredBot>(plain).map_err(|_| Failure::auth())?
    };
    let bot = Bot {
        id: stored.id,
        secret: Zeroizing::new(stored.secret),
    };
    if bot.id.trim().is_empty()
        || bot.secret.trim().is_empty()
        || expected_bot_id.is_some_and(|id| !id.is_empty() && id != bot.id)
    {
        return Err(Failure::auth());
    }
    Ok(bot)
}
