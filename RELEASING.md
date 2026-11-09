# Windows release procedure

The public update channel uses stable GitHub Releases in this repository. Publish a unique increasing `vMAJOR.MINOR.PATCH` tag; never replace a published version's executable or manifest. Keep the repository URL and asset names stable.

1. Update `Cargo.toml`, regenerate `Cargo.lock`, and update the changelog. Run the checks in `TESTING.md` on Windows x64.
2. Build the release executable with the MSVC toolchain. For portable distribution, use static CRT linking (`-C target-feature=+crt-static`) and remap local workspace, Cargo and user paths with Rust's `--remap-path-prefix`. Inspect the resulting EXE for private paths/metadata and confirm its platform and version. Do not include settings, diagnostics or signing secrets.
3. Build the optional signing utility: `cargo build --release --features release-tools --bin cooltakt-release --locked`. The application and updater are both Rust; this utility is not required on end-user computers.
4. Copy the application to `CoolTakt-Windows-x64.exe`. Use the existing private 32-byte Ed25519 seed stored outside the repository. Never regenerate it during a release or commit it. The utility rejects a key that does not match `release-public-key.hex`.

```powershell
.\target\release\cooltakt-release.exe sign <private-seed-file> CoolTakt-Windows-x64.exe <version> cooltakt-update-windows-x64.json
.\target\release\cooltakt-release.exe verify cooltakt-update-windows-x64.json CoolTakt-Windows-x64.exe
```

The manifest signature covers the compact UTF-8 JSON serialization of its `manifest` object, with the field order defined in `src/updates.rs`. It binds schema, product, stable version, platform, immutable release URL, byte count and SHA-256. The public key is safe to publish; keep the seed backed up privately with restricted access. Key rotation needs a separately planned compatibility release.

5. Review the exact staged files, outgoing history, identity, release text and binary assets for secrets and personal information. Commit and push the reviewed source and exact version tag without rewriting history. Verify CI for that commit.
6. Create a draft release attached to that tag. Upload `CoolTakt-Windows-x64.exe`, `cooltakt-update-windows-x64.json`, `SHA256SUMS.txt` and `THIRD_PARTY_NOTICES.txt`. Collect applicable dependency and font license texts without local paths. Verify uploaded sizes/hashes before publishing and marking it latest. The automatic channel uses GitHub's latest non-prerelease release; retain the application's preview status in the release notes until hardware validation is complete.
7. Verify the public, unauthenticated download path using `cooltakt-release.exe fetch <empty-test-directory>`. Compare the resulting hash against the published binary. This downloads and verifies only; it does not install or run the asset.

Do not sign arbitrary third-party binaries. A valid signature authorizes execution on users' computers at the running app's privilege level. The public verification key is deliberately different from the fixed, synthetic key used by unit-test executables.
