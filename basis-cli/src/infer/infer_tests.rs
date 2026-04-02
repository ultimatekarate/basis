use super::*;

#[test]
fn extract_python_params() {
    let content = "def get_user(user_id: str, name: str) -> None:\n    pass\n";
    let params = extraction::extract_raw_params(content, "python");
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].param_name, "user_id");
    assert_eq!(params[0].raw_type, "str");
    assert_eq!(params[0].function_name, "get_user");
    assert_eq!(params[1].param_name, "name");
}

#[test]
fn extract_python_untyped() {
    let content = "def foo(x, y):\n    pass\n";
    let params = extraction::extract_raw_params(content, "python");
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].raw_type, "");
}

#[test]
fn extract_rust_params() {
    let content = "pub fn get_user(user_id: String, count: usize) -> User {\n}\n";
    let params = extraction::extract_raw_params(content, "rust");
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].param_name, "user_id");
    assert_eq!(params[0].raw_type, "String");
    assert_eq!(params[1].param_name, "count");
    assert_eq!(params[1].raw_type, "usize");
}

#[test]
fn extract_java_params() {
    let content = "public User getUser(String userId, int count) {\n}\n";
    let params = extraction::extract_raw_params(content, "java");
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].param_name, "userId");
    assert_eq!(params[0].raw_type, "String");
    assert_eq!(params[1].param_name, "count");
    assert_eq!(params[1].raw_type, "int");
}

#[test]
fn extract_go_params() {
    let content = "func GetUser(userId string, count int) *User {\n}\n";
    let params = extraction::extract_raw_params(content, "go");
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].param_name, "userId");
    assert_eq!(params[0].raw_type, "string");
}

#[test]
fn extract_kotlin_params() {
    let content = "fun getUser(userId: String, count: Int): User {\n}\n";
    let params = extraction::extract_raw_params(content, "kotlin");
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].param_name, "userId");
    assert_eq!(params[0].raw_type, "String");
}

#[test]
fn extract_swift_params() {
    let content = "func getUser(for userId: String) -> User {\n}\n";
    let params = extraction::extract_raw_params(content, "swift");
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].param_name, "userId");
    assert_eq!(params[0].raw_type, "String");
}

#[test]
fn extract_csharp_params() {
    let content = "public User GetUser(string userId, int count) {\n}\n";
    let params = extraction::extract_raw_params(content, "csharp");
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].param_name, "userId");
    assert_eq!(params[0].raw_type, "string");
}

#[test]
fn extract_ruby_params_untyped() {
    let content = "def get_user(user_id, name)\n  # ...\nend\n";
    let params = extraction::extract_raw_params(content, "ruby");
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].param_name, "user_id");
    assert_eq!(params[0].raw_type, "");
}

#[test]
fn extract_rust_match_cases() {
    let content = "match status {\n    Pending => {},\n    Active => {},\n}\n";
    let groups = matching::extract_raw_match_cases(content, "rust");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].matched_expr, "status");
    assert!(groups[0].cases.contains("Pending"));
    assert!(groups[0].cases.contains("Active"));
    assert!(!groups[0].has_wildcard);
}

#[test]
fn extract_rust_match_wildcard() {
    let content = "match status {\n    Pending => {},\n    _ => {},\n}\n";
    let groups = matching::extract_raw_match_cases(content, "rust");
    assert_eq!(groups.len(), 1);
    assert!(groups[0].has_wildcard);
}

#[test]
fn extract_python_match_cases() {
    let content = "match order.status:\n    case \"Pending\":\n        pass\n    case \"Shipped\":\n        pass\n";
    let groups = matching::extract_raw_match_cases(content, "python");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].matched_expr, "order.status");
    assert!(groups[0].cases.contains("Pending"));
    assert!(groups[0].cases.contains("Shipped"));
}

#[test]
fn extract_js_switch_cases() {
    let content = "switch (status) {\n    case \"Active\":\n        break;\n    case \"Inactive\":\n        break;\n}\n";
    let groups = matching::extract_raw_match_cases(content, "js");
    assert_eq!(groups.len(), 1);
    assert!(groups[0].cases.contains("Active"));
    assert!(groups[0].cases.contains("Inactive"));
}
