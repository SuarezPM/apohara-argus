//! Argus Prompt Library loader.
//!
//! The 4 prompt .md files live in `crates/apohara-argus-core/prompts/`. They have
//! YAML frontmatter (model, temperature, max_tokens, output_format) and a
//! Markdown body. This loader reads them at runtime.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use super::errors::{ArgusError, Result};

/// A single prompt loaded from a .md file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    pub metadata: PromptMetadata,
    pub body: String,
}

/// Parsed YAML frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptMetadata {
    pub name: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u32,
    pub description: String,
    pub output_format: String,
}

/// A library of named prompts.
#[derive(Debug, Clone, Default)]
pub struct PromptLibrary {
    prompts: HashMap<String, Prompt>,
}

impl PromptLibrary {
    /// Load all 4 prompts from the embedded directory.
    /// The .md files are compiled into the binary via `include_str!`.
    pub fn load_embedded() -> Result<Self> {
        let raw = [
            (
                "01-slop-detector",
                include_str!("../prompts/01-slop-detector.md"),
            ),
            (
                "02-redteam-security",
                include_str!("../prompts/02-redteam-security.md"),
            ),
            (
                "03-architecture-fit",
                include_str!("../prompts/03-architecture-fit.md"),
            ),
            (
                "04-verdict-synthesizer",
                include_str!("../prompts/04-verdict-synthesizer.md"),
            ),
        ];
        let mut prompts = HashMap::new();
        for (name, content) in raw {
            let prompt = parse_prompt(name, content)?;
            prompts.insert(prompt.metadata.name.clone(), prompt);
        }
        Ok(Self { prompts })
    }

    /// Load from a directory on disk (for dev / hot-reload).
    pub fn load_from_dir<P: AsRef<Path>>(dir: P) -> Result<Self> {
        let mut prompts = HashMap::new();
        for entry in std::fs::read_dir(dir.as_ref())
            .map_err(|e| ArgusError::Internal(format!("read_dir failed: {}", e)))?
        {
            let entry = entry.map_err(|e| ArgusError::Internal(e.to_string()))?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");
            let content =
                std::fs::read_to_string(&path).map_err(|e| ArgusError::Internal(e.to_string()))?;
            let prompt = parse_prompt(name, &content)?;
            prompts.insert(prompt.metadata.name.clone(), prompt);
        }
        Ok(Self { prompts })
    }

    pub fn get(&self, name: &str) -> Option<&Prompt> {
        self.prompts.get(name)
    }

