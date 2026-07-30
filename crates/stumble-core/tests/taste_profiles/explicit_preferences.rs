use chrono::{TimeZone, Utc};
use stumble_core::*;

use crate::common::{accepted_item, feedback_harness, unattended_harness, TestDataDir};

#[test]
fn user_can_inspect_and_edit_explicit_taste_preferences() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let (user, token) = feedback_harness(&tools);

    let mut update = UpdateTasteProfileRequest::default();
    update.interests = Some(vec!["Rust".into(), "rust".into()]);
    update.blocked_topics = Some(vec!["clickbait".into()]);
    update.blocked_sources = Some(vec!["noise.example".into()]);
    update.recurrence_penalty_days = Some(RecurrencePenaltyDays::new(14).unwrap());
    tools.update_taste_profile(&user, update).unwrap();
    let profile = tools.taste_profile(&user).unwrap();

    assert_eq!(profile.explicit.interests, vec!["Rust"]);
    assert_eq!(profile.explicit.blocked_topics, vec!["clickbait"]);
    assert_eq!(profile.explicit.blocked_sources, vec!["noise.example"]);
    assert_eq!(profile.explicit.recurrence_penalty_days, 14);
    assert!(profile.learned.is_empty());
    assert_eq!(
        tools
            .get_feed_batch(
                &user,
                FeedBatchRequest::new(1).unwrap(),
                Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
            )
            .unwrap()
            .recurrence_penalty_days,
        14
    );
    let exact_override = tools
        .complete_feed_batch(
            &user,
            tools
                .get_feed_batch(
                    &user,
                    FeedBatchRequest::new(1).unwrap(),
                    Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
                )
                .unwrap()
                .id,
            Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 1).unwrap(),
        )
        .unwrap();
    assert!(exact_override.completed_at.is_some());
    let mut override_request = FeedBatchRequest::new(1).unwrap();
    override_request.recurrence_penalty_days = Some(RecurrencePenaltyDays::new(30).unwrap());
    assert_eq!(
        tools
            .get_feed_batch(
                &user,
                override_request,
                Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 2).unwrap(),
            )
            .unwrap()
            .recurrence_penalty_days,
        30
    );

    drop(tools);
    let reopened = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    assert_eq!(
        reopened
            .taste_profile(
                &reopened
                    .authenticate_token(token.expose())
                    .unwrap()
                    .unwrap()
            )
            .unwrap(),
        profile
    );
}

#[test]
fn legacy_preferences_can_be_edited_after_sqlite_restart() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let (user, token) = feedback_harness(&tools);
    let user_id = user.user_id.unwrap();
    drop(tools);

    let database_path = data_dir.0.join("stumble.sqlite3");
    let connection = rusqlite::Connection::open(&database_path).unwrap();
    let record_key =
        serde_json::to_string(&[serde_json::json!(user_id), serde_json::Value::Null]).unwrap();
    let value_json: String = connection
        .query_row(
            "SELECT value_json FROM stumble_store_records
             WHERE collection = 'user_preferences' AND record_key = ?1",
            [&record_key],
            |row| row.get(0),
        )
        .unwrap();
    let mut legacy: serde_json::Value = serde_json::from_str(&value_json).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("blocked_source_affinities");
    connection
        .execute(
            "UPDATE stumble_store_records SET value_json = ?1
             WHERE collection = 'user_preferences' AND record_key = ?2",
            rusqlite::params![serde_json::to_string(&legacy).unwrap(), record_key],
        )
        .unwrap();
    drop(connection);

    let reopened = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let user = reopened
        .authenticate_token(token.expose())
        .unwrap()
        .unwrap();
    let mut update = UpdateTasteProfileRequest::default();
    update.interests = Some(vec!["consciousness".into()]);
    let profile = reopened.update_taste_profile(&user, update).unwrap();

    assert_eq!(profile.explicit.interests, vec!["consciousness"]);
    let connection = rusqlite::Connection::open(database_path).unwrap();
    let migrated: String = connection
        .query_row(
            "SELECT value_json FROM stumble_store_records
             WHERE collection = 'user_preferences' AND record_key = ?1",
            [&record_key],
            |row| row.get(0),
        )
        .unwrap();
    assert!(migrated.contains("\"blocked_source_affinities\""));
}

#[test]
fn legacy_preferences_are_persisted_canonically_after_snapshot_load() {
    let data_dir = TestDataDir::new();
    let snapshot_path = data_dir.0.join("store.json");
    let tools = AgentTools::new(seed_store());
    let (user, _) = feedback_harness(&tools);
    let mut update = UpdateTasteProfileRequest::default();
    update.interests = Some(vec!["consciousness".into()]);
    tools.update_taste_profile(&user, update).unwrap();
    save_store_snapshot(&tools.store().read().unwrap(), &snapshot_path).unwrap();

    let mut snapshot: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&snapshot_path).unwrap()).unwrap();
    snapshot["user_preferences"][0]
        .as_object_mut()
        .unwrap()
        .remove("blocked_source_affinities");
    std::fs::write(
        &snapshot_path,
        serde_json::to_vec_pretty(&snapshot).unwrap(),
    )
    .unwrap();

    load_store_snapshot(&snapshot_path).unwrap();

    let migrated: serde_json::Value =
        serde_json::from_slice(&std::fs::read(snapshot_path).unwrap()).unwrap();
    assert!(migrated["user_preferences"][0]["blocked_source_affinities"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn taste_profile_requires_unscoped_feedback_authority_and_honors_revocation() {
    let tools = AgentTools::new(seed_store());
    let (pod, _) = accepted_item(
        &tools,
        "taste-authority",
        401,
        "authority.example",
        vec!["authority".into()],
    );
    let scoped = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "scoped profile".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::Feedback],
                pod_ids: Some(vec![pod.id]),
            },
        )
        .unwrap();
    let scoped = tools
        .authenticate_token(scoped.token.expose())
        .unwrap()
        .unwrap();

    assert!(tools.taste_profile(&scoped).is_err());
    assert!(tools
        .update_taste_profile(&scoped, UpdateTasteProfileRequest::default())
        .is_err());
    assert!(tools
        .reset_learned_taste(&scoped, ResetLearnedTasteRequest::all())
        .is_err());

    let unattended = unattended_harness(
        &tools,
        "unattended profile",
        vec![HarnessCapability::Feedback],
    );
    assert!(matches!(
        tools.taste_profile(&unattended),
        Err(AgentToolsError::Forbidden { reason }) if reason.contains("interactive")
    ));
    assert!(tools
        .update_taste_profile(&unattended, UpdateTasteProfileRequest::default())
        .is_err());
    assert!(tools
        .reset_learned_taste(&unattended, ResetLearnedTasteRequest::all())
        .is_err());

    let (unscoped, _) = feedback_harness(&tools);
    assert!(tools.taste_profile(&unscoped).is_ok());
    tools
        .revoke_agent_harness(
            &tools.default_auth_context().unwrap(),
            unscoped.harness_id.unwrap(),
        )
        .unwrap();
    assert!(tools.taste_profile(&unscoped).is_err());
    assert!(tools
        .update_taste_profile(&unscoped, UpdateTasteProfileRequest::default())
        .is_err());
    assert!(tools
        .reset_learned_taste(&unscoped, ResetLearnedTasteRequest::all())
        .is_err());
}
