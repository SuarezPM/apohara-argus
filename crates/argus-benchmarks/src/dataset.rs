//! Dataset loader for `data/prs.jsonl` and `data/labels.json`.
//!
//! The dataset is the **ground truth** for the P/R bench. Each PR
//! has: an `id`, a `title`, a Rust `diff` (the snippet the
//! deterministic rules + LLM will analyze), a `ground_truth` label
//! (`slop` or `clean`), the `language` (always `rust` for now), and
//! a `source` (`synthetic` for hand-crafted cases, `real-pr` for
//! diffs adapted from open-source projects).
//!
//! The dataset is intentionally honest: 30 entries, not 100, and
//! includes borderline / hard cases that a tuned detector would
//! miss. The P/R bench reports what ARGUS actually achieves — not
//! the best number we can cherry-pick.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Binary ground truth. Mirrored 1:1 in the JSONL `ground_truth`
/// string field; the enum makes the bench code total over labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Label {
    /// The diff is AI-generated slop. The detector SHOULD flag it.
    Slop,
    /// The diff is human-written, normal Rust. The detector SHOULD
    /// NOT flag it.
    Clean,
}

impl Label {
    pub fn is_slop(self) -> bool {
        matches!(self, Self::Slop)
    }
}

/// One labeled PR. The shape of a `prs.jsonl` line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabeledPR {
    pub id: String,
    pub title: String,
    pub diff: String,
    pub ground_truth: Label,
    pub language: String,
    pub source: String,
    /// Optional free-text note (e.g. the slop category or the
    /// real-PR style being referenced). Documented in
    /// `data/prs.jsonl` and reproduced in `docs/BENCHMARK.md`.
    #[serde(default)]
    pub notes: Option<String>,
}

impl LabeledPR {
    /// Convenience: the `Label` version of `ground_truth`.
    pub fn label(&self) -> Label {
        self.ground_truth
    }
}

/// `data/labels.json` shape: `{ "<id>": "slop" | "clean", ... }`.
/// This is a redundant view of the same data, kept as a separate
/// file so the bench can cross-check `prs.jsonl` against
/// `labels.json` and fail loudly if they disagree.
pub type LabelMap = HashMap<String, String>;

#[derive(Error, Debug)]
pub enum DatasetError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parse error in {path}: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("labels.json and prs.jsonl disagree on PR '{id}': label={label}, ground_truth={gt}")]
    LabelMismatch {
        id: String,
        label: String,
        gt: String,
    },
}

/// Load the labeled PR dataset from a JSONL file. One
/// `LabeledPR` per line.
pub fn load_prs_jsonl<P: AsRef<Path>>(path: P) -> Result<Vec<LabeledPR>, DatasetError> {
    let text = fs::read_to_string(path.as_ref())?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let pr: LabeledPR = serde_json::from_str(line).map_err(|e| DatasetError::Json {
            path: format!("{}:line {}", path.as_ref().display(), i + 1),
            source: e,
        })?;
        out.push(pr);
    }
    Ok(out)
}

/// Load `data/labels.json` (the redundant ground-truth map).
pub fn load_labels<P: AsRef<Path>>(path: P) -> Result<LabelMap, DatasetError> {
    let text = fs::read_to_string(path.as_ref())?;
    let map: LabelMap = serde_json::from_str(&text).map_err(|e| DatasetError::Json {
        path: path.as_ref().display().to_string(),
        source: e,
    })?;
    Ok(map)
}

