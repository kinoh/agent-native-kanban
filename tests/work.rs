//! 作業の排他・作業権・取得可能性の規則。

mod common;

use std::collections::HashSet;

use axum::http::StatusCode;
use common::*;
use agent_native_kanban::db::after_seconds;
use serde_json::json;

#[tokio::test]
async fn concurrent_claims_produce_exactly_one_holder() {
    let app = std::sync::Arc::new(App::new());
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let task = app.task(&admin, &project, &project.implement, "Race").await;
    let key = task["key"].as_str().unwrap().to_string();

    let mut agents = Vec::new();
    for index in 0..16 {
        agents.push(app.agent(&admin, &format!("Agent {index}"), worker_grants(&project)).await);
    }
    let mut handles = Vec::new();
    for agent in agents {
        let app = app.clone();
        let path = format!("/tasks/{key}/work/claim");
        handles.push(tokio::spawn(async move {
            let reply = app.post(&agent, &path, json!({})).await;
            (reply.status, reply.code().to_string())
        }));
    }
    let mut winners = 0;
    for handle in handles {
        let (status, code) = handle.await.unwrap();
        if status == StatusCode::OK {
            winners += 1;
        } else {
            assert_eq!(status, StatusCode::CONFLICT);
            assert_eq!(code, "not_claimable");
        }
    }
    assert_eq!(winners, 1);
}

#[tokio::test]
async fn replaced_holders_cannot_use_their_old_work_right() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let a = app.agent(&admin, "A", worker_grants(&project)).await;
    let b = app.agent(&admin, "B", worker_grants(&project)).await;
    let task = app.task(&admin, &project, &project.implement, "X").await;
    let key = task["key"].as_str().unwrap();
    let old = app.claim(&a, &task).await;

    // 管理者が占有を B へ移す
    let moved = app
        .post(&admin, &format!("/tasks/{key}/work/reassign"), json!({ "to": b.id, "reason": "A stalled" }))
        .await;
    assert_eq!(moved.status, StatusCode::OK, "{:?}", moved.body);

    for operation in ["start", "release"] {
        let reply = app
            .post(&a, &format!("/tasks/{key}/work/{operation}"), json!({ "claim_id": old }))
            .await;
        assert_eq!(reply.code(), "stale_claim", "{operation}");
    }
    let complete = app
        .post(&a, &format!("/tasks/{key}/work/complete"), json!({ "claim_id": old, "summary": "late" }))
        .await;
    assert_eq!(complete.code(), "stale_claim");

    // A が再度取得しても、以前の作業権は復活しない
    let current = app.view(&b, &task).await["work"]["claim_id"].as_str().unwrap().to_string();
    app.post(&b, &format!("/tasks/{key}/work/release"), json!({ "claim_id": current })).await;
    let fresh = app.claim(&a, &task).await;
    assert_ne!(fresh, old);
    let reply =
        app.post(&a, &format!("/tasks/{key}/work/start"), json!({ "claim_id": old })).await;
    assert_eq!(reply.code(), "stale_claim");

    // 占有者でない主体が現在の作業権を知っていても使えない
    let reply =
        app.post(&b, &format!("/tasks/{key}/work/start"), json!({ "claim_id": fresh })).await;
    assert_eq!(reply.code(), "stale_claim");
}

#[tokio::test]
async fn release_makes_work_claimable_without_touching_stage_or_assignment() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let a = app.agent(&admin, "A", worker_grants(&project)).await;
    let task = app.task(&admin, &project, &project.implement, "X").await;
    let key = task["key"].as_str().unwrap();
    let assigned = app
        .patch(&admin, &format!("/tasks/{key}"), json!({ "version": 1, "responsible_id": admin.id }))
        .await;
    assert_eq!(assigned.body["version"], 2);

    let claim_id = app.claim(&a, &task).await;
    let claimed = app.view(&a, &task).await;
    assert_eq!(claimed["responsible"]["id"], admin.id.as_str());
    assert!(claimed["assignee"].is_null());
    assert_eq!(claimed["version"], 2);

    let released = app
        .post(&a, &format!("/tasks/{key}/work/release"), json!({ "claim_id": claim_id, "note": "残課題あり" }))
        .await;
    assert_eq!(released.body["work"]["claimable"], true);
    assert_eq!(released.body["stage"]["id"], project.implement.as_str());
    assert_eq!(released.body["responsible"]["id"], admin.id.as_str());
    let context = app.get(&a, &format!("/tasks/{key}/context")).await;
    assert_eq!(context.body["comments"][0]["body"], "残課題あり");
}

