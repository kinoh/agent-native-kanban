//! 共有リソースの確保、排他、権限、監査イベント。

mod common;

use std::sync::Arc;

use agent_native_kanban::db::after_seconds;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

async fn create(
    app: &App,
    admin: &Client,
    project_id: Option<&str>,
    name: &str,
    capacity: i64,
) -> serde_json::Value {
    let reply = app.post(admin, "/resources", json!({
        "project_id": project_id, "name": name, "description": "shared test environment", "capacity": capacity,
    })).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    reply.body
}

#[tokio::test]
async fn resources_enforce_capacity_concurrently_and_keep_holds_explicit() {
    let app = Arc::new(App::new());
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let resource = create(&app, &admin, Some(&project.id), "CI", 1).await;
    let resource_id = resource["id"].as_str().unwrap().to_string();
    let mut agents = Vec::new();
    for index in 0..12 {
        agents.push(
            app.agent(&admin, &format!("Agent {index}"), worker_grants(&project))
                .await,
        );
    }
    let mut joins = Vec::new();
    for agent in agents {
        let app = app.clone();
        let path = format!("/resources/{resource_id}/acquire");
        joins.push(tokio::spawn(async move {
            app.post(&agent, &path, json!({ "reason": "test run" }))
                .await
        }));
    }
    let mut hold = None;
    for join in joins {
        let reply = join.await.unwrap();
        if reply.status == StatusCode::OK {
            assert!(
                hold.replace(reply.body["hold_id"].as_str().unwrap().to_string())
                    .is_none()
            );
        } else {
            assert_eq!(reply.status, StatusCode::CONFLICT, "{:?}", reply.body);
            assert_eq!(reply.code(), "resource_busy");
            assert_eq!(
                reply.body["error"]["details"]["holders"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
        }
    }
    let hold = hold.expect("one concurrent caller acquires the sole slot");
    let view = app.get(&admin, &format!("/resources/{resource_id}")).await;
    assert_eq!(view.body["holds"].as_array().unwrap().len(), 1);

    // capacity 1 also refuses the same principal's second acquire, then release enables another caller.
    let busy = app
        .post(
            &admin,
            &format!("/resources/{resource_id}/acquire"),
            json!({ "reason": "another" }),
        )
        .await;
    assert_eq!(busy.code(), "resource_busy");
    let holder_id = view.body["holds"][0]["holder"]["id"].as_str().unwrap();
    // The winning client is not retained above. Release through the recorded holder is intentionally unavailable to admin,
    // so administrator revocation is the supported recovery path.
    let revoke = app
        .post(
            &admin,
            &format!("/resources/{resource_id}/revoke"),
            json!({ "hold_id": hold, "reason": "test cleanup" }),
        )
        .await;
    assert_eq!(revoke.status, StatusCode::OK, "{:?}", revoke.body);
    assert_eq!(revoke.body["holds"].as_array().unwrap().len(), 0);
    assert!(!holder_id.is_empty());
    let again = app
        .post(
            &admin,
            &format!("/resources/{resource_id}/acquire"),
            json!({ "reason": "admin use" }),
        )
        .await;
    assert_eq!(again.status, StatusCode::OK);
}

#[tokio::test]
async fn holders_release_only_their_own_hold_and_admin_can_revoke_inactive_holder() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let a = app.agent(&admin, "A", worker_grants(&project)).await;
    let b = app.agent(&admin, "B", worker_grants(&project)).await;
    let resource = create(&app, &admin, Some(&project.id), "device", 2).await;
    let resource_id = resource["id"].as_str().unwrap().to_string();
    let acquired = app
        .post(
            &a,
            &format!("/resources/{resource_id}/acquire"),
            json!({ "reason": "diagnosis" }),
        )
        .await;
    let hold = acquired.body["hold_id"].as_str().unwrap().to_string();
    assert_eq!(
        app.post(
            &b,
            &format!("/resources/{resource_id}/release"),
            json!({ "hold_id": hold })
        )
        .await
        .code(),
        "hold_not_owned"
    );
    assert_eq!(
        app.post(
            &a,
            &format!("/resources/{resource_id}/release"),
            json!({ "hold_id": hold })
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        app.post(
            &a,
            &format!("/resources/{resource_id}/release"),
            json!({ "hold_id": hold })
        )
        .await
        .code(),
        "stale_hold"
    );

    let acquired = app
        .post(
            &a,
            &format!("/resources/{resource_id}/acquire"),
            json!({ "reason": "long job" }),
        )
        .await;
    let hold = acquired.body["hold_id"].as_str().unwrap().to_string();
    assert_eq!(
        app.patch(
            &admin,
            &format!("/principals/{}", a.id),
            json!({ "status": "disabled" })
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        app.delete(&admin, &format!("/principals/{}", a.id))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    let still_held = app.get(&admin, &format!("/resources/{resource_id}")).await;
    assert_eq!(still_held.body["holds"][0]["holder"]["status"], "deleted");
    assert_eq!(
        app.post(
            &admin,
            &format!("/resources/{resource_id}/revoke"),
            json!({ "hold_id": hold, "reason": "owner is inactive" })
        )
        .await
        .status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn resource_administration_keeps_scope_name_capacity_and_active_holds_consistent() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let worker = app.agent(&admin, "Worker", worker_grants(&project)).await;
    let resource = create(&app, &admin, Some(&project.id), "runner", 2).await;
    let resource_id = resource["id"].as_str().unwrap();
    assert_eq!(
        app.post(
            &admin,
            "/resources",
            json!({ "project_id": project.id, "name": "runner", "capacity": 1 })
        )
        .await
        .code(),
        "duplicate_resource_name"
    );
    let hold = app
        .post(
            &worker,
            &format!("/resources/{resource_id}/acquire"),
            json!({ "reason": "test one" }),
        )
        .await
        .body["hold_id"]
        .as_str()
        .unwrap()
        .to_string();
    let second_hold = app
        .post(
            &worker,
            &format!("/resources/{resource_id}/acquire"),
            json!({ "reason": "test two" }),
        )
        .await
        .body["hold_id"]
        .as_str()
        .unwrap()
        .to_string();
    let too_small = app
        .patch(
            &admin,
            &format!("/resources/{resource_id}"),
            json!({ "capacity": 0 }),
        )
        .await;
    assert_eq!(too_small.status, StatusCode::UNPROCESSABLE_ENTITY);
    let below_holds = app
        .patch(
            &admin,
            &format!("/resources/{resource_id}"),
            json!({ "capacity": 1 }),
        )
        .await;
    assert_eq!(below_holds.code(), "resource_capacity_in_use");
    assert_eq!(
        app.delete(&admin, &format!("/resources/{resource_id}"))
            .await
            .code(),
        "resource_held"
    );
    assert_eq!(
        app.post(
            &worker,
            &format!("/resources/{resource_id}/release"),
            json!({ "hold_id": hold })
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        app.post(
            &worker,
            &format!("/resources/{resource_id}/release"),
            json!({ "hold_id": second_hold })
        )
        .await
        .status,
        StatusCode::OK
    );
    let updated = app
        .patch(
            &admin,
            &format!("/resources/{resource_id}"),
            json!({ "name": "runner two", "capacity": 1 }),
        )
        .await;
    assert_eq!(updated.status, StatusCode::OK);
    assert_eq!(updated.body["project_id"], project.id);
    assert_eq!(
        app.delete(&admin, &format!("/resources/{resource_id}"))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn resource_visibility_events_and_delegation_boundaries_are_preserved() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let hidden = app.project(&admin, "SECRET").await;
    let worker = app.agent(&admin, "Worker", worker_grants(&project)).await;
    let public = create(&app, &admin, None, "org mutex", 1).await;
    let private = create(&app, &admin, Some(&hidden.id), "secret lab", 1).await;
    let public_id = public["id"].as_str().unwrap();
    let private_id = private["id"].as_str().unwrap();
    assert_eq!(
        app.get(&worker, "/resources").await.body["resources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        app.get(&worker, &format!("/resources/{private_id}"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.get(&worker, "/events?after=0")
            .await
            .body
            .to_string()
            .contains("resource.created"),
        true
    );

    let task = app
        .task(&admin, &project, &project.implement, "limited")
        .await;
    let root = app
        .post(
            &admin,
            &format!("/principals/{}/tokens", worker.id),
            json!({ "name": "root" }),
        )
        .await;
    let root = Client {
        id: worker.id.clone(),
        auth: Auth::Bearer(root.body["token"].as_str().unwrap().to_string()),
    };
    let delegated = app.post(&root, "/me/delegations", json!({
        "name": "limited", "expires_at": after_seconds(3600),
        "scopes": [{ "project_id": project.id, "permission": "claim" }], "tasks": [task["key"]],
    })).await;
    assert_eq!(
        delegated.status,
        StatusCode::CREATED,
        "{:?}",
        delegated.body
    );
    let limited = Client {
        id: worker.id.clone(),
        auth: Auth::Bearer(delegated.body["token"].as_str().unwrap().to_string()),
    };
    assert_eq!(
        app.get(&limited, "/resources").await.body["resources"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        app.get(&limited, &format!("/resources/{public_id}"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.post(
            &limited,
            &format!("/resources/{public_id}/acquire"),
            json!({ "reason": "no" })
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    let other = app.organization("Other", "other@other.test").await;
    assert_eq!(
        app.get(&other, "/resources").await.body["resources"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        app.get(&other, &format!("/resources/{public_id}"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}