    pub fn list(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.prompts.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    pub fn len(&self) -> usize {
        self.prompts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.prompts.is_empty()
    }
}

fn parse_prompt(name: &str, content: &str) -> Result<Prompt> {
    // Expect: ---\n<yaml>\n---\n<body>
    let content = content.trim_start_matches('\u{feff}'); // strip BOM
    if !content.starts_with("---") {
        return Err(ArgusError::PromptNotFound(format!(
            "Prompt '{}' has no YAML frontmatter",
            name
        )));
    }
    let after_first = &content[3..];
    let after_first = after_first.trim_start_matches('\n');
    if let Some(end) = after_first.find("\n---") {
        let yaml_str = &after_first[..end];
        let body = after_first[end + 4..].trim_start_matches('\n').to_string();
        let metadata: PromptMetadata = serde_yaml::from_str(yaml_str).map_err(|e| {
            ArgusError::PromptNotFound(format!("Invalid YAML in '{}': {}", name, e))
        })?;
        Ok(Prompt { metadata, body })
    } else {
        Err(ArgusError::PromptNotFound(format!(
            "Prompt '{}' has unterminated frontmatter",
            name
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_4_embedded_prompts() {
        let lib = PromptLibrary::load_embedded().expect("should load");
        assert_eq!(lib.len(), 4);
        for name in [
            "slop-detector",
            "redteam-security",
            "architecture-fit",
            "verdict-synthesizer",
        ] {
            let p = lib
                .get(name)
                .unwrap_or_else(|| panic!("missing prompt: {}", name));
            assert!(!p.body.is_empty());
            assert!(!p.metadata.model.is_empty());
        }
    }

    #[test]
    fn parses_frontmatter_correctly() {
        let md = "---\nname: foo\nmodel: bar\ntemperature: 0.5\nmax_tokens: 100\ndescription: desc\noutput_format: json\n---\n\nThis is the body.\n";
        let p = parse_prompt("foo", md).expect("parse ok");
        assert_eq!(p.metadata.name, "foo");
        assert_eq!(p.metadata.model, "bar");
        assert!((p.metadata.temperature - 0.5).abs() < 1e-6);
        assert_eq!(p.metadata.max_tokens, 100);
        assert!(p.body.contains("This is the body."));
    }

    #[test]
    fn parse_prompt_without_frontmatter_errors() {
        // A .md file that doesn't start with `---` has no YAML
        // frontmatter and must be rejected with PromptNotFound.
        let md = "This is just body text without frontmatter.\n";
        let res = parse_prompt("no-frontmatter", md);
        assert!(res.is_err());
        match res {
            Err(ArgusError::PromptNotFound(msg)) => {
                assert!(msg.contains("no YAML frontmatter"));
            }
            other => panic!("expected PromptNotFound, got {:?}", other),
        }
    }

    #[test]
    fn parse_prompt_with_unterminated_frontmatter_errors() {
        // A .md file that starts with `---` but never closes the
        // frontmatter block must be rejected.
        let md = "---\nname: foo\nmodel: bar\nThis body never gets separated from the YAML.\n";
        let res = parse_prompt("unterminated", md);
        assert!(res.is_err());
        match res {
            Err(ArgusError::PromptNotFound(msg)) => {
                assert!(msg.contains("unterminated frontmatter"));
            }
            other => panic!("expected PromptNotFound, got {:?}", other),
        }
    }

    #[test]
    fn parse_prompt_with_invalid_yaml_errors() {
        // A .md file with a closed frontmatter block but malformed
        // YAML must be rejected with the YAML error message.
        let md = "---\nname: foo\nmodel: [unclosed bracket\n---\n\nBody.\n";
        let res = parse_prompt("bad-yaml", md);
        assert!(res.is_err());
        match res {
            Err(ArgusError::PromptNotFound(msg)) => {
                assert!(msg.contains("Invalid YAML"));
            }
            other => panic!("expected PromptNotFound, got {:?}", other),
        }
    }

    #[test]
    fn parse_prompt_strips_bom() {
        // The UTF-8 BOM (U+FEFF) is common in Windows-edited files.
        // The parser must strip it before checking for the frontmatter
        // delimiter.
        let md = "\u{feff}---\nname: foo\nmodel: bar\ntemperature: 0.5\nmax_tokens: 100\ndescription: desc\noutput_format: json\n---\n\nBody after BOM.\n";
        let p = parse_prompt("with-bom", md).expect("BOM should be stripped");
        assert_eq!(p.metadata.name, "foo");
    }

    #[test]
    fn list_returns_sorted_names() {
        let lib = PromptLibrary::load_embedded().expect("should load");
        let names = lib.list();
        assert_eq!(names.len(), 4);
        // Must be sorted alphabetically.
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn is_empty_on_empty_library() {
        let lib = PromptLibrary::default();
        assert!(lib.is_empty());
        assert_eq!(lib.len(), 0);
        assert!(lib.list().is_empty());
        assert!(lib.get("anything").is_none());
    }

    #[test]
    fn is_empty_on_loaded_library() {
        let lib = PromptLibrary::load_embedded().expect("should load");
        assert!(!lib.is_empty());
    }

    #[test]
    fn load_from_dir_skips_non_md_files() {
        // The directory loader must only pick up .md files. Other
        // extensions (e.g., .txt, .yaml) in the directory are
        // silently skipped — they don't error and they don't load.
        let dir = std::env::temp_dir().join("argus-prompts-test-skips");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Write one valid .md and one .txt that must be skipped.
        std::fs::write(
            dir.join("good.md"),
            "---\nname: good\nmodel: m\ntemperature: 0.5\nmax_tokens: 100\ndescription: d\noutput_format: json\n---\n\nbody\n",
        )
        .unwrap();
        std::fs::write(dir.join("ignored.txt"), "not a prompt").unwrap();
        let lib = PromptLibrary::load_from_dir(&dir).expect("should load");
        assert_eq!(lib.len(), 1);
        assert!(lib.get("good").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_from_dir_errors_on_missing_directory() {
        let dir = std::env::temp_dir().join("argus-prompts-test-nonexistent-xyz");
        let _ = std::fs::remove_dir_all(&dir);
        let res = PromptLibrary::load_from_dir(&dir);
        assert!(res.is_err());
        match res {
            Err(ArgusError::Internal(msg)) => {
                assert!(msg.contains("read_dir failed"));
            }
            other => panic!("expected ArgusError::Internal, got {:?}", other),
        }
    }

    #[test]
    fn load_from_dir_errors_on_invalid_prompt_file() {
        // If a .md file in the directory is malformed (no
        // frontmatter), the loader must propagate the error from
        // parse_prompt. The error stops the load — partial loads
        // are not returned.
        let dir = std::env::temp_dir().join("argus-prompts-test-invalid");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bad.md"), "no frontmatter here\n").unwrap();
        let res = PromptLibrary::load_from_dir(&dir);
        assert!(res.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