#[tokio::test]
async fn completed_work_needs_an_explicit_reopen() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let a = app.agent(&admin, "A", worker_grants(&project)).await;
    let task = app.task(&admin, &project, &project.implement, "X").await;
    let key = task["key"].as_str().unwrap();
    let claim_id = app.claim(&a, &task).await;
    app.post(&a, &format!("/tasks/{key}/work/complete"), json!({ "claim_id": claim_id, "summary": "ok" })).await;

    let denied = app.post(&a, &format!("/tasks/{key}/work/claim"), json!({})).await;
    assert_eq!(denied.reason_codes(), ["work_done"]);

    let refused = app.post(&a, &format!("/tasks/{key}/work/reopen"), json!({ "reason": "bug" })).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    let reopened =
        app.post(&admin, &format!("/tasks/{key}/work/reopen"), json!({ "reason": "bug" })).await;
    assert_eq!(reopened.body["work"]["claimable"], true);
    app.claim(&a, &task).await;
}

#[tokio::test]
async fn moving_stage_starts_fresh_work_for_that_stage() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let a = app.agent(&admin, "A", worker_grants(&project)).await;
    let task = app.task(&admin, &project, &project.backlog, "X").await;
    let key = task["key"].as_str().unwrap();

    let denied = app.post(&a, &format!("/tasks/{key}/work/claim"), json!({})).await;
    assert_eq!(denied.reason_codes(), ["no_work_required"]);

    assert_eq!(app.move_to(&admin, &task, &project.implement).await.status, StatusCode::OK);
    let claim_id = app.claim(&a, &task).await;
    app.post(&a, &format!("/tasks/{key}/work/complete"), json!({ "claim_id": claim_id, "summary": "ok" })).await;
    let moved = app.move_to(&admin, &task, &project.review).await;
    assert_eq!(moved.body["work"]["readiness"], "ready");
}

#[tokio::test]
async fn dependencies_block_until_the_blocker_reaches_a_final_stage() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let blocker = app.task(&admin, &project, &project.implement, "Schema").await;
    let task = app.task(&admin, &project, &project.implement, "Feature").await;
    let key = task["key"].as_str().unwrap();
    let blocker_key = blocker["key"].as_str().unwrap();

    let added = app
        .post(&admin, &format!("/tasks/{key}/dependencies"), json!({
            "depends_on": blocker_key, "reason": "スキーマが先に必要",
        }))
        .await;
    assert_eq!(added.status, StatusCode::CREATED, "{:?}", added.body);

    let seen = app.view(&admin, &task).await;
    assert_eq!(seen["work"]["readiness"], "waiting");
    assert_eq!(seen["work"]["claimable"], false);
    assert_eq!(seen["work"]["blockers"][0]["reason"], "スキーマが先に必要");

    let cursor = app.get(&admin, "/events?after=latest").await.body["cursor"].as_i64().unwrap();
    assert_eq!(app.move_to(&admin, &blocker, &project.done).await.status, StatusCode::OK);
    assert_eq!(app.view(&admin, &task).await["work"]["claimable"], true);

    // 新しく取得可能になった仕事を購読側で検出できる
    let events = app.get(&admin, &format!("/events?after={cursor}")).await;
    assert!(events.body["events"].as_array().unwrap().iter().any(|event| {
        event["type"] == "work.available" && event["data"]["key"] == key
    }));

    let cycle = app
        .post(&admin, &format!("/tasks/{blocker_key}/dependencies"), json!({
            "depends_on": key, "reason": "循環",
        }))
        .await;
    assert_eq!(cycle.code(), "dependency_cycle");
}

#[tokio::test]
async fn dependencies_cannot_cross_project_boundaries() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let visible = app.project(&admin, "OPEN").await;
    let hidden = app.project(&admin, "SEC").await;
    let agent = app.agent(&admin, "A", worker_grants(&visible)).await;
    let secret = app.task(&admin, &hidden, &hidden.implement, "Secret").await;
    let task = app.task(&admin, &visible, &visible.implement, "Public").await;
    let rejected = app.post(&admin, &format!("/tasks/{}/dependencies", task["key"].as_str().unwrap()), json!({
        "depends_on": secret["key"], "reason": "secret reason",
    }))
    .await;
    assert_eq!(rejected.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(rejected.code(), "relation_project_mismatch");

    let seen = app.view(&agent, &task).await;
    assert_eq!(seen["work"]["claimable"], true);
    assert_eq!(seen["work"]["hidden_blockers"], 0);
    assert_eq!(seen["work"]["blockers"].as_array().unwrap().len(), 0);
    assert!(!seen.to_string().contains("Secret"));
}

