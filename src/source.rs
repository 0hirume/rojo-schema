use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{ensure, Context, Result};
use cargo_metadata::{Metadata, MetadataCommand, Package};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{docs, grammar, model::SourceInfo};

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
const REFLECTION_CRATE: &str = "rbx_reflection_database";
const TRACKER_REPOSITORY: &str = "https://github.com/MaximumADHD/Roblox-Client-Tracker";
const TRACKER_DUMP: &str = "Full-API-Dump.json";
const TRACKER_REVISION: &str = ".revision";
const TRACKER_VERSION: &str = "version.txt";

#[derive(Debug, Clone)]
pub struct Rojo {
    pub source: SourceInfo,
    pub grammar: grammar::Grammar,
}

pub fn load_rojo(path: &Path) -> Result<Rojo> {
    let manifest = path.join("Cargo.toml");
    let lockfile = path.join("Cargo.lock");
    let source = path.join("src");
    ensure!(
        manifest.is_file(),
        "Rojo manifest not found at {}",
        manifest.display()
    );
    ensure!(
        lockfile.is_file(),
        "Rojo lockfile not found at {}",
        lockfile.display()
    );
    ensure!(
        source.is_dir(),
        "Rojo source not found at {}",
        source.display()
    );

    let metadata = cargo_metadata(&manifest, true)?;
    let package = metadata
        .root_package()
        .context("Rojo manifest has no root package")?;
    let grammar = grammar::load(&source)?;
    let mut files = rust_files(path, &source)?;
    files.extend([PathBuf::from("Cargo.toml"), PathBuf::from("Cargo.lock")]);

    Ok(Rojo {
        source: SourceInfo {
            repository: repository(
                package
                    .repository
                    .as_deref()
                    .context("Rojo package has no repository")?,
            ),
            version: package.version.to_string(),
            sha256: hash_paths(path, files)?,
            revision: git_revision(path),
        },
        grammar,
    })
}

pub fn docs_source(path: &Path, studio_version: &str) -> Result<SourceInfo> {
    let root = docs::engine_root(path)?;
    let files = collect_files(&root)?;
    let package = fs::read_to_string(path.join("package.json"))
        .with_context(|| format!("reading {}/package.json", path.display()))?;
    let package: NpmPackage = serde_json::from_str(&package)
        .with_context(|| format!("parsing {}/package.json", path.display()))?;
    Ok(SourceInfo {
        repository: repository(&package.repository.url),
        version: studio_version.to_owned(),
        sha256: hash_paths(&root, files)?,
        revision: git_revision(path),
    })
}

pub fn tracker_source(path: &Path) -> Result<SourceInfo> {
    let root = crate::tracker::root(path)?;
    let version_path = root.join(TRACKER_VERSION);
    let version = fs::read_to_string(&version_path)
        .with_context(|| format!("reading {}", version_path.display()))?
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    ensure!(
        !version.is_empty(),
        "{} has no Studio version",
        version_path.display()
    );

    Ok(SourceInfo {
        repository: TRACKER_REPOSITORY.to_owned(),
        version,
        sha256: hash_paths(
            &root,
            [PathBuf::from(TRACKER_DUMP), PathBuf::from(TRACKER_VERSION)],
        )?,
        revision: source_revision(&root, TRACKER_REVISION),
    })
}

pub fn reflection_source(rojo: &Path) -> Result<SourceInfo> {
    let rojo_metadata = cargo_metadata(&rojo.join("Cargo.toml"), false)?;
    let rojo_package = package(&rojo_metadata, REFLECTION_CRATE)?;
    let local_metadata = cargo_metadata(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
        false,
    )?;
    let local_package = package(&local_metadata, REFLECTION_CRATE)?;
    ensure!(
        rojo_package.version == local_package.version,
        "linked {REFLECTION_CRATE} {} does not match Rojo's resolved {}",
        local_package.version,
        rojo_package.version
    );

    let lock = fs::read_to_string(rojo.join("Cargo.lock"))
        .with_context(|| format!("reading {}/Cargo.lock", rojo.display()))?;
    let lock: Lockfile = toml::from_str(&lock).context("parsing Rojo Cargo.lock")?;
    let version = rojo_package.version.to_string();
    let checksum = lock
        .package
        .iter()
        .find(|entry| entry.name == REFLECTION_CRATE && entry.version == version)
        .and_then(|entry| entry.checksum.clone())
        .context("finding the resolved reflection package checksum in Rojo Cargo.lock")?;

    Ok(SourceInfo {
        repository: repository(
            local_package
                .repository
                .as_deref()
                .context("linked reflection package has no repository")?,
        ),
        version,
        sha256: checksum,
        revision: None,
    })
}

