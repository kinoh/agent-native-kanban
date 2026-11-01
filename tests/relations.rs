mod common;

use agent_native_kanban::events::{EventInput, emit};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::*;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn all_relations_stay_in_the_project_and_failed_changes_are_atomic() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let other = app.project(&admin, "OTHER").await;
    let task = app.task(&admin, &project, &project.implement, "X").await;
    let peer = app.task(&admin, &project, &project.implement, "Peer").await;
    let foreign = app.task(&admin, &other, &other.implement, "Foreign").await;
    let path = format!("/tasks/{}", task["key"].as_str().unwrap());
    for (endpoint, body) in [
        ("dependencies", json!({"depends_on": foreign["key"], "reason": "wait"})),
        ("links", json!({"related": foreign["id"]})),
    ] {
        let reply = app.post(&admin, &format!("{path}/{endpoint}"), body).await;
        assert_eq!(reply.code(), "relation_project_mismatch");
    }
    let reply = app.patch(&admin, &path, json!({
        "version": 1, "title": "must roll back", "parent": foreign["key"]
    })).await;
    assert_eq!(reply.code(), "relation_project_mismatch");
    let reply = app.post(&admin, "/tasks", json!({
        "project_id": project.id, "title": "must not exist", "parent": foreign["id"]
    })).await;
    assert_eq!(reply.code(), "relation_project_mismatch");
    let context = app.get(&admin, &format!("{path}/context")).await.body;
    assert_eq!(context["task"]["title"], "X");
    assert_eq!(context["task"]["version"], 1);
    assert!(context["parent"].is_null());
    assert_eq!(context["dependencies"], json!([]));
    assert_eq!(context["links"], json!([]));
    assert_eq!(app.get(&admin, &format!("/tasks?project={}", project.id)).await.body["total"], 2);

    let reader = app.agent(&admin, "Reader", worker_grants(&project)).await;
    let denied = app.get(&reader, &format!("/tasks?depends_on={}", foreign["id"].as_str().unwrap())).await;
    assert_eq!(denied.status, StatusCode::NOT_FOUND);

    assert_eq!(app.post(&admin, &format!("{path}/dependencies"), json!({
        "depends_on": peer["id"], "reason": "wait for peer"
    })).await.status, StatusCode::CREATED);
    assert_eq!(app.post(&admin, &format!("{path}/links"), json!({
        "related": peer["key"]
    })).await.status, StatusCode::CREATED);
    assert_eq!(app.patch(&admin, &path, json!({
        "version": 1, "parent": peer["key"]
    })).await.status, StatusCode::OK);
    let context = app.get(&reader, &format!("{path}/context")).await.body;
    assert_eq!(context["dependencies"][0]["task"]["id"], peer["id"]);
    assert_eq!(context["links"][0]["id"], peer["id"]);
    assert_eq!(context["parent"]["id"], peer["id"]);
}

#[tokio::test]
async fn legacy_relation_events_obey_current_access_in_context_history_and_streams() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let other = app.project(&admin, "SECRET").await;
    let reader = app.agent(&admin, "Reader", worker_grants(&project)).await;
    let task = app.task(&admin, &project, &project.implement, "X").await;
    let secret = app.task(&admin, &other, &other.implement, "Secret").await;
    let org = app.get(&admin, "/me").await.body["organization"]["id"].as_str().unwrap().to_owned();
    let after = app.get(&reader, "/events?after=latest").await.body["cursor"].as_i64().unwrap();
    // 保存済みの旧形式を再現。関係自体を解除済みでも履歴は残る。
    app.db.write_blocking(|conn| {
        for (kind, data) in [
            ("dependency.added", json!({"depends_on": secret["key"], "reason": "secret reason"})),
            ("dependency.removed", json!({"depends_on": secret["key"]})),
            ("link.added", json!({"related": secret["key"]})),
            ("link.removed", json!({"related": secret["key"]})),
            ("task.updated", json!({"changes":{"parent_id":{"from":null,"to":secret["id"]}}})),
            ("task.updated", json!({"changes":{"parent_id":{"from":secret["id"],"to":null}}})),
        ] {
            emit(conn, EventInput {
                org_id: &org, project_id: Some(&project.id), task_id: task["id"].as_str(),
                actor_id: Some(&admin.id), category: "task", kind, data,
            })?;
        }
        Ok(())
    }).unwrap();
    let path = format!("/tasks/{}", task["key"].as_str().unwrap());
    let marker = app.post(&admin, &format!("{path}/comments"), json!({"kind":"comment", "body":"visible"})).await;
    assert_eq!(marker.status, StatusCode::CREATED);
    for endpoint in [format!("{path}/context"), format!("{path}/history"), format!("/events?after={after}")] {
        let visible = app.get(&reader, &endpoint).await;
        assert_eq!(visible.status, StatusCode::OK);
        let body = visible.body.to_string();
        assert!(!body.contains("SECRET-1") && !body.contains("secret reason"));
        assert!(!body.contains(secret["id"].as_str().unwrap()));
        let privileged = app.get(&admin, &endpoint).await;
        assert!(privileged.body.to_string().contains("SECRET-1"));
    }
    let batch = app.get(&reader, &format!("/events?after={after}&limit=1")).await.body;
    assert_eq!(batch["events"][0]["type"], "comment.added");
    assert!(batch["cursor"].as_i64().unwrap() > after + 6);
    let Auth::Bearer(token) = &reader.auth else { unreachable!() };
    let response = app.router.clone().oneshot(Request::builder()
        .uri(format!("/api/v1/events/stream?after={after}"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty()).unwrap()).await.unwrap();
    let mut body = response.into_body();
    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), body.frame()).await.unwrap().unwrap().unwrap();
    let text = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
    assert!(text.contains("comment.added"));
    assert!(!text.contains("SECRET-1") && !text.contains("secret reason"));
}

