//! Host-side source checkouts for layers that build from local trees.
//!
//! Layers with a `source_repo` are cloned into `<config>/sources/<layer-name>/`
//! by [`ensure_source`]. Before each `docker build`, [`stage_sources`] walks
//! that directory and hardlinks every populated source tree into the build
//! context, alongside a default `.dockerignore`, so the layer's Dockerfile can
//! `COPY` from it without any per-user setup.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::config;
use crate::layer::Layer;

/// Directories that should never be copied into a Docker build context, either
/// because they're enormous (build artefacts) or irrelevant (VCS metadata, OS
/// junk). Skipping at walk time saves the work of linking them; the matching
/// `.dockerignore` also tells `docker build` to ignore them if they do slip in.
const SKIP_DIRS: &[&str] = &["target", ".git", "node_modules", ".DS_Store"];

/// Default `.dockerignore` written into every staged build context.
const DOCKERIGNORE: &str = "**/target\n**/.git\n**/node_modules\n**/.DS_Store\n";

/// Ensure the host-side checkout for a layer is present and up to date.
///
/// No-op for layers without a `source_repo`. On first run the repo is cloned
/// into `<config>/sources/<layer-name>/`. On subsequent runs the remote is
/// fetched and the working tree is reset to the desired ref (or the remote's
/// default branch). Claudine owns this directory, so a hard reset is
/// intentional — any local edits will be discarded.
pub fn ensure_source(layer: &Layer) -> anyhow::Result<()> {
    let Some(repo) = layer.source_repo else {
        return Ok(());
    };

    let sources = config::sources_dir()?;
    fs::create_dir_all(&sources).map_err(|e| {
        anyhow::anyhow!("Failed to create sources directory {}: {e}", sources.display())
    })?;

    let target = sources.join(layer.name);

    if !target.exists() {
        println!("Cloning {} into {}...", repo, target.display());
        let status = Command::new("git")
            .args(["clone", repo])
            .arg(&target)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| anyhow::anyhow!("Failed to run 'git clone': {e}"))?;

        if !status.success() {
            anyhow::bail!("git clone of {} failed", repo);
        }

        if let Some(r#ref) = layer.source_ref {
            run_git(&target, &["checkout", r#ref])?;
        }

        return Ok(());
    }

    println!("Refreshing {} checkout at {}...", layer.name, target.display());
    run_git(&target, &["fetch", "--quiet", "--prune", "origin"])?;

    let target_ref = match layer.source_ref {
        Some(r) => format!("origin/{}", r),
        None => resolve_default_branch(&target)?,
    };
    run_git(&target, &["reset", "--hard", &target_ref])?;

    Ok(())
}

/// Stage every populated directory under `<config>/sources/` into `build_ctx`
/// as hardlinked copies, and write a default `.dockerignore`.
///
/// The staging is convention-based: whatever's in `sources/` gets linked into
/// the build context under the same name. A layer's Dockerfile references the
/// name via `COPY <name> ...`. Missing sources are silently skipped — `docker
/// build` will surface any real mismatch as a COPY failure.
pub fn stage_sources(build_ctx: &Path) -> anyhow::Result<()> {
    fs::write(build_ctx.join(".dockerignore"), DOCKERIGNORE).map_err(|e| {
        anyhow::anyhow!("Failed to write .dockerignore in build context: {e}")
    })?;

    let sources = match config::sources_dir() {
        Ok(p) => p,
        Err(_) => return Ok(()),
    };

    if !sources.exists() {
        return Ok(());
    }

    let entries = fs::read_dir(&sources)
        .map_err(|e| anyhow::anyhow!("Failed to read sources directory {}: {e}", sources.display()))?;

    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let src = entry.path();
        let dst = build_ctx.join(&name);
        link_tree(&src, &dst)?;
    }

    Ok(())
}

/// Recursively hardlink a directory tree, creating directories as needed and
/// skipping [`SKIP_DIRS`] at every level.
fn link_tree(src: &Path, dst: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dst)
        .map_err(|e| anyhow::anyhow!("Failed to create {}: {e}", dst.display()))?;

    let entries = fs::read_dir(src)
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {e}", src.display()))?;

    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name();

        if SKIP_DIRS.iter().any(|s| *s == name) {
            continue;
        }

        let child_src = entry.path();
        let child_dst = dst.join(&name);

        if file_type.is_dir() {
            link_tree(&child_src, &child_dst)?;
        } else if file_type.is_symlink() {
            let resolved = fs::canonicalize(&child_src).map_err(|e| {
                anyhow::anyhow!("Failed to resolve symlink {}: {e}", child_src.display())
            })?;
            link_file(&resolved, &child_dst)?;
        } else {
            link_file(&child_src, &child_dst)?;
        }
    }

    Ok(())
}

/// Try to hardlink a single file, falling back to a copy when the hardlink
/// fails (different filesystems, permissions, etc.).
fn link_file(src: &Path, dst: &Path) -> anyhow::Result<()> {
    match fs::hard_link(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            fs::copy(src, dst).map_err(|e| {
                anyhow::anyhow!("Failed to stage {} -> {}: {e}", src.display(), dst.display())
            })?;
            Ok(())
        }
    }
}

/// Run a git subcommand inside a working tree, inheriting stdio.
fn run_git(cwd: &Path, args: &[&str]) -> anyhow::Result<()> {
    let status = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run 'git {}': {e}", args.join(" ")))?;

    if !status.success() {
        anyhow::bail!("git {} failed in {}", args.join(" "), cwd.display());
    }

    Ok(())
}

/// Resolve the remote's default branch (e.g. `origin/main`) from a checkout.
fn resolve_default_branch(cwd: &Path) -> anyhow::Result<String> {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to query origin HEAD: {e}"))?;

    if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !value.is_empty() {
            return Ok(value);
        }
    }

    // Fallback: many repos have either main or master.
    Ok("origin/HEAD".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn stage_sources_writes_dockerignore_when_empty() {
        let tmp = TempDir::new().unwrap();
        stage_sources(tmp.path()).unwrap();
        let contents = fs::read_to_string(tmp.path().join(".dockerignore")).unwrap();
        assert!(contents.contains("**/target"));
        assert!(contents.contains("**/.git"));
    }

    #[test]
    fn link_tree_skips_ignored_dirs() {
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();

        fs::create_dir_all(src.path().join("target/debug")).unwrap();
        fs::write(src.path().join("target/debug/big.bin"), b"huge").unwrap();
        fs::create_dir_all(src.path().join(".git")).unwrap();
        fs::write(src.path().join(".git/HEAD"), b"ref").unwrap();
        fs::write(src.path().join("Cargo.toml"), b"[package]").unwrap();
        fs::create_dir_all(src.path().join("src")).unwrap();
        fs::write(src.path().join("src/main.rs"), b"fn main() {}").unwrap();

        link_tree(src.path(), dst.path()).unwrap();

        assert!(dst.path().join("Cargo.toml").exists());
        assert!(dst.path().join("src/main.rs").exists());
        assert!(!dst.path().join("target").exists());
        assert!(!dst.path().join(".git").exists());
    }
}
