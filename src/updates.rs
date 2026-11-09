//! Signed, bounded GitHub Release downloads. Installation is handled by a copy
//! of the running executable after the original process exits.
use anyhow::{Context, Result, ensure};
use ed25519_dalek::{Signature, VerifyingKey};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

pub mod installer;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const RELEASE_ROOT: &str = "https://github.com/Jirkas-prog/workflow-teploty/releases/download/";
pub const MANIFEST_URL: &str = "https://github.com/Jirkas-prog/workflow-teploty/releases/latest/download/cooltakt-update-windows-x64.json";
pub const ASSET_NAME: &str = "CoolTakt-Windows-x64.exe";
pub const MAX_ASSET_SIZE: u64 = 64 * 1024 * 1024;
pub const MAX_MANIFEST_SIZE: u64 = 16 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub product: String,
    pub version: String,
    pub platform: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedManifest {
    pub manifest: Manifest,
    pub signature: String,
}

pub fn public_key() -> Result<VerifyingKey> {
    #[cfg(not(test))]
    let bytes: [u8; 32] = hex::decode(include_str!("../release-public-key.hex").trim())?
        .try_into()
        .map_err(|_| {
            crate::terr!(
                "Neplatný veřejný klíč aktualizací.",
                "Invalid embedded update public key."
            )
        })?;
    #[cfg(test)]
    let bytes = ed25519_dalek::SigningKey::from_bytes(&[7; 32])
        .verifying_key()
        .to_bytes();
    Ok(VerifyingKey::from_bytes(&bytes)?)
}

impl SignedManifest {
    pub fn verify(&self) -> Result<Version> {
        let message = serde_json::to_vec(&self.manifest)?;
        let signature = hex::decode(&self.signature)
            .ok()
            .and_then(|b| Signature::from_slice(&b).ok())
            .ok_or_else(|| {
                crate::terr!("Neplatný podpis aktualizace.", "Invalid update signature.")
            })?;
        public_key()?
            .verify_strict(&message, &signature)
            .map_err(|_| {
                crate::terr!(
                    "Podpis aktualizace nesouhlasí.",
                    "The update signature does not match."
                )
            })?;
        let m = &self.manifest;
        ensure!(
            m.schema == 1 && m.product == "CoolTakt" && m.platform == "windows-x86_64",
            crate::tr!(
                "Aktualizace není určená pro tuto aplikaci a platformu.",
                "This update is not for this application and platform."
            )
        );
        let version = Version::parse(&m.version)?;
        ensure!(
            version.pre.is_empty() && version.build.is_empty() && version.to_string() == m.version,
            crate::tr!(
                "Nepodporovaný formát verze aktualizace.",
                "Unsupported update version format."
            )
        );
        ensure!(
            m.url == format!("{RELEASE_ROOT}v{version}/{ASSET_NAME}"),
            crate::tr!(
                "Aktualizace neodkazuje na oficiální vydání.",
                "The update does not point to the official release."
            )
        );
        ensure!(
            (1024..=MAX_ASSET_SIZE).contains(&m.size)
                && m.sha256.len() == 64
                && m.sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            crate::tr!(
                "Neplatná velikost nebo kontrolní součet aktualizace.",
                "Invalid update size or checksum."
            )
        );
        Ok(version)
    }

    pub fn newer_than(&self, current: &str) -> Result<bool> {
        Ok(self.verify()? > Version::parse(current)?)
    }

    pub fn verify_file(&self, path: &Path) -> Result<()> {
        self.verify()?;
        let metadata = fs::metadata(path)?;
        ensure!(
            metadata.is_file() && metadata.len() == self.manifest.size,
            crate::tr!(
                "Stažená aktualizace má nesprávnou velikost.",
                "The downloaded update has the wrong size."
            )
        );
        ensure!(
            hash_file(path)? == self.manifest.sha256,
            crate::tr!(
                "Stažená aktualizace má nesprávný kontrolní součet.",
                "The downloaded update has the wrong checksum."
            )
        );
        let mut header = [0u8; 2];
        File::open(path)?.read_exact(&mut header)?;
        ensure!(
            header == *b"MZ",
            crate::tr!(
                "Aktualizace není Windows EXE.",
                "The update is not a Windows executable."
            )
        );
        Ok(())
    }
}

pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn nonce() -> Result<String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(hex::encode(bytes))
}

