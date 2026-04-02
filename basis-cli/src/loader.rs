use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::spec::{BasisSpec, SpecError};
use crate::validate;

/// Maximum depth for extends chains, to prevent infinite loops.
const MAX_EXTENDS_DEPTH: usize = 10;

/// Load a spec file, resolving any `extends` chain.
/// Each parent spec is loaded and merged (child overrides parent).
/// Circular extends are detected and rejected.
/// Remote URLs (http:// or https://) in `extends` are fetched over the network.
pub fn load_spec(path: &Path) -> Result<BasisSpec, SpecError> {
    load_spec_with_chain(path, &mut HashSet::new(), 0)
}

/// Load a single spec file without resolving extends.
pub fn load_spec_raw(path: &Path) -> Result<BasisSpec, SpecError> {
    let content = std::fs::read_to_string(path)?;
    let spec: BasisSpec = serde_yaml::from_str(&content)?;
    Ok(spec)
}

/// Returns true if the extends value is a remote URL (http:// or https://).
fn is_remote_url(extends: &str) -> bool {
    extends.starts_with("http://") || extends.starts_with("https://")
}

/// Fetch a remote spec over HTTP/HTTPS and parse it.
fn fetch_remote_spec(url: &str) -> Result<BasisSpec, SpecError> {
    let body = ureq::get(url)
        .call()
        .map_err(|e| SpecError::Network(format!("{url}: {e}")))?
        .into_string()
        .map_err(|e| SpecError::Network(format!("{url}: failed to read body: {e}")))?;

    let spec: BasisSpec = serde_yaml::from_str(&body)?;
    Ok(spec)
}

fn load_spec_with_chain(
    path: &Path,
    seen: &mut HashSet<PathBuf>,
    depth: usize,
) -> Result<BasisSpec, SpecError> {
    if depth > MAX_EXTENDS_DEPTH {
        return Err(SpecError::Extends(format!(
            "extends chain exceeds maximum depth of {MAX_EXTENDS_DEPTH}"
        )));
    }

    let canonical = path.canonicalize().map_err(|e| {
        std::io::Error::new(e.kind(), format!("{}: {e}", path.display()))
    })?;

    if !seen.insert(canonical.clone()) {
        return Err(SpecError::Extends(format!(
            "circular extends detected: {} was already in the chain",
            path.display()
        )));
    }

    let child = load_spec_raw(path)?;

    let Some(extends_ref) = &child.extends else {
        return Ok(child);
    };

    let parent = if is_remote_url(extends_ref) {
        // Remote spec: fetch over HTTP/HTTPS, then resolve its own extends chain
        let mut remote_spec = fetch_remote_spec(extends_ref)?;
        if let Some(nested_extends) = &remote_spec.extends {
            if is_remote_url(nested_extends) {
                // Remote parent also extends remotely — resolve recursively
                let nested_parent = resolve_remote_chain(nested_extends, seen, depth + 1)?;
                remote_spec = validate::merge_specs(&nested_parent, &remote_spec);
                remote_spec.extends = None;
            } else {
                return Err(SpecError::Extends(format!(
                    "remote spec {extends_ref} uses a relative extends path '{}' — remote specs can only extend other remote URLs",
                    nested_extends
                )));
            }
        }
        remote_spec
    } else {
        // Local spec: resolve path relative to current spec file's directory
        let parent_path = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(extends_ref);
        load_spec_with_chain(&parent_path, seen, depth + 1)?
    };

    // Pure merge: laboratory function, no IO
    Ok(validate::merge_specs(&parent, &child))
}

/// Resolve a chain of remote extends (remote → remote → ...).
/// Uses URL strings for circular detection since remote specs have no canonical path.
fn resolve_remote_chain(
    url: &str,
    seen: &mut HashSet<PathBuf>,
    depth: usize,
) -> Result<BasisSpec, SpecError> {
    if depth > MAX_EXTENDS_DEPTH {
        return Err(SpecError::Extends(format!(
            "extends chain exceeds maximum depth of {MAX_EXTENDS_DEPTH}"
        )));
    }

    // Use the URL as a synthetic path for circular detection
    let synthetic = PathBuf::from(url);
    if !seen.insert(synthetic) {
        return Err(SpecError::Extends(format!(
            "circular extends detected: {url} was already in the chain"
        )));
    }

    let mut spec = fetch_remote_spec(url)?;

    if let Some(nested) = &spec.extends {
        if is_remote_url(nested) {
            let parent = resolve_remote_chain(nested, seen, depth + 1)?;
            spec = validate::merge_specs(&parent, &spec);
            spec.extends = None;
        } else {
            return Err(SpecError::Extends(format!(
                "remote spec {url} uses a relative extends path '{}' — remote specs can only extend other remote URLs",
                nested
            )));
        }
    }

    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn load_valid_spec() {
        let dir = tempfile::tempdir().unwrap();
        let spec_path = dir.path().join("basis.yaml");
        let mut f = std::fs::File::create(&spec_path).unwrap();
        f.write_all(b"governance:\n  version: '1.0'\n").unwrap();

        let spec = load_spec(&spec_path).unwrap();
        assert_eq!(spec.governance.version, "1.0");
    }

    #[test]
    fn load_spec_file_not_found() {
        let result = load_spec(Path::new("/nonexistent/basis.yaml"));
        assert!(result.is_err());
    }

    #[test]
    fn load_spec_invalid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let spec_path = dir.path().join("basis.yaml");
        let mut f = std::fs::File::create(&spec_path).unwrap();
        f.write_all(b"{{not valid yaml").unwrap();
        assert!(load_spec(&spec_path).is_err());
    }

    // ── extends tests ──────────────────────────────────────

    fn write_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn extends_single_parent() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "base.yaml",
            r#"