fn cargo_metadata(manifest: &Path, no_dependencies: bool) -> Result<Metadata> {
    let mut command = MetadataCommand::new();
    command
        .manifest_path(manifest)
        .other_options(vec!["--locked".to_owned()]);
    if no_dependencies {
        command.no_deps();
    }
    command
        .exec()
        .with_context(|| format!("reading Cargo metadata for {}", manifest.display()))
}

fn package<'a>(metadata: &'a Metadata, name: &str) -> Result<&'a Package> {
    let matches = metadata
        .packages
        .iter()
        .filter(|package| package.name.as_str() == name)
        .collect::<Vec<_>>();
    ensure!(
        matches.len() == 1,
        "expected one resolved {name} package, found {}",
        matches.len()
    );
    Ok(matches[0])
}

#[derive(Deserialize)]
struct Lockfile {
    package: Vec<LockedPackage>,
}

#[derive(Deserialize)]
struct LockedPackage {
    name: String,
    version: String,
    checksum: Option<String>,
}

#[derive(Deserialize)]
struct NpmPackage {
    repository: NpmRepository,
}

#[derive(Deserialize)]
struct NpmRepository {
    url: String,
}

fn rust_files(root: &Path, source: &Path) -> Result<Vec<PathBuf>> {
    Ok(collect_files(source)?
        .into_iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .map(|path| {
            source
                .join(path)
                .strip_prefix(root)
                .expect("source is beneath root")
                .to_path_buf()
        })
        .collect())
}

fn collect_files(root: &Path) -> Result<Vec<PathBuf>> {
    fn visit(root: &Path, current: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
        let mut entries = fs::read_dir(current)
            .with_context(|| format!("reading {}", current.display()))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        entries.sort();
        for path in entries {
            if path.is_dir() {
                visit(root, &path, output)?;
            } else if path.is_file() {
                output.push(
                    path.strip_prefix(root)
                        .expect("descendant path")
                        .to_path_buf(),
                );
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    visit(root, root, &mut files)?;
    Ok(files)
}

fn hash_paths(root: &Path, paths: impl IntoIterator<Item = PathBuf>) -> Result<String> {
    let mut paths = paths.into_iter().collect::<Vec<_>>();
    paths.sort();
    let mut hasher = Sha256::new();
    for relative in paths {
        let bytes = fs::read(root.join(&relative))
            .with_context(|| format!("reading {} for hashing", root.join(&relative).display()))?;
        hasher.update(slash(&relative));
        hasher.update([0]);
        hasher.update(u64::try_from(bytes.len())?.to_le_bytes());
        hasher.update(bytes);
    }
    let digest = hasher.finalize();
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
    }
    Ok(output)
}

fn source_revision(path: &Path, revision_file: &str) -> Option<String> {
    if let Ok(revision) = fs::read_to_string(path.join(revision_file)) {
        let revision = revision.trim();
        if !revision.is_empty() {
            return Some(revision.to_owned());
        }
    }
    git_revision(path)
}

fn git_revision(path: &Path) -> Option<String> {
    let mut current = Some(path);
    while let Some(directory) = current {
        let git = directory.join(".git");
        if git.is_dir() {
            return read_head(&git);
        }
        if git.is_file() {
            let pointer = fs::read_to_string(&git).ok()?;
            let target = pointer.trim().strip_prefix("gitdir: ")?;
            let target = directory.join(target);
            return read_head(&target);
        }
        current = directory.parent();
    }
    None
}

fn read_head(git: &Path) -> Option<String> {
    let head = fs::read_to_string(git.join("HEAD")).ok()?;
    let head = head.trim();
    if let Some(reference) = head.strip_prefix("ref: ") {
        return fs::read_to_string(git.join(reference))
            .ok()
            .map(|text| text.trim().to_owned());
    }
    Some(head.to_owned())
}

fn repository(value: &str) -> String {
    value.strip_suffix(".git").unwrap_or(value).to_owned()
}

fn slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