#[tokio::test]
async fn hold_records_why_work_cannot_be_started() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let a = app.agent(&admin, "A", worker_grants(&project)).await;
    let task = app.task(&admin, &project, &project.implement, "X").await;
    let key = task["key"].as_str().unwrap();

    let held = app
        .post(&admin, &format!("/tasks/{key}/work/hold"), json!({ "reason": "ベンダー回答待ち" }))
        .await;
    assert_eq!(held.body["work"]["readiness"], "waiting");
    let denied = app.post(&a, &format!("/tasks/{key}/work/claim"), json!({})).await;
    assert_eq!(denied.reason_codes(), ["on_hold"]);
    assert!(denied.body["error"]["message"].as_str().unwrap().contains("ベンダー回答待ち"));

    app.post(&admin, &format!("/tasks/{key}/work/unhold"), json!({})).await;
    app.claim(&a, &task).await;
}

#[tokio::test]
async fn disabled_holder_keeps_the_work_until_an_admin_revokes_it() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let a = app.agent(&admin, "A", worker_grants(&project)).await;
    let b = app.agent(&admin, "B", worker_grants(&project)).await;
    let task = app.task(&admin, &project, &project.implement, "X").await;
    let key = task["key"].as_str().unwrap();
    app.claim(&a, &task).await;

    app.patch(&admin, &format!("/principals/{}", a.id), json!({ "status": "disabled" })).await;
    let denied = app.post(&b, &format!("/tasks/{key}/work/claim"), json!({})).await;
    assert_eq!(denied.reason_codes(), ["occupied_not_started", "occupant_inactive"]);

    let forbidden = app.post(&b, &format!("/tasks/{key}/work/revoke"), json!({ "reason": "stuck" })).await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    let revoked =
        app.post(&admin, &format!("/tasks/{key}/work/revoke"), json!({ "reason": "stuck" })).await;
    assert_eq!(revoked.status, StatusCode::OK, "{:?}", revoked.body);
    app.claim(&b, &task).await;

    let history = app.get(&admin, &format!("/tasks/{key}/history")).await;
    let revocation = history.body["history"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["type"] == "work.revoked")
        .unwrap()
        .clone();
    assert_eq!(revocation["data"]["reason"], "stuck");
    assert_eq!(revocation["actor"]["id"], admin.id.as_str());
}

#[tokio::test]
async fn principal_work_lists_visible_claims_without_leaking_hidden_or_scoped_tasks() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let visible_project = app.project(&admin, "OPEN").await;
    let hidden_project = app.project(&admin, "SECRET").await;
    let holder = app.agent(
        &admin,
        "Holder",
        json!([
            { "project_id": visible_project.id, "permission": "claim" },
            { "project_id": hidden_project.id, "permission": "claim" },
        ]),
    ).await;
    let viewer = app.agent(&admin, "Viewer", worker_grants(&visible_project)).await;
    let visible = app.task(&admin, &visible_project, &visible_project.implement, "Visible").await;
    let hidden = app.task(&admin, &hidden_project, &hidden_project.implement, "Hidden").await;
    let visible_claim = app.claim(&holder, &visible).await;
    let hidden_claim = app.claim(&holder, &hidden).await;
    assert_eq!(
        app.post(
            &holder,
            &format!("/tasks/{}/work/start", visible["key"].as_str().unwrap()),
            json!({ "claim_id": visible_claim }),
        ).await.status,
        StatusCode::OK,
    );
    assert_eq!(
        app.post(
            &holder,
            &format!("/tasks/{}/work/start", hidden["key"].as_str().unwrap()),
            json!({ "claim_id": hidden_claim }),
        ).await.status,
        StatusCode::OK,
    );
    assert_eq!(
        app.post(
            &holder,
            &format!("/tasks/{}/work/pause", hidden["key"].as_str().unwrap()),
            json!({ "claim_id": hidden_claim }),
        ).await.status,
        StatusCode::OK,
    );

    assert_eq!(app.get(&ANON, "/principals/work").await.status, StatusCode::UNAUTHORIZED);
    let visible_only = app.get(&viewer, "/principals/work").await;
    assert_eq!(visible_only.status, StatusCode::OK);
    let holder_work = visible_only.body["principals"].as_array().unwrap().iter()
        .find(|principal| principal["id"] == holder.id).unwrap();
    assert_eq!(holder_work["work"].as_array().unwrap().len(), 1);
    assert_eq!(holder_work["work"][0]["key"], visible["key"]);
    assert_eq!(holder_work["work"][0]["activity"], "working");
    assert!(!visible_only.body.to_string().contains("Hidden"));
    assert!(!visible_only.body.to_string().contains(hidden["key"].as_str().unwrap()));

    let delegated = app.post(&holder, "/me/delegations", json!({
        "name": "one task",
        "expires_at": after_seconds(3600),
        "scopes": [{ "project_id": visible_project.id, "permission": "claim" }],
        "tasks": [visible["key"]],
    })).await;
    assert_eq!(delegated.status, StatusCode::CREATED, "{:?}", delegated.body);
    let scoped = Client {
        id: holder.id.clone(),
        auth: Auth::Bearer(delegated.body["token"].as_str().unwrap().to_string()),
    };
    let scoped_work = app.get(&scoped, "/principals/work").await;
    assert_eq!(scoped_work.status, StatusCode::OK);
    assert_eq!(
        scoped_work.body["principals"].as_array().unwrap().iter()
            .find(|principal| principal["id"] == holder.id).unwrap()["work"],
        json!([{
            "key": visible["key"],
            "title": "Visible",
            "project": { "id": visible_project.id, "key": "OPEN", "name": "Project OPEN" },
            "activity": "working",
            "acquired_at": scoped_work.body["principals"].as_array().unwrap().iter()
                .find(|principal| principal["id"] == holder.id).unwrap()["work"][0]["acquired_at"],
        }]),
    );

    assert_eq!(app.delete(&admin, &format!("/principals/{}", holder.id)).await.status, StatusCode::NO_CONTENT);
    let after_deletion = app.get(&admin, "/principals/work").await;
    let deleted_holder = after_deletion.body["principals"].as_array().unwrap().iter()
        .find(|principal| principal["id"] == holder.id).unwrap();
    assert_eq!(deleted_holder["status"], "deleted");
    assert_eq!(deleted_holder["work"].as_array().unwrap().len(), 2);
    assert_eq!(app.view(&admin, &visible).await["work"]["occupant"]["id"], holder.id);
}

