use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, thiserror::Error)]
pub enum SpecError {
    #[error("Failed to read spec file: {0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to parse spec YAML: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("Extends error: {0}")]
    Extends(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BasisSpec {
    pub governance: Governance,
    /// Path to a parent spec to inherit from. Resolved relative to this spec file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
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

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Governance {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub model: Option<String>,
    /// Whether to apply governance checks to test files.
    /// Default: false — governance applies to production code only.
    #[serde(default)]
    pub check_tests: bool,
}

impl Governance {
    pub fn new(version: &str) -> Self {
        Self {
            version: version.into(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Layer {
    pub role: String,
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(default)]
    pub rules: HashMap<String, String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// If true, this layer represents external (third-party) packages.
    /// External layers are matched by package name against imports,
    /// not by file path prefix.
    #[serde(default)]
    pub external: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NewtypeConfig {
    pub enabled: bool,
    #[serde(default)]
    pub types: Vec<NewtypeDef>,
    #[serde(default)]
    pub exclude_params: Vec<String>,
    #[serde(default)]
    pub exclude_functions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NewtypeDef {
    pub name: String,
    pub wraps: String,
    #[serde(default)]
    pub validation: Option<String>,
    #[serde(default)]
    pub languages: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExhaustiveConfig {
    pub enabled: bool,
    #[serde(default)]
    pub unions: Vec<UnionDef>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UnionDef {
    pub name: String,
    pub variants: Vec<String>,
    #[serde(default)]
    pub languages: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PurityConfig {
    pub enabled: bool,
    #[serde(default)]
    pub forbidden_in_strict: Vec<String>,
    #[serde(default)]
    pub per_layer: HashMap<String, LayerPurityOverride>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LayerPurityOverride {
    /// Additional categories forbidden in this layer beyond the global list.
    #[serde(default)]
    pub also_forbid: Vec<String>,
    /// Categories to exempt from the global forbidden list for this layer.
    #[serde(default)]
    pub allow: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BoundaryConfig {
    pub enabled: bool,
    #[serde(default)]
    pub rules: Vec<BoundaryRule>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BoundaryRule {
    pub from: String,
    pub to: String,
    pub action: String,
    #[serde(default)]
    pub reason: Option<String>,
}


pub const KNOWN_LANGUAGES: &[&str] = &[
    "python", "rust", "js", "go", "java", "kotlin", "ruby", "swift", "csharp",
];

/// Check whether a type definition applies to a given language.
/// `None` means "all languages" (backward compatible default).
pub fn applies_to_lang(languages: &Option<Vec<String>>, lang_name: &str) -> bool {
    match languages {
        None => true,
        Some(langs) => langs.iter().any(|l| l == lang_name),
    }
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

    #[test]
    fn deserialize_newtype_with_languages() {
        let yaml = r#"
governance:
  version: "1.0"
newtypes:
  enabled: true
  types:
    - name: WindowId
      wraps: int
      languages: [js]
    - name: UserId
      wraps: string
"#;
        let spec: BasisSpec = serde_yaml::from_str(yaml).unwrap();
        let types = &spec.newtypes.unwrap().types;
        assert_eq!(types[0].languages, Some(vec!["js".to_string()]));
        assert_eq!(types[1].languages, None);
    }

    #[test]
    fn deserialize_union_with_languages() {
        let yaml = r#"
governance:
  version: "1.0"
exhaustive_matching:
  enabled: true
  unions:
    - name: TaskStatus
      languages: [python]
      variants: [Queued, Running]
    - name: IpcMessage
      variants: [Open, Close]
"#;
        let spec: BasisSpec = serde_yaml::from_str(yaml).unwrap();
        let unions = &spec.exhaustive_matching.unwrap().unions;
        assert_eq!(unions[0].languages, Some(vec!["python".to_string()]));
        assert_eq!(unions[1].languages, None);
    }

    #[test]
    fn deserialize_external_layer() {
        let yaml = r#"
governance:
  version: "1.0"
layers:
  serialization:
    external: true
    role: "Serialization tools"
    packages: [serde, thiserror]
  dictionary:
    role: "Data types"
    packages: ["src/models"]
    depends_on: [serialization]
"#;
        let spec: BasisSpec = serde_yaml::from_str(yaml).unwrap();
        assert!(spec.layers["serialization"].external);
        assert!(!spec.layers["dictionary"].external);
    }

    #[test]
    fn external_defaults_to_false() {
        let yaml = "governance:\n  version: '1.0'\nlayers:\n  dict:\n    role: data\n    packages: ['src/models']\n    depends_on: []\n";
        let spec: BasisSpec = serde_yaml::from_str(yaml).unwrap();
        assert!(!spec.layers["dict"].external);
    }

    #[test]
    fn applies_to_lang_none_matches_all() {
        assert!(applies_to_lang(&None, "python"));
        assert!(applies_to_lang(&None, "js"));
    }

    #[test]
    fn applies_to_lang_some_filters() {
        let langs = Some(vec!["python".to_string(), "js".to_string()]);
        assert!(applies_to_lang(&langs, "python"));
        assert!(applies_to_lang(&langs, "js"));
        assert!(!applies_to_lang(&langs, "rust"));
        assert!(!applies_to_lang(&langs, "go"));
    }
}
