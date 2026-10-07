use autopilot::campaign::{format_duration_ago, CampaignStatus, queue_campaign, load_all_campaigns};
use autopilot::config::AppConfig;
use std::time::{Duration, SystemTime};

#[test]
fn test_format_duration_ago() {
    let now = SystemTime::now();
    assert_eq!(format_duration_ago(now), "0s ago");

    let five_mins_ago = now - Duration::from_secs(300);
    assert_eq!(format_duration_ago(five_mins_ago), "5m ago");

    let two_hours_ago = now - Duration::from_secs(7200 + 180);
    assert_eq!(format_duration_ago(two_hours_ago), "2h 03m ago");
}

#[test]
fn test_campaign_status_str() {
    assert_eq!(CampaignStatus::Pending.as_str(), "pending");
    assert_eq!(CampaignStatus::Claimed.as_str(), "claimed");
    assert_eq!(CampaignStatus::Running.as_str(), "running");
    assert_eq!(CampaignStatus::Done.as_str(), "done");
    assert_eq!(CampaignStatus::Failed.as_str(), "failed");
}

#[test]
fn test_queue_campaign_in_temp_dir() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut config = AppConfig::default();
    config.base_dir = temp_dir.path().to_path_buf();

    let id = queue_campaign(&config, "test-repo", "Test objective", "continuous", 15, None, Some("codex")).expect("Queue campaign");
    assert_eq!(id, "001");

    let campaigns = load_all_campaigns(&config);
    assert_eq!(campaigns.len(), 1);
    assert_eq!(campaigns[0].id, "001");
    assert_eq!(campaigns[0].repo, "test-repo");
    assert_eq!(campaigns[0].prompt, "Test objective");
    assert_eq!(campaigns[0].agent, "codex");
    assert_eq!(campaigns[0].status, CampaignStatus::Pending);

    // Queue another campaign with opencode
    let id2 = queue_campaign(&config, "test-repo", "Opencode test", "single", 1, None, Some("opencode")).expect("Queue opencode campaign");
    assert_eq!(id2, "002");

    let campaigns2 = load_all_campaigns(&config);
    assert_eq!(campaigns2.len(), 2);
    let opencode_c = campaigns2.iter().find(|c| c.id == "002").unwrap();
    assert_eq!(opencode_c.agent, "opencode");
    assert_eq!(opencode_c.mode, "single");
}
