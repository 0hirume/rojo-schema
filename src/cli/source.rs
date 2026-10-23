use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use eyre::{Context as _, Result, ensure};

pub(super) fn load(
    supplied: Option<PathBuf>,
    name: &str,
    repository: &str,
    branch: Option<&str>,
    sparse: &[&str],
) -> Result<PathBuf> {
    if let Some(path) = supplied {
        return Ok(path);
    }

    let root = Path::new("target/sources");
    let path = root.join(name);
    fs::create_dir_all(root).context("creating source directory")?;
    let existing = path.exists();

    if !existing {
        let mut arguments = vec![
            "clone",
            "--depth",
            "1",
            "--filter=blob:none",
            "--no-checkout",
        ];

        if let Some(branch) = branch {
            arguments.extend(["--branch", branch]);
        }

        arguments.extend([repository, name]);
        git(root, &arguments)?;
    }

    ensure!(
        path.join(".git").is_dir(),
        "source directory is not a checkout: {}",
        path.display()
    );

    if !sparse.is_empty() {
        let mut arguments = vec!["sparse-checkout", "set", "--no-cone"];
        arguments.extend_from_slice(sparse);
        git(&path, &arguments)?;
    }

    git(&path, &["checkout"])?;

    if existing {
        git(&path, &["pull", "--ff-only"])?;
    }

    Ok(path)
}

fn git(directory: &Path, arguments: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .current_dir(directory)
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .args(arguments)
        .status()
        .with_context(|| format!("running Git in {}", directory.display()))?;

    ensure!(
        status.success(),
        "Git failed in {}: {status}",
        directory.display()
    );

    Ok(())
}
