use std::path::Path;

use crate::spec::{BasisSpec, SpecError};

pub fn load_spec(path: &Path) -> Result<BasisSpec, SpecError> {
    let content = std::fs::read_to_string(path)?;
    let spec: BasisSpec = serde_yaml::from_str(&content)?;
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
}
