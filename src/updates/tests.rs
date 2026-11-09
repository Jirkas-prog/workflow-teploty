use super::*;
use ed25519_dalek::{Signer, SigningKey};

fn signed(version: &str, bytes: &[u8]) -> SignedManifest {
    let manifest = Manifest {
        schema: 1,
        product: "CoolTakt".into(),
        version: version.into(),
        platform: "windows-x86_64".into(),
        url: format!("{RELEASE_ROOT}v{version}/{ASSET_NAME}"),
        size: bytes.len() as u64,
        sha256: hex::encode(Sha256::digest(bytes)),
    };
    let signature = hex::encode(
        SigningKey::from_bytes(&[7; 32])
            .sign(&serde_json::to_vec(&manifest).unwrap())
            .to_bytes(),
    );
    SignedManifest {
        manifest,
        signature,
    }
}

#[test]
fn rejects_tampering_wrong_source_and_downgrades() {
    let bytes = vec![0; 2048];
    let mut valid = signed("0.2.1", &bytes);
    assert!(valid.newer_than("0.2.0").unwrap());
    assert!(!valid.newer_than("0.2.1").unwrap());
    assert!(!valid.newer_than("0.3.0").unwrap());
    valid.manifest.size += 1;
    assert!(valid.verify().is_err());
    let mut wrong_source = signed("0.2.1", &bytes);
    wrong_source.manifest.url = "https://example.com/update.exe".into();
    wrong_source.signature = hex::encode(
        SigningKey::from_bytes(&[7; 32])
            .sign(&serde_json::to_vec(&wrong_source.manifest).unwrap())
            .to_bytes(),
    );
    assert!(wrong_source.verify().is_err());
    assert!(signed("0.2.1-beta.1", &bytes).verify().is_err());
    assert!(signed("0.2.1+build", &bytes).verify().is_err());
}

#[test]
fn bounds_server_responses_and_detects_corrupt_payloads() {
    assert!(read_bounded(&[0u8; 9][..], 8).is_err());
    assert_eq!(read_bounded(&[0u8; 8][..], 8).unwrap().len(), 8);
    let directory = std::env::temp_dir().join(format!("cooltakt-hash-{}", nonce().unwrap()));
    fs::create_dir(&directory).unwrap();
    let file = directory.join("test.exe");
    let mut bytes = vec![0u8; 2048];
    bytes[..2].copy_from_slice(b"MZ");
    let manifest = signed("0.2.1", &bytes);
    fs::write(&file, &bytes).unwrap();
    manifest.verify_file(&file).unwrap();
    bytes[100] = 1;
    fs::write(&file, &bytes).unwrap();
    assert!(manifest.verify_file(&file).is_err());
    fs::write(&file, &bytes[..1000]).unwrap();
    assert!(manifest.verify_file(&file).is_err());
    fs::remove_file(file).unwrap();
    fs::remove_dir(directory).unwrap();
}

// A copied test executable exercises the real Windows replacement and rollback
// protocol. These child processes never create a GUI or change power settings.
#[test]
fn process_entry() {
    let Ok(role) = std::env::var("COOLTAKT_TEST_ROLE") else {
        return;
    };
    let directory = PathBuf::from(std::env::var_os("COOLTAKT_TEST_DIRECTORY").unwrap());
    match role.as_str() {
        "parent" => {
            let signed = pending(&directory).unwrap().unwrap();
            installer::prepare(&directory, signed, true, false).unwrap();
        }
        "helper" => {
            let path = PathBuf::from(std::env::var_os("COOLTAKT_TEST_PLAN").unwrap());
            let _ = installer::apply(&path);
        }
        "child" => {
            if std::env::var_os("COOLTAKT_TEST_FAIL_CHILD").is_some() {
                return;
            }
            let path = PathBuf::from(std::env::var_os("COOLTAKT_TEST_PLAN").unwrap());
            let plan: installer::Plan = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            assert!(plan.resume_control);
            installer::acknowledge(&path, &directory).unwrap();
            fs::write(directory.join("child-started"), b"ok").unwrap();
            std::thread::sleep(Duration::from_secs(2));
        }
        "rollback" => {
            fs::write(directory.join("rollback-started"), b"ok").unwrap();
        }
        _ => panic!("Unexpected subprocess role"),
    }
}