pub fn read_manifest(path: &Path) -> Result<SignedManifest> {
    let bytes = read_bounded(File::open(path)?, MAX_MANIFEST_SIZE)?;
    let manifest: SignedManifest = serde_json::from_slice(&bytes)?;
    manifest.verify()?;
    Ok(manifest)
}

fn read_bounded(reader: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        crate::tr!(
            "Odpověď serveru je příliš velká.",
            "The server response is too large."
        )
    );
    Ok(bytes)
}

pub fn update_dir(directory: &Path) -> PathBuf {
    directory.join("updates")
}

pub fn pending(directory: &Path) -> Result<Option<SignedManifest>> {
    let path = update_dir(directory).join("pending.json");
    if !path.exists() {
        return Ok(None);
    }
    let signed = read_manifest(&path)?;
    if !signed.newer_than(CURRENT_VERSION)? {
        return Ok(None);
    }
    signed.verify_file(&payload_path(directory, &signed)?)?;
    Ok(Some(signed))
}

pub fn payload_path(directory: &Path, signed: &SignedManifest) -> Result<PathBuf> {
    let version = signed.verify()?;
    Ok(update_dir(directory)
        .join("staged")
        .join(version.to_string())
        .join(ASSET_NAME))
}

#[derive(Clone, Debug)]
pub enum Progress {
    Checking,
    Downloading {
        version: String,
        downloaded: u64,
        total: u64,
    },
}

pub fn check_and_download(
    directory: &Path,
    current: &str,
    retry_failed: bool,
    mut progress: impl FnMut(Progress),
) -> Result<Option<SignedManifest>> {
    progress(Progress::Checking);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(180)))
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_per_read(Some(Duration::from_secs(20)))
        .user_agent(concat!("CoolTakt/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut response = match agent.get(MANIFEST_URL).call() {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let bytes = read_bounded(response.body_mut().as_reader(), MAX_MANIFEST_SIZE)?;
    let signed: SignedManifest = serde_json::from_slice(&bytes)?;
    if !signed.newer_than(current)? {
        return Ok(None);
    }
    if !retry_failed
        && last_result(directory)
            .is_some_and(|r| !r.success && r.version == signed.manifest.version)
    {
        return Ok(None);
    }
    let path = payload_path(directory, &signed)?;
    fs::create_dir_all(path.parent().unwrap())?;
    if signed.verify_file(&path).is_err() {
        let temporary = path.with_extension(format!("{}.part", nonce()?));
        let download = (|| -> Result<()> {
            let mut response = agent.get(&signed.manifest.url).call()?;
            if let Some(length) = response.headers().get("content-length") {
                ensure!(
                    length.to_str()?.parse::<u64>()? == signed.manifest.size,
                    crate::tr!(
                        "Velikost aktualizace na serveru nesouhlasí.",
                        "The server's update size does not match."
                    )
                );
            }
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)?;
            let mut reader = response.body_mut().as_reader();
            let mut downloaded = 0;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let n = reader.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                downloaded += n as u64;
                ensure!(
                    downloaded <= signed.manifest.size,
                    crate::tr!(
                        "Server poslal větší aktualizaci, než bylo oznámeno.",
                        "The server sent more update data than declared."
                    )
                );
                file.write_all(&buffer[..n])?;
                progress(Progress::Downloading {
                    version: signed.manifest.version.clone(),
                    downloaded,
                    total: signed.manifest.size,
                });
            }
            file.sync_all()?;
            drop(file);
            signed.verify_file(&temporary)?;
            fs::rename(&temporary, &path)?;
            Ok(())
        })();
        if download.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        download?;
    }
    crate::persist::write_json(&update_dir(directory).join("pending.json"), &signed)?;
    Ok(Some(signed))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InstallResult {
    pub version: String,
    pub success: bool,
    pub detail: String,
}
pub fn last_result(directory: &Path) -> Option<InstallResult> {
    let bytes = read_bounded(
        File::open(update_dir(directory).join("last-result.json")).ok()?,
        MAX_MANIFEST_SIZE,
    )
    .ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Rejects symlink/reparse-point redirection inside the update staging tree.
pub fn contained_path(root: &Path, path: &Path) -> Result<PathBuf> {
    let root = fs::canonicalize(root)?;
    let canonical = fs::canonicalize(path)?;
    ensure!(
        canonical.starts_with(&root) && canonical != root,
        crate::tr!(
            "Cesta aktualizace je mimo její pracovní složku.",
            "The update path is outside its working directory."
        )
    );
    Ok(canonical)
}

#[cfg(test)]
mod tests;
