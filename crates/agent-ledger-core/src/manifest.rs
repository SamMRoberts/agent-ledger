use std::{fs, path::Path};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChallengeManifest {
    pub id: String,
    pub name: String,
    pub baseline_repo: String,
    pub allowed_agents: Vec<String>,
    pub time_limit_minutes: u64,
    pub workspace: WorkspaceConfig,
    pub network: NetworkConfig,
    pub commands: CommandsConfig,
    pub scoring: ScoringConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceConfig {
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkConfig {
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandsConfig {
    pub install: String,
    pub test: String,
    pub build: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScoringConfig {
    pub public_tests_weight: u32,
    pub hidden_tests_weight: u32,
    pub token_efficiency_weight: u32,
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("failed to read manifest: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse manifest: {0}")]
    Parse(#[from] serde_yaml::Error),
    #[error("manifest validation failed: {0}")]
    Validation(String),
}

impl ChallengeManifest {
    pub fn load_from_file(path: &Path) -> Result<Self, ManifestError> {
        let contents = fs::read_to_string(path)?;
        let manifest: Self = serde_yaml::from_str(&contents)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.id.trim().is_empty() {
            return Err(ManifestError::Validation("id must not be empty".into()));
        }
        if self.name.trim().is_empty() {
            return Err(ManifestError::Validation("name must not be empty".into()));
        }
        if self.baseline_repo.trim().is_empty() {
            return Err(ManifestError::Validation(
                "baseline_repo must not be empty".into(),
            ));
        }
        if self.allowed_agents.is_empty() {
            return Err(ManifestError::Validation(
                "allowed_agents must not be empty".into(),
            ));
        }
        if self.time_limit_minutes == 0 {
            return Err(ManifestError::Validation(
                "time_limit_minutes must be greater than zero".into(),
            ));
        }
        for agent in &self.allowed_agents {
            match agent.as_str() {
                "copilot" | "codex" | "claude" => {}
                _ => {
                    return Err(ManifestError::Validation(format!(
                        "unsupported agent '{agent}'"
                    )))
                }
            }
        }
        for (field, value) in [
            ("workspace.mode", self.workspace.mode.trim()),
            ("network.mode", self.network.mode.trim()),
            ("commands.install", self.commands.install.trim()),
            ("commands.test", self.commands.test.trim()),
            ("commands.build", self.commands.build.trim()),
        ] {
            if value.is_empty() {
                return Err(ManifestError::Validation(format!(
                    "{field} must not be empty"
                )));
            }
        }
        let total_weight = self.scoring.public_tests_weight
            + self.scoring.hidden_tests_weight
            + self.scoring.token_efficiency_weight;
        if total_weight == 0 {
            return Err(ManifestError::Validation(
                "scoring weights must not all be zero".into(),
            ));
        }
        Ok(())
    }

    pub fn default_manifest() -> Self {
        Self {
            id: "todo-app-2026-06".into(),
            name: "Todo App Challenge".into(),
            baseline_repo: "https://github.com/example/todo-challenge".into(),
            allowed_agents: vec!["copilot".into(), "codex".into()],
            time_limit_minutes: 120,
            workspace: WorkspaceConfig {
                mode: "local".into(),
            },
            network: NetworkConfig {
                mode: "unrestricted".into(),
            },
            commands: CommandsConfig {
                install: "npm install".into(),
                test: "npm test".into(),
                build: "npm run build".into(),
            },
            scoring: ScoringConfig {
                public_tests_weight: 40,
                hidden_tests_weight: 40,
                token_efficiency_weight: 20,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_yaml_parses_correctly() {
        let yaml = r#"
id: todo-app-2026-06
name: Todo App Challenge
baseline_repo: https://github.com/example/todo-challenge
allowed_agents:
  - copilot
  - codex
time_limit_minutes: 120
workspace:
  mode: local
network:
  mode: unrestricted
commands:
  install: npm install
  test: npm test
  build: npm run build
scoring:
  public_tests_weight: 40
  hidden_tests_weight: 40
  token_efficiency_weight: 20
"#;

        let manifest: ChallengeManifest = serde_yaml::from_str(yaml).expect("manifest should parse");
        manifest.validate().expect("manifest should validate");
        assert_eq!(manifest.id, "todo-app-2026-06");
        assert_eq!(manifest.allowed_agents, vec!["copilot", "codex"]);
    }

    #[test]
    fn invalid_yaml_returns_error() {
        let yaml = "id: [unterminated";
        let err = serde_yaml::from_str::<ChallengeManifest>(yaml).unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn missing_required_fields_return_error() {
        let yaml = r#"
id: todo-app-2026-06
name: Todo App Challenge
"#;
        let err = serde_yaml::from_str::<ChallengeManifest>(yaml).unwrap_err();
        assert!(!err.to_string().is_empty());
    }
}
