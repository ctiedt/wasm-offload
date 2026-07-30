use std::collections::BTreeSet;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use cargo_metadata::{Message, MetadataCommand};
use offload_core::{GUEST_PATH_ENV, guest_path_env_suffix};

const DEFAULT_TARGET: &str = "wasm32-wasip1";

#[derive(Clone, Debug)]
pub struct GuestBuilder {
    package: String,
    target: String,
    profile: String,
    manifest_path: Option<PathBuf>,
    features: Vec<String>,
    no_default_features: bool,
}

impl GuestBuilder {
    pub fn new(package: impl Into<String>) -> Self {
        Self {
            package: package.into(),
            target: DEFAULT_TARGET.to_owned(),
            profile: "release".to_owned(),
            manifest_path: None,
            features: Vec::new(),
            no_default_features: false,
        }
    }

    pub fn target(mut self, target: impl Into<String>) -> Self {
        self.target = target.into();
        self
    }

    pub fn profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = profile.into();
        self
    }

    pub fn manifest_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.manifest_path = Some(path.into());
        self
    }

    pub fn feature(mut self, feature: impl Into<String>) -> Self {
        self.features.push(feature.into());
        self
    }

    pub fn no_default_features(mut self, enabled: bool) -> Self {
        self.no_default_features = enabled;
        self
    }

    pub fn build(self) -> Result<GuestArtifact> {
        if env::var_os("TARGET") == Some(OsString::from(self.target.as_str())) {
            return Ok(GuestArtifact {
                path: PathBuf::new(),
                watched_files: Vec::new(),
            });
        }

        let current_dir = PathBuf::from(
            env::var_os("CARGO_MANIFEST_DIR")
                .context("CARGO_MANIFEST_DIR is not set; GuestBuilder must run from build.rs")?,
        );
        let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));

        let mut metadata_command = MetadataCommand::new();
        metadata_command
            .cargo_path(PathBuf::from(&cargo))
            .current_dir(&current_dir)
            .no_deps();
        if let Some(path) = &self.manifest_path {
            metadata_command.manifest_path(path);
        }
        let metadata = metadata_command
            .exec()
            .context("failed to read Cargo workspace metadata")?;
        let package = metadata
            .packages
            .iter()
            .find(|package| package.name == self.package)
            .with_context(|| format!("package `{}` is not a workspace member", self.package))?;
        let package_id = package.id.clone();
        let package_manifest = package.manifest_path.as_std_path().to_path_buf();

        let target_dir = metadata
            .target_directory
            .as_std_path()
            .join("offload")
            .join(sanitize_component(&self.package));

        let mut command = Command::new(&cargo);
        command
            .current_dir(&current_dir)
            .arg("build")
            .args(["-p", &self.package])
            .arg("--lib")
            .args(["--target", &self.target])
            .args(["--profile", &self.profile])
            .arg("--message-format=json-render-diagnostics")
            .arg("--target-dir")
            .arg(&target_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(path) = &self.manifest_path {
            command.arg("--manifest-path").arg(path);
        }
        if self.no_default_features {
            command.arg("--no-default-features");
        }
        if !self.features.is_empty() {
            command.arg("--features").arg(self.features.join(","));
        }
        scrub_outer_build_environment(&mut command);

        let mut child = command.spawn().with_context(|| {
            format!("failed to spawn nested Cargo build for `{}`", self.package)
        })?;
        let stdout = child
            .stdout
            .take()
            .context("nested Cargo stdout was not piped")?;
        let mut wasm_path = None;
        let mut cargo_reported_success = false;
        let parsed = (|| -> Result<()> {
            for message in Message::parse_stream(BufReader::new(stdout)) {
                match message.context("failed to parse nested Cargo JSON output")? {
                    Message::CompilerArtifact(artifact) => {
                        if artifact.package_id == package_id
                            && artifact.target.is_cdylib()
                            && let Some(path) = artifact
                                .filenames
                                .iter()
                                .find(|path| path.extension() == Some("wasm"))
                        {
                            wasm_path = Some(path.as_std_path().to_path_buf());
                        }
                    }
                    Message::CompilerMessage(message) => {
                        if let Some(rendered) = message.message.rendered {
                            eprint!("{rendered}");
                        }
                    }
                    Message::BuildFinished(finished) => cargo_reported_success = finished.success,
                    Message::TextLine(line) => eprintln!("{line}"),
                    _ => {}
                }
            }
            Ok(())
        })();
        if let Err(error) = parsed {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let status = child
            .wait()
            .context("failed to wait for nested Cargo build")?;
        if !status.success() || !cargo_reported_success {
            bail!("guest build for `{}` failed with {status}", self.package);
        }
        let wasm_path = wasm_path.with_context(|| {
            format!(
                "Cargo built `{}` but reported no wasm cdylib artifact; add `crate-type = [\"lib\", \"cdylib\"]`",
                self.package
            )
        })?;

        let depfile = wasm_path.with_extension("d");
        let mut watched = BTreeSet::new();
        collect_depfile_leaves(&depfile, &mut BTreeSet::new(), &mut watched)?;
        watched.insert(package_manifest);
        watched.insert(
            metadata
                .workspace_root
                .join("Cargo.toml")
                .into_std_path_buf(),
        );
        let lockfile = metadata.workspace_root.join("Cargo.lock");
        if lockfile.exists() {
            watched.insert(lockfile.into_std_path_buf());
        }
        for path in &watched {
            println!("cargo::rerun-if-changed={}", path.display());
        }
        emit_guest_path(&self.package, &wasm_path);

        Ok(GuestArtifact {
            path: wasm_path,
            watched_files: watched.into_iter().collect(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct GuestArtifact {
    path: PathBuf,
    watched_files: Vec<PathBuf>,
}

impl GuestArtifact {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn watched_files(&self) -> &[PathBuf] {
        &self.watched_files
    }
}

fn emit_guest_path(package: &str, wasm_path: &Path) {
    use std::sync::Mutex;

    let suffix = guest_path_env_suffix(package);
    println!(
        "cargo::rustc-env={GUEST_PATH_ENV}_{suffix}={}",
        wasm_path.display()
    );

    static PLAIN_OWNER: Mutex<Option<String>> = Mutex::new(None);
    let mut owner = PLAIN_OWNER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match owner.as_deref() {
        None => {
            println!("cargo::rustc-env={GUEST_PATH_ENV}={}", wasm_path.display());
            *owner = Some(package.to_owned());
        }
        Some(first) if first == package => {
            println!("cargo::rustc-env={GUEST_PATH_ENV}={}", wasm_path.display());
        }
        Some(first) => {
            println!(
                "cargo::warning={GUEST_PATH_ENV} already points at guest `{first}`; \
                 read `{GUEST_PATH_ENV}_{suffix}` to embed `{package}`"
            );
        }
    }
}

fn scrub_outer_build_environment(command: &mut Command) {
    for key in [
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO",
        "CARGO_TARGET_DIR",
    ] {
        command.env_remove(key);
    }
    for (key, _) in env::vars_os() {
        let key_lossy = key.to_string_lossy();
        if key_lossy.starts_with("CARGO_FEATURE_") || key_lossy.starts_with("CARGO_CFG_") {
            command.env_remove(key);
        }
    }
}

fn sanitize_component(package: &str) -> String {
    package
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn collect_depfile_leaves(
    depfile: &Path,
    visited: &mut BTreeSet<PathBuf>,
    leaves: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    let depfile = depfile
        .canonicalize()
        .with_context(|| format!("missing rustc depfile `{}`", depfile.display()))?;
    if !visited.insert(depfile.clone()) {
        return Ok(());
    }
    let contents = fs::read_to_string(&depfile)
        .with_context(|| format!("failed to read `{}`", depfile.display()))?;
    let dependencies = parse_makefile_dependencies(&contents)
        .with_context(|| format!("failed to parse `{}`", depfile.display()))?;
    for dependency in dependencies {
        let path = if dependency.is_absolute() {
            dependency
        } else {
            depfile
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(dependency)
        };
        if path.extension().is_some_and(|extension| extension == "d") && path.exists() {
            collect_depfile_leaves(&path, visited, leaves)?;
        } else if path.exists() {
            leaves.insert(path.canonicalize().unwrap_or(path));
        }
    }
    Ok(())
}

fn parse_makefile_dependencies(contents: &str) -> Result<Vec<PathBuf>> {
    let logical = contents.replace("\\\r\n", "").replace("\\\n", "");
    let colon = find_rule_separator(&logical).context("depfile has no target separator")?;
    let mut paths = Vec::new();
    let mut current = String::new();
    let mut characters = logical[colon + 1..].chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\\' && matches!(characters.peek(), Some(' ') | Some('#')) {
            current.push(characters.next().expect("peeked"));
        } else if character.is_whitespace() {
            if !current.is_empty() {
                paths.push(PathBuf::from(std::mem::take(&mut current)));
            }
        } else {
            current.push(character);
        }
    }
    if !current.is_empty() {
        paths.push(PathBuf::from(current));
    }
    Ok(paths)
}

fn find_rule_separator(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut escaped = false;
    for (index, &byte) in bytes.iter().enumerate() {
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b':'
            && matches!(
                bytes.get(index + 1),
                None | Some(b' ' | b'\t' | b'\r' | b'\n')
            )
        {
            return Some(index);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_escaped_makefile_paths() {
        let paths = parse_makefile_dependencies(concat!(
            "out.wasm: /tmp/a.rs relative\\ path/lib.rs \\",
            "\n             /tmp/last.rs\n",
        ))
        .unwrap();
        assert_eq!(
            paths,
            [
                PathBuf::from("/tmp/a.rs"),
                PathBuf::from("relative path/lib.rs"),
                PathBuf::from("/tmp/last.rs"),
            ]
        );
    }

    #[test]
    fn parses_windows_paths_with_drive_letters_and_backslashes() {
        let paths = parse_makefile_dependencies(
            "C:\\work\\target\\guest.wasm: C:\\work\\src\\lib.rs C:\\work\\src\\util.rs\n",
        )
        .unwrap();
        assert_eq!(
            paths,
            [
                PathBuf::from("C:\\work\\src\\lib.rs"),
                PathBuf::from("C:\\work\\src\\util.rs"),
            ]
        );
    }

    #[test]
    fn sanitizes_target_directory_component() {
        assert_eq!(sanitize_component("my package/guest"), "my_package_guest");
    }
}
