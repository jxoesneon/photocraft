//! `cargo xtask sync` — continuous upstream integration with GUI protection.
//!
//! Upstream (storytold/photocraft) develops the egui shell in `crates/ui-egui`
//! and the app entry in `apps/photocraft`; this fork replaced both with the
//! Martensite shell (`crates/ui-martensite`). A plain `git merge upstream/main`
//! would resurrect egui code (as clean additions of files we never had, or as
//! delete/modify conflicts that resolve wrong by hand) and could clobber the
//! Martensite GUI. This command does the merge mechanically instead:
//!
//! 1. `git fetch <remote>` and report incoming commits, split into "touches
//!    protected GUI paths" vs the rest.
//! 2. `--apply`: `git merge --no-commit`, then force every protected path back
//!    to our side (`git rm -rf` + `git checkout HEAD --`), so upstream GUI
//!    edits can never land. Remaining conflicts are genuine non-GUI work for
//!    the operator.
//! 3. Write `log/upstream-gui-report.md`: the commits and file delta under the
//!    protected paths — the re-adaptation queue. Anything upstream changed in
//!    the egui surface must be re-expressed in the Martensite shell by hand.

use std::path::Path;
use std::process::Command;

use crate::root;

/// Paths whose upstream content must never merge. `ui-egui` is a tombstone;
/// `ui-martensite` is ours; the app entries are the Martensite runners.
const PROTECTED: &[&str] =
    &["crates/ui-egui", "crates/ui-martensite", "apps/photocraft", "apps/photocraft-web"];

fn git(args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("git")
        .args(args)
        .current_dir(root())
        .output()
        .map_err(|e| format!("git {}: {e}", args.first().copied().unwrap_or("")))
}

fn git_ok(args: &[&str]) -> Result<String, String> {
    let out = git(args)?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn git_lines(args: &[&str]) -> Result<Vec<String>, String> {
    Ok(git_ok(args)?.lines().map(str::to_string).collect())
}

/// One incoming commit, classified by whether it touched protected GUI paths.
struct Incoming {
    sha: String,
    subject: String,
    gui: bool,
}

fn incoming_commits(base: &str, upstream_ref: &str) -> Result<Vec<Incoming>, String> {
    let range = format!("{base}..{upstream_ref}");
    let raw = git_lines(&["log", "--format=%h%x00%s", &range])?;
    let mut out = Vec::new();
    for line in raw {
        let Some((sha, subject)) = line.split_once('\0') else { continue };
        let mut paths = git_lines(&["diff-tree", "--no-commit-id", "--name-only", "-r", sha])?;
        // Merge commits report no names via --no-commit-id; diff the merge
        // result against its first parent instead.
        if paths.is_empty() {
            let parent = format!("{sha}^");
            paths = git_lines(&["diff", "--name-only", &parent, sha]).unwrap_or_default();
        }
        let gui = paths.iter().any(|p| PROTECTED.iter().any(|g| p.starts_with(g)));
        out.push(Incoming { sha: sha.to_string(), subject: subject.to_string(), gui });
    }
    Ok(out)
}

/// Restores a protected path wholesale to HEAD: clears whatever the merge
/// staged (modifications, additions, delete/modify conflicts), then replays
/// our tree. Paths absent from HEAD stay gone.
fn force_ours(path: &str) -> Result<(), String> {
    git_ok(&["rm", "-rf", "--quiet", "--ignore-unmatch", "--", path])?;
    git_ok(&["checkout", "HEAD", "--", path])?;
    Ok(())
}

pub fn run(args: &[&str]) -> Result<(), String> {
    let apply = args.iter().any(|a| *a == "--apply");
    let remote = args
        .iter()
        .position(|a| *a == "--remote")
        .and_then(|i| args.get(i + 1))
        .copied()
        .unwrap_or("upstream");
    let upstream_ref = format!("{remote}/main");

    git_ok(&["fetch", remote])?;
    let base = git_ok(&["merge-base", "HEAD", &upstream_ref])?;
    let behind = git_lines(&["rev-list", "--count", &format!("HEAD..{upstream_ref}")])?
        .first()
        .cloned()
        .unwrap_or_else(|| "0".into());
    let commits = incoming_commits(&base, &upstream_ref)?;
    let gui: Vec<&Incoming> = commits.iter().filter(|c| c.gui).collect();

    println!("upstream {upstream_ref}: {behind} commit(s) incoming, {} touch protected GUI paths", gui.len());
    for c in &gui {
        println!("  gui  {} {}", c.sha, c.subject);
    }
    if commits.len() == gui.len() && commits.is_empty() {
        println!("up to date");
        return Ok(());
    }

    // The re-adaptation queue is useful even on a dry run.
    write_report(remote, &upstream_ref, &base, &gui)?;
    if !apply {
        println!("\ndry run — `cargo xtask sync --apply` merges with GUI paths pinned to ours");
        println!("re-adaptation queue: log/upstream-gui-report.md");
        return Ok(());
    }

    if !git_ok(&["status", "--porcelain"])?.is_empty() {
        return Err("working tree is not clean — commit or stash before `--apply`".into());
    }
    if Path::new(&root()).join(".git/MERGE_HEAD").exists() {
        return Err("a merge is already in progress — resolve it or `git merge --abort` first".into());
    }

    let merge = git(&["merge", "--no-commit", "--no-ff", &upstream_ref])?;
    eprint!("{}", String::from_utf8_lossy(&merge.stderr));

    // GUI paths are ours regardless of how the merge resolved them.
    for path in PROTECTED {
        force_ours(path)?;
    }

    let remaining = git_lines(&["diff", "--name-only", "--diff-filter=U"])?;
    if remaining.is_empty() {
        git_ok(&["commit", "-m", &format!("Merge {upstream_ref} (GUI paths pinned to martensite shell)")])?;
        println!("merged clean: protected GUI paths kept ours, no other conflicts");
    } else {
        println!("\nmerge staged with protected GUI paths reset to ours.");
        println!("{} non-GUI conflict(s) remain for the operator:", remaining.len());
        for f in &remaining {
            println!("  conflict  {f}");
        }
        println!("resolve them, then `git commit`. Abort everything with `git merge --abort`.");
    }
    println!("re-adaptation queue: log/upstream-gui-report.md ({} GUI commits to port)", gui.len());
    Ok(())
}

fn write_report(remote: &str, upstream_ref: &str, base: &str, gui: &[&Incoming]) -> Result<(), String> {
    if gui.is_empty() {
        return Ok(());
    }
    let mut report = format!("# Upstream GUI delta — {remote} ({base}..{upstream_ref})\n\n");
    report.push_str("These commits touched the protected GUI surface and must be\n");
    report.push_str("re-adapted into `crates/ui-martensite` (never merged directly):\n\n");
    for c in gui {
        report.push_str(&format!("- `{}` {}\n", c.sha, c.subject));
    }
    report.push_str("\n## File delta\n\n```\n");
    let stat = git_ok(&["diff", "--stat", &format!("{base}..{upstream_ref}"), "--", PROTECTED[0], PROTECTED[1], PROTECTED[2], PROTECTED[3]])?;
    report.push_str(&stat);
    report.push_str("\n```\n");
    let dir = root().join("log");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create log/: {e}"))?;
    std::fs::write(dir.join("upstream-gui-report.md"), report).map_err(|e| format!("write report: {e}"))?;
    Ok(())
}