#[tokio::test]
async fn dependency_discovery_combines_filters_and_pagination() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let root = app.task(&admin, &project, &project.implement, "Root").await;
    let unrelated = app.task(&admin, &project, &project.implement, "Unrelated").await;
    let mut ids = Vec::new();
    for title in ["First", "Second"] {
        let task = app.task(&admin, &project, &project.implement, title).await;
        ids.push(task["id"].clone());
        assert_eq!(app.post(&admin, &format!("/tasks/{}/dependencies", task["key"].as_str().unwrap()), json!({
            "depends_on": root["key"], "reason": "needs root"
        })).await.status, StatusCode::CREATED);
    }
    for reference in [&root["id"], &root["key"]] {
        let query = format!("/tasks?project={}&depends_on={}&priority=none&limit=1", project.id, reference.as_str().unwrap());
        let first = app.get(&admin, &query).await.body;
        assert_eq!(first["total"], 2);
        assert_eq!(first["tasks"][0]["id"], ids[0]);
        assert_eq!(first["next_cursor"], 1);
        let second = app.get(&admin, &format!("{query}&cursor=1")).await.body;
        assert_eq!(second["tasks"][0]["id"], ids[1]);
        assert!(second["next_cursor"].is_null());
    }
    assert_eq!(app.get(&admin, &format!("/tasks?depends_on={}", unrelated["key"].as_str().unwrap())).await.body["total"], 0);
    // 完了済みの依存も関係として検索できる。
    assert_eq!(app.move_to(&admin, &root, &project.done).await.status, StatusCode::OK);
    assert_eq!(app.get(&admin, &format!("/tasks?depends_on={}&can_claim=true", root["key"].as_str().unwrap())).await.body["total"], 2);
}

#[tokio::test]
async fn workflow_history_preserves_before_and_after_and_failed_edits_leave_no_event() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let path = format!("/projects/{}/transition-rules", project.id);
    let reply = app.put(&admin, &path, json!({"rules":[{
        "from_stage_id":project.review,"to_stage_id":project.done,
        "required_permission":"edit","require_work_done":false
    }]})).await;
    assert_eq!(reply.status, StatusCode::OK);
    let events = app.get(&admin, "/events?after=0").await.body;
    let changes: Vec<&Value> = events["events"].as_array().unwrap().iter()
        .filter(|event| event["type"] == "workflow.rules_replaced").collect();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[1]["actor"]["id"], admin.id);
    let data = &changes[1]["data"];
    assert_eq!(data["from"][0]["required_permission"], "approve");
    assert_eq!(data["from"][0]["require_work_done"], true);
    assert_eq!(data["to"][0]["required_permission"], "edit");
    assert_eq!(data["to"][0]["require_work_done"], false);
    assert_eq!(data["to"][0]["from_stage_id"], project.review);
    assert_eq!(data["to"][0]["to_stage_id"], project.done);
    let failed = app.put(&admin, &path, json!({"rules":[{
        "from_stage_id":"missing","to_stage_id":project.done,
        "required_permission":"edit","require_work_done":false
    }]})).await;
    assert_eq!(failed.status, StatusCode::UNPROCESSABLE_ENTITY);
    let current = app.get(&admin, &format!("/projects/{}", project.id)).await.body;
    assert_eq!(current["transition_rules"], data["to"]);
    assert_eq!(app.get(&admin, "/events?after=0").await.body, events);
}
