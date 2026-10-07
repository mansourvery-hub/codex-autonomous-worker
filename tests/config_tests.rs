use autopilot::config::AppConfig;
use std::path::PathBuf;

#[test]
fn test_default_config() {
    let config = AppConfig::default();
    assert_eq!(config.base_dir, PathBuf::from("/home/ubuntu/srv-codex"));
    assert_eq!(config.tasks_dir(), PathBuf::from("/home/ubuntu/srv-codex/tasks"));
    assert_eq!(config.worktrees_dir(), PathBuf::from("/home/ubuntu/srv-codex/worktrees"));
    assert_eq!(config.current_file(), PathBuf::from("/home/ubuntu/srv-codex/state/current.json"));
    assert_eq!(config.task_timeout_seconds, 7200);
    assert_eq!(config.idle_timeout_seconds, 30);
}

#[test]
fn test_config_json_roundtrip() {
    let config = AppConfig::default();
    let json_str = serde_json::to_string(&config).expect("Serialize to JSON");
    let deserialized: AppConfig = serde_json::from_str(&json_str).expect("Deserialize from JSON");
    assert_eq!(config.base_dir, deserialized.base_dir);
    assert_eq!(config.default_model, deserialized.default_model);
}
