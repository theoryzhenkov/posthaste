use super::*;

#[test]
fn idle_notification_is_an_observation_hint_without_changed_ids() {
    let notification = imap_idle_notification(
        AccountId::from("primary"),
        "2026-04-29T00:00:00Z".to_string(),
    );

    assert!(notification.changed.is_empty());
    assert_eq!(notification.checkpoint, None);
}
