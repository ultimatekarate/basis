use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, thiserror::Error)]
pub enum SpecError {
    #[error("Failed to read spec file: {0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to parse spec YAML: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BasisSpec {
    pub governance: Governance,
    #[serde(default)]
    pub layers: HashMap<String, Layer>,
    #[serde(default)]
    pub newtypes: Option<NewtypeConfig>,
    #[serde(default)]
    pub exhaustive_matching: Option<ExhaustiveConfig>,
    #[serde(default)]
    pub purity: Option<PurityConfig>,
    #[serde(default)]
    pub boundaries: Option<BoundaryConfig>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Governance {
    pub version: String,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Layer {
    pub role: String,
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(default)]
    pub rules: HashMap<String, String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NewtypeConfig {
    pub enabled: bool,
    #[serde(default)]
    pub types: Vec<NewtypeDef>,
    #[serde(default)]
    pub exclude_params: Vec<String>,
    #[serde(default)]
    pub exclude_functions: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NewtypeDef {
    pub name: String,
    pub wraps: String,
    #[serde(default)]
    pub validation: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ExhaustiveConfig {
    pub enabled: bool,
    #[serde(default)]
    pub unions: Vec<UnionDef>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UnionDef {
    pub name: String,
    pub variants: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PurityConfig {
    pub enabled: bool,
    #[serde(default)]
    pub forbidden_in_strict: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BoundaryConfig {
    pub enabled: bool,
    #[serde(default)]
    pub rules: Vec<BoundaryRule>,
    #[serde(default)]
    pub external: HashMap<String, ExternalRules>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BoundaryRule {
    pub from: String,
    pub to: String,
    pub action: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ExternalRules {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny_patterns: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_error_display() {
        let err = SpecError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        let msg = format!("{err}");
        assert!(msg.contains("file not found"));
    }

    #[test]
    fn deserialize_minimal_spec() {
        let yaml = "governance:\n  version: '1.0'\n";
        let spec: BasisSpec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spec.governance.version, "1.0");
        assert!(spec.layers.is_empty());
    }

    #[test]
    fn deserialize_full_spec() {
        let yaml = "governance:\n  version: '1.0'\nlayers:\n  dict:\n    role: data\n    packages: ['src/models']\n    depends_on: []\nnewtypes:\n  enabled: true\n  types:\n    - name: UserId\n      wraps: string\n";
        let spec: BasisSpec = serde_yaml::from_str(yaml).unwrap();
        assert!(spec.layers.contains_key("dict"));
        assert_eq!(spec.newtypes.unwrap().types[0].name, "UserId");
    }
}
