use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

const DEBT_BRANCH: &str = "cognitive-debt/v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    NewFeature,
    Refactor,
    BugFix,
    SubsystemChange,
    Minor,
    Risk,
    TechDebt,
    DependencyUpdate,
}

impl std::fmt::Display for Classification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Classification::NewFeature => "new_feature",
            Classification::Refactor => "refactor",
            Classification::BugFix => "bug_fix",
            Classification::SubsystemChange => "subsystem_change",
            Classification::Minor => "minor",
            Classification::Risk => "risk",
            Classification::TechDebt => "tech_debt",
            Classification::DependencyUpdate => "dependency_update",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EndorsementStatus {
    Unendorsed,
    Reviewed,
    Endorsed,
    Excluded,
}

impl std::fmt::Display for EndorsementStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            EndorsementStatus::Unendorsed => "unendorsed",
            EndorsementStatus::Reviewed => "reviewed",
            EndorsementStatus::Endorsed => "endorsed",
            EndorsementStatus::Excluded => "excluded",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityItem {
    pub id: String,
    pub branch: String,
    pub classification: Classification,
    pub subsystem: String,
    pub title: String,
    pub summary: String,
    pub commits: Vec<String>,
    pub since_sha: String,
    pub until_sha: String,
    pub cognitive_friction_score: f32,
    pub ai_attributed: bool,
    pub attribution_pct: Option<f32>,
    pub zombie: bool,
    pub endorsement_status: EndorsementStatus,
    pub audited_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndorsementRecord {
    pub sha: String,
    pub status: EndorsementStatus,
    pub author: String,
    pub timestamp: String,
}

fn shard_path(id: &str) -> String {
    let id = id.trim_start_matches('0');
    let id = if id.len() < 6 {
        &id[..id.len().min(6)]
    } else {
        &id[..6]
    };
    let chars: Vec<char> = id.chars().collect();
    let a: String = chars[..2.min(chars.len())].iter().collect();
    let b: String = chars[2..4.min(chars.len())].iter().collect();
    let rest: String = chars[4..].iter().collect();
    format!("{}/{}/{}", a, b, rest)
}

fn ensure_debt_branch(repo_path: &Path) -> Result<()> {
    let exists = Command::new("git")
        .current_dir(repo_path)
        .args(["rev-parse", "--verify", DEBT_BRANCH])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if exists {
        return Ok(());
    }

    let empty_tree = Command::new("git")
        .current_dir(repo_path)
        .args(["hash-object", "-t", "tree", "--stdin"])
        .stdin(std::process::Stdio::null())
        .output()
        .context("Failed to create empty tree")?;

    if !empty_tree.status.success() {
        anyhow::bail!(
            "Failed to create empty tree: {}",
            String::from_utf8_lossy(&empty_tree.stderr)
        );
    }

    let tree_sha = String::from_utf8_lossy(&empty_tree.stdout)
        .trim()
        .to_string();

    let commit = Command::new("git")
        .current_dir(repo_path)
        .args([
            "commit-tree",
            &tree_sha,
            "-m",
            "init: create cognitive-debt branch",
        ])
        .output()
        .context("Failed to create initial commit")?;

    if !commit.status.success() {
        anyhow::bail!(
            "Failed to create initial commit: {}",
            String::from_utf8_lossy(&commit.stderr)
        );
    }

    let commit_sha = String::from_utf8_lossy(&commit.stdout).trim().to_string();

    let out = Command::new("git")
        .current_dir(repo_path)
        .args(["branch", DEBT_BRANCH, &commit_sha])
        .output()
        .context("Failed to create cognitive-debt branch")?;

    if !out.status.success() {
        anyhow::bail!(
            "Failed to create cognitive-debt branch: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    Ok(())
}

pub struct DebtStore {
    repo_path: PathBuf,
    worktree_path: PathBuf,
}

impl DebtStore {
    pub fn open(repo_path: &Path) -> Result<Self> {
        ensure_debt_branch(repo_path)?;

        let worktree_path = repo_path.join(".git").join("debt-worktree");

        if worktree_path.exists() {
            Command::new("git")
                .current_dir(repo_path)
                .args([
                    "worktree",
                    "remove",
                    "--force",
                    worktree_path.to_str().unwrap(),
                ])
                .output()
                .ok();
            std::fs::remove_dir_all(&worktree_path).ok();
            Command::new("git")
                .current_dir(repo_path)
                .args(["worktree", "prune"])
                .output()
                .ok();
        }

        let out = Command::new("git")
            .current_dir(repo_path)
            .args([
                "worktree",
                "add",
                "--no-checkout",
                worktree_path.to_str().unwrap(),
                DEBT_BRANCH,
            ])
            .output()
            .context("Failed to add debt worktree")?;

        if !out.status.success() {
            anyhow::bail!(
                "Failed to set up debt worktree: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }

        Command::new("git")
            .current_dir(&worktree_path)
            .args(["checkout", DEBT_BRANCH, "--", "."])
            .output()
            .ok();

        Ok(Self {
            repo_path: repo_path.to_path_buf(),
            worktree_path,
        })
    }

    pub fn write_activity(&self, item: &ActivityItem) -> Result<()> {
        let shard = shard_path(&item.id);
        let dir = self.worktree_path.join(&shard);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create shard dir {}", shard))?;

        let activity_path = dir.join("activity.json");
        let json =
            serde_json::to_string_pretty(item).context("Failed to serialize activity item")?;
        std::fs::write(&activity_path, json).context("Failed to write activity.json")?;

        let endorsements_path = dir.join("endorsements.json");
        if !endorsements_path.exists() {
            std::fs::write(&endorsements_path, "[]").context("Failed to init endorsements.json")?;
        }

        Ok(())
    }

    pub fn read_activity(&self, id: &str) -> Result<Option<ActivityItem>> {
        let shard = shard_path(id);
        let activity_path = self.worktree_path.join(&shard).join("activity.json");

        if !activity_path.exists() {
            return Ok(None);
        }

        let json =
            std::fs::read_to_string(&activity_path).context("Failed to read activity.json")?;
        let item = serde_json::from_str(&json).context("Failed to parse activity.json")?;
        Ok(Some(item))
    }

    pub fn write_endorsement(&self, record: &EndorsementRecord) -> Result<()> {
        let shard = shard_path(&record.sha);
        let dir = self.worktree_path.join(&shard);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create shard dir {}", shard))?;

        let endorsements_path = dir.join("endorsements.json");

        let mut records: Vec<EndorsementRecord> = if endorsements_path.exists() {
            let json = std::fs::read_to_string(&endorsements_path)
                .context("Failed to read endorsements.json")?;
            serde_json::from_str(&json).unwrap_or_default()
        } else {
            vec![]
        };

        records.push(record.clone());

        let json =
            serde_json::to_string_pretty(&records).context("Failed to serialize endorsements")?;
        std::fs::write(&endorsements_path, json).context("Failed to write endorsements.json")?;

        if let Ok(Some(mut item)) = self.read_activity(&record.sha) {
            item.endorsement_status = record.status.clone();
            self.write_activity(&item)?;
        }

        Ok(())
    }

    pub fn read_endorsements(&self, id: &str) -> Result<Vec<EndorsementRecord>> {
        let shard = shard_path(id);
        let endorsements_path = self.worktree_path.join(&shard).join("endorsements.json");

        if !endorsements_path.exists() {
            return Ok(vec![]);
        }

        let json = std::fs::read_to_string(&endorsements_path)
            .context("Failed to read endorsements.json")?;
        let records = serde_json::from_str(&json).unwrap_or_default();
        Ok(records)
    }

    pub fn read_all_activity(&self) -> Result<Vec<ActivityItem>> {
        let mut items = Vec::new();
        collect_activity_items(&self.worktree_path, &self.worktree_path, &mut items)?;
        Ok(items)
    }

    pub fn commit(self) -> Result<()> {
        Command::new("git")
            .current_dir(&self.worktree_path)
            .args(["add", "-A"])
            .output()
            .context("Failed to stage debt files")?;

        let status = Command::new("git")
            .current_dir(&self.worktree_path)
            .args(["diff", "--cached", "--quiet"])
            .status()
            .context("Failed to check worktree status")?;

        if !status.success() {
            let head_sha = Command::new("git")
                .current_dir(&self.repo_path)
                .args(["rev-parse", "--short", "HEAD"])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|_| "unknown".to_string());

            let message = format!("debt: update activity items from {}", head_sha);

            let out = Command::new("git")
                .current_dir(&self.worktree_path)
                .args(["commit", "-m", &message])
                .output()
                .context("Failed to commit to cognitive-debt branch")?;

            if !out.status.success() {
                anyhow::bail!(
                    "Failed to commit cognitive-debt branch: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        }

        Command::new("git")
            .current_dir(&self.repo_path)
            .args([
                "worktree",
                "remove",
                "--force",
                self.worktree_path.to_str().unwrap(),
            ])
            .output()
            .context("Failed to remove debt worktree")?;

        Ok(())
    }
}

fn collect_activity_items(_base: &Path, dir: &Path, result: &mut Vec<ActivityItem>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("Failed to read dir {:?}", dir))? {
        let entry = entry?;
        let path = entry.path();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        if name == ".git" {
            continue;
        }

        if path.is_dir() {
            collect_activity_items(_base, &path, result)?;
        } else if name == "activity.json" {
            let json = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {:?}", path))?;
            if let Ok(item) = serde_json::from_str::<ActivityItem>(&json) {
                result.push(item);
            }
        }
    }
    Ok(())
}

pub fn parse_agent_attribution(commit_message: &str) -> Option<f32> {
    for line in commit_message.lines() {
        let line = line.trim();
        if let Some(rest) = line
            .strip_prefix("Agent-Attribution:")
            .or_else(|| line.strip_prefix("Entire-Attribution:"))
        {
            let rest = rest.trim();
            if let Some(pct_str) = rest.split('%').next() {
                if let Ok(pct) = pct_str.trim().parse::<f32>() {
                    return Some(pct / 100.0);
                }
            }
        }
    }
    None
}

pub fn detect_ai_attribution(commit_message: &str) -> (bool, Option<f32>) {
    if let Some(pct) = parse_agent_attribution(commit_message) {
        return (pct >= 0.5, Some(pct));
    }

    let lower = commit_message.to_lowercase();
    let ai = [
        "generated by",
        "co-authored-by: claude",
        "co-authored-by: copilot",
        "cursor",
        "ai-generated",
    ]
    .iter()
    .any(|kw| lower.contains(kw));

    (ai, None)
}

pub fn now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, mo, d, h, mi, s) = epoch_to_parts(secs);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, mi, s)
}

fn epoch_to_parts(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let s = secs % 60;
    let mins = secs / 60;
    let mi = mins % 60;
    let hours = mins / 60;
    let h = hours % 24;
    let days = hours / 24;

    let mut year = 1970u64;
    let mut remaining = days;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }

    let months = [
        31u64,
        if is_leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u64;
    for days_in_month in &months {
        if remaining < *days_in_month {
            break;
        }
        remaining -= days_in_month;
        month += 1;
    }

    (year, month, remaining + 1, h, mi, s)
}

fn is_leap(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}