/// Load both, cross-check, and return `(prs, labels)`. The cross-
/// check is the bench's "the dataset is internally consistent"
/// gate.
pub fn load_dataset<P1: AsRef<Path>, P2: AsRef<Path>>(
    prs_path: P1,
    labels_path: P2,
) -> Result<(Vec<LabeledPR>, LabelMap), DatasetError> {
    let prs = load_prs_jsonl(&prs_path)?;
    let labels = load_labels(&labels_path)?;
    for pr in &prs {
        if let Some(label_str) = labels.get(&pr.id) {
            let expected = match label_str.as_str() {
                "slop" => Label::Slop,
                "clean" => Label::Clean,
                other => {
                    return Err(DatasetError::Json {
                        path: labels_path.as_ref().display().to_string(),
                        source: serde_json::Error::io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("unknown label '{}' for id '{}'", other, pr.id),
                        )),
                    });
                }
            };
            if expected != pr.ground_truth {
                return Err(DatasetError::LabelMismatch {
                    id: pr.id.clone(),
                    label: label_str.clone(),
                    gt: format!("{:?}", pr.ground_truth),
                });
            }
        }
    }
    Ok((prs, labels))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_is_slop() {
        assert!(Label::Slop.is_slop());
        assert!(!Label::Clean.is_slop());
    }

    #[test]
    fn parse_minimal_prs_line() {
        let line = r#"{"id":"x","title":"t","diff":"fn f() {}","ground_truth":"slop","language":"rust","source":"synthetic"}"#;
        let pr: LabeledPR = serde_json::from_str(line).unwrap();
        assert_eq!(pr.id, "x");
        assert_eq!(pr.ground_truth, Label::Slop);
        assert!(pr.notes.is_none());
    }

    #[test]
    fn label_returns_ground_truth() {
        let pr_slop = LabeledPR {
            id: "a".into(),
            title: "t".into(),
            diff: String::new(),
            ground_truth: Label::Slop,
            language: "rust".into(),
            source: "synthetic".into(),
            notes: None,
        };
        let pr_clean = LabeledPR {
            ground_truth: Label::Clean,
            ..pr_slop.clone()
        };
        assert_eq!(pr_slop.label(), Label::Slop);
        assert_eq!(pr_clean.label(), Label::Clean);
    }

    #[test]
    fn load_prs_jsonl_reads_multiple_lines() {
        // The JSONL loader must skip blank lines and parse
        // each non-blank line as a separate LabeledPR.
        let dir = std::env::temp_dir().join("argus-bench-dataset-prs-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("prs.jsonl");
        let content = "\
{\"id\":\"a\",\"title\":\"t\",\"diff\":\"+ let x = 1;\",\"ground_truth\":\"slop\",\"language\":\"rust\",\"source\":\"synthetic\"}\n\
\n\
{\"id\":\"b\",\"title\":\"t2\",\"diff\":\"+ let y = 2;\",\"ground_truth\":\"clean\",\"language\":\"rust\",\"source\":\"real-pr\"}\n";
        std::fs::write(&path, content).unwrap();
        let prs = load_prs_jsonl(&path).expect("load ok");
        assert_eq!(prs.len(), 2);
        assert_eq!(prs[0].id, "a");
        assert_eq!(prs[1].id, "b");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_prs_jsonl_errors_on_missing_file() {
        let path = std::env::temp_dir().join("argus-bench-nonexistent-xyz.jsonl");
        let _ = std::fs::remove_file(&path);
        let res = load_prs_jsonl(&path);
        assert!(res.is_err());
    }

    #[test]
    fn load_prs_jsonl_errors_on_invalid_json() {
        // A line that is not valid JSON must produce a
        // DatasetError::Json with the line number in the path.
        let dir = std::env::temp_dir().join("argus-bench-dataset-invalid-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("prs.jsonl");
        std::fs::write(&path, "not valid json\n").unwrap();
        let res = load_prs_jsonl(&path);
        match res {
            Err(DatasetError::Json { path: p, .. }) => {
                assert!(p.contains("line 1"));
            }
            other => panic!("expected DatasetError::Json, got {:?}", other),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_labels_reads_json_map() {
        // The labels.json file is a flat string→string map.
        let dir = std::env::temp_dir().join("argus-bench-labels-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("labels.json");
        std::fs::write(&path, r#"{"a":"slop","b":"clean"}"#).unwrap();
        let labels = load_labels(&path).expect("load ok");
        assert_eq!(labels.len(), 2);
        assert_eq!(labels.get("a").map(|s| s.as_str()), Some("slop"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_labels_errors_on_invalid_json() {
        let dir = std::env::temp_dir().join("argus-bench-labels-invalid-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("labels.json");
        std::fs::write(&path, "not json").unwrap();
        let res = load_labels(&path);
        assert!(matches!(res, Err(DatasetError::Json { .. })));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_dataset_cross_check_succeeds_when_consistent() {
        // When prs.jsonl and labels.json agree on every id,
        // load_dataset returns Ok with both collections.
        let dir = std::env::temp_dir().join("argus-bench-dataset-crosscheck-ok");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("prs.jsonl"),
            "{\"id\":\"a\",\"title\":\"t\",\"diff\":\"+x\",\"ground_truth\":\"slop\",\"language\":\"rust\",\"source\":\"synthetic\"}\n",
        ).unwrap();
        std::fs::write(dir.join("labels.json"), r#"{"a":"slop"}"#).unwrap();
        let (prs, labels) =
            load_dataset(dir.join("prs.jsonl"), dir.join("labels.json")).expect("cross-check ok");
        assert_eq!(prs.len(), 1);
        assert_eq!(labels.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_dataset_cross_check_errors_on_label_mismatch() {
        // prs.jsonl says id "a" is slop, labels.json says
        // clean → LabelMismatch error.
        let dir = std::env::temp_dir().join("argus-bench-dataset-mismatch");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("prs.jsonl"),
            "{\"id\":\"a\",\"title\":\"t\",\"diff\":\"+x\",\"ground_truth\":\"slop\",\"language\":\"rust\",\"source\":\"synthetic\"}\n",
        ).unwrap();
        std::fs::write(dir.join("labels.json"), r#"{"a":"clean"}"#).unwrap();
        let res = load_dataset(dir.join("prs.jsonl"), dir.join("labels.json"));
        match res {
            Err(DatasetError::LabelMismatch { id, label, gt }) => {
                assert_eq!(id, "a");
                assert_eq!(label, "clean");
                assert!(gt.contains("Slop"));
            }
            other => panic!("expected LabelMismatch, got {:?}", other),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_dataset_cross_check_errors_on_unknown_label() {
        // labels.json contains a label string other than
        // "slop" or "clean" → DatasetError::Json.
        let dir = std::env::temp_dir().join("argus-bench-dataset-unknown-label");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("prs.jsonl"),
            "{\"id\":\"a\",\"title\":\"t\",\"diff\":\"+x\",\"ground_truth\":\"slop\",\"language\":\"rust\",\"source\":\"synthetic\"}\n",
        ).unwrap();
        std::fs::write(dir.join("labels.json"), r#"{"a":"maybe"}"#).unwrap();
        let res = load_dataset(dir.join("prs.jsonl"), dir.join("labels.json"));
        assert!(matches!(res, Err(DatasetError::Json { .. })));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_dataset_allows_ids_only_in_prs() {
        // If a PR id is in prs.jsonl but not in labels.json,
        // the cross-check skips it (no error). This is by
        // design — labels.json is a subset that cross-validates
        // the PRs that ARE labeled.
        let dir = std::env::temp_dir().join("argus-bench-dataset-prs-only");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("prs.jsonl"),
            "{\"id\":\"a\",\"title\":\"t\",\"diff\":\"+x\",\"ground_truth\":\"slop\",\"language\":\"rust\",\"source\":\"synthetic\"}\n",
        ).unwrap();
        std::fs::write(dir.join("labels.json"), r#"{}"#).unwrap();
        let (prs, labels) =
            load_dataset(dir.join("prs.jsonl"), dir.join("labels.json")).expect("prs-only is ok");
        assert_eq!(prs.len(), 1);
        assert_eq!(labels.len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
