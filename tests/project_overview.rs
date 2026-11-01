//! プロジェクトの概要。目的やリポジトリを、作業する主体へタスクの文脈として届ける。

mod common;

use agent_native_kanban::db::after_seconds;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

fn key(task: &serde_json::Value) -> String {
    task["key"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn project_admin_saves_trims_and_clears_the_overview() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let path = format!("/projects/{}", project.id);
    let before = app.get(&admin, &path).await.body;

    let saved = app.patch(&admin, &path, json!({ "description": "  目的\nrepo: org/app\n " })).await;
    assert_eq!(saved.status, StatusCode::OK, "{:?}", saved.body);
    assert_eq!(saved.body["description"], "目的\nrepo: org/app");
    assert_eq!(app.get(&admin, &path).await.body["description"], "目的\nrepo: org/app");
    assert_eq!(saved.body["stages"], before["stages"]);
    assert_eq!(saved.body["name"], before["name"]);

    let events = app.get(&admin, "/events?after=0").await.body;
    let updated = events["events"].as_array().unwrap().iter().rev()
        .find(|event| event["type"] == "project.updated").expect("project.updated event");
    assert_eq!(updated["data"]["description"], "目的\nrepo: org/app");
    assert_eq!(updated["actor"]["id"], admin.id);

    let cleared = app.patch(&admin, &path, json!({ "description": "   " })).await;
    assert_eq!(cleared.status, StatusCode::OK);
    assert_eq!(cleared.body["description"], "");
}

#[tokio::test]
async fn overview_length_is_limited_on_create_and_update() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let path = format!("/projects/{}", project.id);

    let exact = "あ".repeat(2000);
    assert_eq!(app.patch(&admin, &path, json!({ "description": exact })).await.status, StatusCode::OK);

    let over = "あ".repeat(2001);
    let rejected = app.patch(&admin, &path, json!({ "description": over })).await;
    assert_eq!(rejected.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", rejected.body);
    assert_eq!(app.get(&admin, &path).await.body["description"], exact);

    let created = app.post(&admin, "/projects", json!({
        "key": "LONG", "name": "Long", "description": over,
        "stages": [{ "name": "Todo", "requires_work": true, "claim_permission": "claim", "is_final": false }],
    })).await;
    assert_eq!(created.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", created.body);
}

#[tokio::test]
async fn only_project_admin_may_change_the_overview_but_viewers_read_it() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let path = format!("/projects/{}", project.id);
    app.patch(&admin, &path, json!({ "description": "purpose" })).await;
    let worker = app.agent(&admin, "worker", worker_grants(&project)).await;

    assert_eq!(app.get(&worker, &path).await.body["description"], "purpose");
    let refused = app.patch(&worker, &path, json!({ "description": "changed" })).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{:?}", refused.body);
    assert_eq!(app.patch(&ANON, &path, json!({ "description": "changed" })).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(app.get(&admin, &path).await.body["description"], "purpose");
}

#[tokio::test]
async fn task_context_delivers_the_project_overview_within_task_visibility() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    app.patch(&admin, &format!("/projects/{}", project.id), json!({ "description": "purpose\nrepo: org/app" })).await;
    let task = app.task(&admin, &project, &project.backlog, "visible").await;
    let outside = app.task(&admin, &project, &project.backlog, "outside").await;
    let path = format!("/tasks/{}/context", key(&task));

    let worker = app.agent(&admin, "worker", worker_grants(&project)).await;
    let context = app.get(&worker, &path).await;
    assert_eq!(context.status, StatusCode::OK);
    assert_eq!(context.body["project"], json!({
        "id": project.id, "key": "DEV", "name": "Project DEV", "description": "purpose\nrepo: org/app",
    }));

    let stranger_org = app.organization("Other", "admin@other.test").await;
    assert_eq!(app.get(&stranger_org, &path).await.status, StatusCode::NOT_FOUND);

    let delegated = app.post(&worker, "/me/delegations", json!({
        "name": "sub", "expires_at": after_seconds(3600),
        "scopes": [{ "project_id": project.id, "permission": "claim" }],
        "tasks": [key(&task)],
    })).await;
    assert_eq!(delegated.status, StatusCode::CREATED, "{:?}", delegated.body);
    let limited = Client {
        id: worker.id.clone(),
        auth: Auth::Bearer(delegated.body["token"].as_str().unwrap().to_string()),
    };
    assert_eq!(app.get(&limited, &path).await.body["project"]["description"], "purpose\nrepo: org/app");
    assert_eq!(
        app.get(&limited, &format!("/tasks/{}/context", key(&outside))).await.status,
        StatusCode::NOT_FOUND,
    );
}