governance:
  version: "1.0"
layers:
  domain:
    role: "Base domain"
    packages: ["src/models"]
    depends_on: []
newtypes:
  enabled: true
  types:
    - name: UserId
      wraps: string
"#,
        );
        write_file(
            dir.path(),
            "child.yaml",
            r#"
governance:
  version: "1.0"
extends: "base.yaml"
layers:
  api:
    role: "API layer"
    packages: ["src/api"]
    depends_on: [domain]
"#,
        );

        let spec = load_spec(&dir.path().join("child.yaml")).unwrap();
        // Inherited from parent
        assert!(spec.layers.contains_key("domain"));
        assert!(spec.newtypes.is_some());
        assert_eq!(spec.newtypes.as_ref().unwrap().types.len(), 1);
        // From child
        assert!(spec.layers.contains_key("api"));
        // extends cleared
        assert!(spec.extends.is_none());
    }

    #[test]
    fn extends_grandparent_chain() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "grandparent.yaml",
            r#"
governance:
  version: "1.0"
newtypes:
  enabled: true
  types:
    - name: UserId
      wraps: string
"#,
        );
        write_file(
            dir.path(),
            "parent.yaml",
            r#"
governance:
  version: "1.0"
extends: "grandparent.yaml"
newtypes:
  enabled: true
  types:
    - name: OrderId
      wraps: string
"#,
        );
        write_file(
            dir.path(),
            "child.yaml",
            r#"
governance:
  version: "1.0"
extends: "parent.yaml"
layers:
  api:
    role: "API"
    packages: ["src/api"]
    depends_on: []
"#,
        );

        let spec = load_spec(&dir.path().join("child.yaml")).unwrap();
        // Accumulated from grandparent + parent
        let nt = spec.newtypes.unwrap();
        assert_eq!(nt.types.len(), 2);
        assert!(nt.types.iter().any(|t| t.name == "UserId"));
        assert!(nt.types.iter().any(|t| t.name == "OrderId"));
        // From child
        assert!(spec.layers.contains_key("api"));
    }

    #[test]
    fn extends_circular_detected() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "a.yaml",
            "governance:\n  version: '1.0'\nextends: 'b.yaml'\n",
        );
        write_file(
            dir.path(),
            "b.yaml",
            "governance:\n  version: '1.0'\nextends: 'a.yaml'\n",
        );

        let result = load_spec(&dir.path().join("a.yaml"));
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("circular"), "Expected circular error, got: {err}");
    }

    #[test]
    fn extends_self_circular() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "self.yaml",
            "governance:\n  version: '1.0'\nextends: 'self.yaml'\n",
        );

        let result = load_spec(&dir.path().join("self.yaml"));
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("circular"), "Expected circular error, got: {err}");
    }

    #[test]
    fn extends_missing_parent_fails() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "child.yaml",
            "governance:\n  version: '1.0'\nextends: 'nonexistent.yaml'\n",
        );

        let result = load_spec(&dir.path().join("child.yaml"));
        assert!(result.is_err());
    }

    #[test]
    fn extends_child_overrides_parent_layer() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "base.yaml",
            r#"
governance:
  version: "1.0"
layers:
  domain:
    role: "Parent domain"
    packages: ["src/models"]
    depends_on: []
"#,
        );
        write_file(
            dir.path(),
            "child.yaml",
            r#"
governance:
  version: "1.0"
extends: "base.yaml"
layers:
  domain:
    role: "Child domain"
    packages: ["src/domain"]
    depends_on: []
"#,
        );

        let spec = load_spec(&dir.path().join("child.yaml")).unwrap();
        assert_eq!(spec.layers["domain"].role, "Child domain");
        assert_eq!(spec.layers["domain"].packages, vec!["src/domain"]);
    }

    #[test]
    fn no_extends_works_as_before() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "simple.yaml",
            "governance:\n  version: '1.0'\nlayers:\n  a:\n    role: test\n    packages: []\n    depends_on: []\n",
        );

        let spec = load_spec(&dir.path().join("simple.yaml")).unwrap();
        assert!(spec.layers.contains_key("a"));
        assert!(spec.extends.is_none());
    }

    // ── Remote extends tests ──────────────────────────────

    #[test]
    fn is_remote_url_detects_https() {
        assert!(is_remote_url("https://example.com/base.yaml"));
        assert!(is_remote_url("http://internal.corp/spec.yaml"));
        assert!(!is_remote_url("../shared/base.yaml"));
        assert!(!is_remote_url("base.yaml"));
        assert!(!is_remote_url("/absolute/path/base.yaml"));
    }

    #[test]
    fn remote_extends_unreachable_host_returns_network_error() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "child.yaml",
            "governance:\n  version: '1.0'\nextends: 'https://this-host-does-not-exist.invalid/base.yaml'\n",
        );

        let result = load_spec(&dir.path().join("child.yaml"));
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("remote") || err.contains("fetch") || err.contains("Network"),
            "Expected network error, got: {err}"
        );
    }

    #[test]
    fn local_extends_still_works_with_remote_support() {
        // Regression: ensure local extends is unaffected by remote URL detection
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "base.yaml",
            "governance:\n  version: '1.0'\nnewtypes:\n  enabled: true\n  types:\n    - name: UserId\n      wraps: string\n",
        );
        write_file(
            dir.path(),
            "child.yaml",
            "governance:\n  version: '1.0'\nextends: 'base.yaml'\n",
        );

        let spec = load_spec(&dir.path().join("child.yaml")).unwrap();
        assert!(spec.newtypes.is_some());
        assert_eq!(spec.newtypes.unwrap().types.len(), 1);
    }
}