fn installation_roundtrip(fail_child: bool) {
    use std::os::windows::process::CommandExt;
    use std::{process::Command, time::Instant};
    let directory = std::env::temp_dir().join(format!("cooltakt-install-{}", nonce().unwrap()));
    let data = directory.join("data");
    fs::create_dir_all(&data).unwrap();
    let target = directory.join("CoolTakt.exe");
    let original = fs::read(std::env::current_exe().unwrap()).unwrap();
    fs::write(&target, &original).unwrap();
    let mut replacement = original.clone();
    replacement.extend_from_slice(b"CoolTakt update integration fixture");
    let mut next_version = Version::parse(CURRENT_VERSION).unwrap();
    next_version.patch += 1;
    let signed = signed(&next_version.to_string(), &replacement);
    let payload = payload_path(&data, &signed).unwrap();
    fs::create_dir_all(payload.parent().unwrap()).unwrap();
    fs::write(&payload, &replacement).unwrap();
    crate::persist::write_json(&update_dir(&data).join("pending.json"), &signed).unwrap();
    for name in ["nastaveni.json", "aplikace.json", "profily.json"] {
        fs::write(data.join(name), b"preserve user settings").unwrap();
    }
    let mut command = Command::new(&target);
    command
        .creation_flags(0x08000000)
        .args(["--exact", "updates::tests::process_entry", "--nocapture"])
        .env("COOLTAKT_TEST_ROLE", "parent")
        .env("COOLTAKT_TEST_DIRECTORY", &data);
    if fail_child {
        command.env("COOLTAKT_TEST_FAIL_CHILD", "1");
    }
    let mut parent = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(40);
    let result = loop {
        if let Some(result) = last_result(&data) {
            break result;
        }
        assert!(
            Instant::now() < deadline,
            "Update subprocess timed out in {}",
            directory.display()
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(parent.wait().unwrap().success());
    assert_eq!(result.success, !fail_child, "{}", result.detail);
    let expected = if fail_child { &original } else { &replacement };
    assert_eq!(
        hash_file(&target).unwrap(),
        hex::encode(Sha256::digest(expected))
    );
    assert!(!update_dir(&data).join("pending.json").exists());
    for name in ["nastaveni.json", "aplikace.json", "profily.json"] {
        assert_eq!(
            fs::read(data.join(name)).unwrap(),
            b"preserve user settings"
        );
    }
    let marker = if fail_child {
        "rollback-started"
    } else {
        "child-started"
    };
    while !data.join(marker).is_file() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(100));
    }
    // Wait for our exact child executable to be released before deleting only
    // this randomly named test directory.
    while let Err(error) = fs::remove_dir_all(&directory) {
        assert!(Instant::now() < deadline, "Test cleanup: {error}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn installs_verified_update_and_preserves_user_settings() {
    installation_roundtrip(false);
}

#[test]
fn rolls_back_and_restarts_after_failed_startup() {
    installation_roundtrip(true);
}

#[test]
fn refuses_installation_with_unrestored_power_settings() {
    let directory = std::env::temp_dir().join(format!("cooltakt-recovery-{}", nonce().unwrap()));
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("power-recovery.json"), b"journal").unwrap();
    let before = hash_file(&std::env::current_exe().unwrap()).unwrap();
    let mut next_version = Version::parse(CURRENT_VERSION).unwrap();
    next_version.patch += 1;
    let error = installer::prepare(
        &directory,
        signed(&next_version.to_string(), &[0; 2048]),
        false,
        false,
    )
    .unwrap_err();
    assert!(error.to_string().contains("napájecí") || error.to_string().contains("power"));
    assert_eq!(
        before,
        hash_file(&std::env::current_exe().unwrap()).unwrap()
    );
    assert_eq!(
        fs::read(directory.join("power-recovery.json")).unwrap(),
        b"journal"
    );
    fs::remove_dir_all(directory).unwrap();
}