#[tokio::test]
async fn stale_edits_are_rejected_as_conflicts() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let task = app.task(&admin, &project, &project.implement, "X").await;
    let key = task["key"].as_str().unwrap();

    app.patch(&admin, &format!("/tasks/{key}"), json!({ "version": 1, "title": "first" })).await;
    let stale = app.patch(&admin, &format!("/tasks/{key}"), json!({ "version": 1, "title": "second" })).await;
    assert_eq!(stale.status, StatusCode::CONFLICT);
    assert_eq!(stale.code(), "version_conflict");
    assert_eq!(stale.body["error"]["details"]["current_version"], 2);
    assert_eq!(app.view(&admin, &task).await["title"], "first");
}

#[tokio::test]
async fn discovery_filters_and_sorts() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let urgent = app
        .post(&admin, "/tasks", json!({
            "project_id": project.id, "stage_id": project.implement, "title": "urgent one",
            "priority": "urgent", "due_at": "2030-01-01T00:00:00+09:00",
        }))
        .await;
    let low = app
        .post(&admin, "/tasks", json!({
            "project_id": project.id, "stage_id": project.implement, "title": "100% low",
            "priority": "low", "due_at": "2031-01-01T00:00:00Z",
        }))
        .await;
    let keys = |reply: &Reply| -> Vec<String> {
        reply.body["tasks"].as_array().unwrap().iter()
            .map(|task| task["key"].as_str().unwrap().to_string())
            .collect()
    };
    let urgent_key = urgent.body["key"].as_str().unwrap().to_string();
    let low_key = low.body["key"].as_str().unwrap().to_string();

    let sorted = app.get(&admin, "/tasks?sort=priority").await;
    assert_eq!(keys(&sorted), [urgent_key.clone(), low_key.clone()]);
    let by_due = app.get(&admin, "/tasks?sort=due&order=desc").await;
    assert_eq!(keys(&by_due), [low_key.clone(), urgent_key.clone()]);
    let filtered = app.get(&admin, "/tasks?priority=low").await;
    assert_eq!(keys(&filtered), [low_key.clone()]);
    let due = app.get(&admin, "/tasks?due_before=2030-06-01T00:00:00Z").await;
    assert_eq!(keys(&due), [urgent_key.clone()]);
    // LIKE のワイルドカードは文字として扱う
    let search = app.get(&admin, "/tasks?q=100%25").await;
    assert_eq!(keys(&search), [low_key.clone()]);
    let page = app.get(&admin, "/tasks?limit=1").await;
    assert_eq!(page.body["next_cursor"], 1);
    let next = app.get(&admin, "/tasks?limit=1&cursor=1").await;
    assert!(next.body["next_cursor"].is_null());
    let seen: HashSet<_> = keys(&page).into_iter().chain(keys(&next)).collect();
    assert_eq!(seen.len(), 2);
    assert_eq!(app.get(&admin, "/tasks?sort=nope").await.status, StatusCode::UNPROCESSABLE_ENTITY);
}
