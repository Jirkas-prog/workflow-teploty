//! Optional Rust release tooling. Signing keys are supplied from outside the repository.
use anyhow::{Result, ensure};
use ed25519_dalek::{Signer, SigningKey};
use std::{fs, path::Path};
use workflow_teploty::updates::{self, Manifest, SignedManifest};

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("sign") if args.len() == 6 => {
            let seed: [u8; 32] = fs::read(&args[2])?
                .try_into()
                .map_err(|_| anyhow::anyhow!("Expected a 32-byte Ed25519 seed"))?;
            let key = SigningKey::from_bytes(&seed);
            ensure!(
                key.verifying_key() == updates::public_key()?,
                "Signing key does not match the embedded public key"
            );
            let exe = Path::new(&args[3]);
            let version = &args[4];
            let manifest = Manifest {
                schema: 1,
                product: "CoolTakt".into(),
                version: version.clone(),
                platform: "windows-x86_64".into(),
                url: format!(
                    "{}v{version}/{}",
                    updates::RELEASE_ROOT,
                    updates::ASSET_NAME
                ),
                size: fs::metadata(exe)?.len(),
                sha256: updates::hash_file(exe)?,
            };
            let signature = hex::encode(key.sign(&serde_json::to_vec(&manifest)?).to_bytes());
            let signed = SignedManifest {
                manifest,
                signature,
            };
            signed.verify_file(exe)?;
            workflow_teploty::persist::write_json(Path::new(&args[5]), &signed)?;
            println!(
                "Signed {} · {} bytes · SHA256 {}",
                signed.manifest.version, signed.manifest.size, signed.manifest.sha256
            );
        }
        Some("verify") if args.len() == 4 => {
            let signed = updates::read_manifest(Path::new(&args[2]))?;
            signed.verify_file(Path::new(&args[3]))?;
            println!(
                "Verified {} · SHA256 {}",
                signed.manifest.version, signed.manifest.sha256
            );
        }
        Some("fetch") if args.len() == 3 => {
            let directory = Path::new(&args[2]);
            fs::create_dir_all(directory)?;
            let signed = updates::check_and_download(directory, "0.0.0", true, |_| {})?
                .ok_or_else(|| anyhow::anyhow!("No published release"))?;
            signed.verify_file(&updates::payload_path(directory, &signed)?)?;
            println!(
                "Downloaded and verified {} · SHA256 {}",
                signed.manifest.version, signed.manifest.sha256
            );
        }
        _ => anyhow::bail!(
            "Usage: cooltakt-release sign <seed> <exe> <version> <manifest> | verify <manifest> <exe> | fetch <directory>"
        ),
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
