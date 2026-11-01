//! タスクの破棄。実施しないと決めた仕事を、実施済みの完了と区別して閉じる。

mod common;

use agent_native_kanban::db::after_seconds;
use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};

fn key(task: &Value) -> String {
    task["key"].as_str().unwrap().to_string()
}

fn editor_grants(project: &Project) -> Value {
    json!([
        { "project_id": project.id, "permission": "view" },
        { "project_id": project.id, "permission": "edit" },
        { "project_id": project.id, "permission": "claim" },
    ])
}

async fn discard(app: &App, client: &Client, task: &Value, stage: &str, reason: &str) -> Reply {
    let view = app.view(client, task).await;
    app.post(client, &format!("/tasks/{}/discard", key(task)), json!({
        "version": view["version"], "to_stage_id": stage, "reason": reason,
    })).await
}

async fn restore(app: &App, client: &Client, task: &Value, stage: &str, reason: &str) -> Reply {
    let view = app.view(client, task).await;
    app.post(client, &format!("/tasks/{}/restore", key(task)), json!({
        "version": view["version"], "to_stage_id": stage, "reason": reason,
    })).await
}

async fn setup() -> (App, Client, Project) {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    (app, admin, project)
}

#[tokio::test]
async fn discarding_closes_the_task_in_a_final_stage_and_keeps_its_records() {
    let (app, admin, project) = setup().await;
    let task = app.task(&admin, &project, &project.implement, "unneeded").await;
    let path = format!("/tasks/{}", key(&task));
    let comment = app.post(&admin, &format!("{path}/comments"), json!({ "kind": "comment", "body": "keep me" })).await;
    assert_eq!(comment.status, StatusCode::CREATED, "{:?}", comment.body);

    assert!(app.view(&admin, &task).await["discard"].is_null());
    let done = discard(&app, &admin, &task, &project.done, "  replaced by DEV-9 ").await;
    assert_eq!(done.status, StatusCode::OK, "{:?}", done.body);
    assert_eq!(done.body["stage"]["id"], json!(project.done));
    assert_eq!(done.body["discard"]["reason"], "replaced by DEV-9");
    assert_eq!(done.body["discard"]["discarded_by"]["id"], json!(admin.id));
    assert!(done.body["discard"]["discarded_at"].is_string());
    assert_eq!(done.body["work"]["reasons"][0]["code"], "discarded");

    let context = app.get(&admin, &format!("{path}/context")).await.body;
    assert_eq!(context["comments"][0]["body"], "keep me");
    let events: Vec<&str> = context["history"].as_array().unwrap().iter().filter_map(|e| e["type"].as_str()).collect();
    assert!(events.contains(&"task.created") && events.contains(&"task.discarded"), "{events:?}");
}

#[tokio::test]
async fn discard_requires_edit_permission_and_valid_input() {
    let (app, admin, project) = setup().await;
    let task = app.task(&admin, &project, &project.implement, "x").await;
    let other = app.project(&admin, "OTH").await;
    let path = format!("/tasks/{}/discard", key(&task));
    let version = app.view(&admin, &task).await["version"].clone();
    let body = json!({ "version": version, "to_stage_id": project.done, "reason": "no" });

    assert_eq!(app.post(&ANON, &path, body.clone()).await.status, StatusCode::UNAUTHORIZED);
    let worker = app.agent(&admin, "worker", worker_grants(&project)).await;
    assert_eq!(app.post(&worker, &path, body.clone()).await.status, StatusCode::FORBIDDEN);
    let stranger = app.agent(&admin, "stranger", worker_grants(&other)).await;
    assert_eq!(app.post(&stranger, &path, body.clone()).await.status, StatusCode::NOT_FOUND);

    for (reason, stage) in [
        ("   ".to_string(), project.done.clone()),
        ("あ".repeat(501), project.done.clone()),
        ("no".to_string(), other.done.clone()),
        ("no".to_string(), project.review.clone()),
    ] {
        let reply = app.post(&admin, &path, json!({ "version": version, "to_stage_id": stage, "reason": reason })).await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", reply.body);
    }
    let stale = app.post(&admin, &path, json!({ "version": 0, "to_stage_id": project.done, "reason": "no" })).await;
    assert_eq!(stale.status, StatusCode::CONFLICT);
    assert_eq!(stale.code(), "version_conflict");

    let view = app.view(&admin, &task).await;
    assert!(view["discard"].is_null());
    assert_eq!(view["stage"]["id"], json!(project.implement));
    assert_eq!(view["version"], version);

    let editor = app.agent(&admin, "editor", editor_grants(&project)).await;
    assert_eq!(app.post(&editor, &path, body).await.status, StatusCode::OK);
}

#[tokio::test]
async fn only_open_unoccupied_tasks_can_be_discarded() {
    let (app, admin, project) = setup().await;
    let worker = app.agent(&admin, "worker", worker_grants(&project)).await;

    let occupied = app.task(&admin, &project, &project.implement, "occupied").await;
    app.claim(&worker, &occupied).await;
    let reply = discard(&app, &admin, &occupied, &project.done, "no").await;
    assert_eq!((reply.status, reply.code()), (StatusCode::CONFLICT, "task_occupied"));

    let finished = app.task(&admin, &project, &project.done, "finished").await;
    let reply = discard(&app, &admin, &finished, &project.done, "no").await;
    assert_eq!((reply.status, reply.code()), (StatusCode::CONFLICT, "task_finished"));

    let open = app.task(&admin, &project, &project.backlog, "open").await;
    let reply = restore(&app, &admin, &open, &project.backlog, "no").await;
    assert_eq!((reply.status, reply.code()), (StatusCode::CONFLICT, "task_not_discarded"));

    assert_eq!(discard(&app, &admin, &open, &project.done, "no").await.status, StatusCode::OK);
    let reply = discard(&app, &admin, &open, &project.done, "again").await;
    assert_eq!((reply.status, reply.code()), (StatusCode::CONFLICT, "task_discarded"));
}

#[tokio::test]
async fn discard_ignores_transition_rules_because_it_approves_nothing() {
    let (app, admin, project) = setup().await;
    // Review → Done は approve と作業完了を求めるルールがある。
    let task = app.task(&admin, &project, &project.review, "under review").await;
    let editor = app.agent(&admin, "editor", editor_grants(&project)).await;
    let refused = app.move_to(&editor, &task, &project.done).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{:?}", refused.body);
    let reply = discard(&app, &editor, &task, &project.done, "no longer needed").await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
}

#[tokio::test]
async fn discarded_task_cannot_be_claimed_even_in_a_final_stage_that_requires_work() {
    let (app, admin, project) = setup().await;
    let verify = app.post(&admin, &format!("/projects/{}/stages", project.id), json!({
        "name": "Verified", "requires_work": true, "claim_permission": "claim", "is_final": true,
    })).await;
    let stage = verify.body["stages"].as_array().unwrap().iter()
        .find(|item| item["name"] == "Verified").unwrap()["id"].as_str().unwrap().to_string();
    let task = app.task(&admin, &project, &project.implement, "x").await;
    assert_eq!(discard(&app, &admin, &task, &stage, "no").await.status, StatusCode::OK);

    let worker = app.agent(&admin, "worker", worker_grants(&project)).await;
    let view = app.view(&worker, &task).await;
    assert_eq!(view["work"]["claimable"], false);
    assert_eq!(view["work"]["reasons"][0]["code"], "discarded");
    let claim = app.post(&worker, &format!("/tasks/{}/work/claim", key(&task)), json!({})).await;
    assert_eq!(claim.status, StatusCode::CONFLICT);
    assert!(claim.reason_codes().contains(&"discarded".to_string()), "{:?}", claim.body);
}

#[tokio::test]
async fn discarded_dependency_keeps_blocking_until_removed_or_finished() {
    let (app, admin, project) = setup().await;
    let dependency = app.task(&admin, &project, &project.implement, "premise").await;
    let dependent = app.task(&admin, &project, &project.implement, "follow-up").await;
    let path = format!("/tasks/{}/dependencies", key(&dependent));
    let added = app.post(&admin, &path, json!({ "depends_on": key(&dependency), "reason": "needs it" })).await;
    assert_eq!(added.status, StatusCode::CREATED, "{:?}", added.body);

    let view = app.view(&admin, &dependent).await;
    assert_eq!(view["work"]["blockers"][0]["discarded"], false);

    assert_eq!(discard(&app, &admin, &dependency, &project.done, "dropped").await.status, StatusCode::OK);
    let view = app.view(&admin, &dependent).await;
    assert_eq!(view["work"]["claimable"], false);
    assert_eq!(view["work"]["blockers"][0]["key"], json!(key(&dependency)));
    assert_eq!(view["work"]["blockers"][0]["discarded"], true);

    // 取り消すと通常の未完了の依存に戻り、完了すれば解除される。
    assert_eq!(restore(&app, &admin, &dependency, &project.implement, "needed").await.status, StatusCode::OK);
    let view = app.view(&admin, &dependent).await;
    assert_eq!(view["work"]["claimable"], false);
    assert_eq!(view["work"]["blockers"][0]["discarded"], false);
    assert_eq!(app.move_to(&admin, &dependency, &project.backlog).await.status, StatusCode::OK);
    let reviewed = app.view(&admin, &dependency).await;
    assert_eq!(app.post(&admin, &format!("/tasks/{}/stage", key(&dependency)), json!({
        "to_stage_id": project.done, "version": reviewed["version"],
    })).await.status, StatusCode::OK);
    assert_eq!(app.view(&admin, &dependent).await["work"]["claimable"], true);

    // 依存を外しても解除される。
    let second = app.task(&admin, &project, &project.implement, "second premise").await;
    app.post(&admin, &path, json!({ "depends_on": key(&second), "reason": "needs it" })).await;
    assert_eq!(discard(&app, &admin, &second, &project.done, "dropped").await.status, StatusCode::OK);
    assert_eq!(app.view(&admin, &dependent).await["work"]["claimable"], false);
    let removed = app.delete(&admin, &format!("{path}/{}", key(&second))).await;
    assert!(removed.status.is_success(), "{:?}", removed.body);
    assert_eq!(app.view(&admin, &dependent).await["work"]["claimable"], true);
}

#[tokio::test]
async fn discarded_task_leaves_normal_stage_moves_until_restored() {
    let (app, admin, project) = setup().await;
    let task = app.task(&admin, &project, &project.implement, "x").await;
    assert_eq!(discard(&app, &admin, &task, &project.done, "no").await.status, StatusCode::OK);

    let moved = app.move_to(&admin, &task, &project.backlog).await;
    assert_eq!((moved.status, moved.code()), (StatusCode::CONFLICT, "task_discarded"));

    let to_final = restore(&app, &admin, &task, &project.done, "back").await;
    assert_eq!(to_final.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", to_final.body);

    let back = restore(&app, &admin, &task, &project.implement, "needed after all").await;
    assert_eq!(back.status, StatusCode::OK, "{:?}", back.body);
    assert!(back.body["discard"].is_null());
    assert_eq!(back.body["stage"]["id"], json!(project.implement));
    assert_eq!(back.body["work"]["claimable"], true);
    assert_eq!(app.move_to(&admin, &task, &project.backlog).await.status, StatusCode::OK);
}

#[tokio::test]
async fn a_stage_holding_discarded_tasks_stays_final() {
    let (app, admin, project) = setup().await;
    let task = app.task(&admin, &project, &project.implement, "x").await;
    assert_eq!(discard(&app, &admin, &task, &project.done, "no").await.status, StatusCode::OK);

    let path = format!("/stages/{}", project.done);
    let reply = app.patch(&admin, &path, json!({ "is_final": false })).await;
    assert_eq!((reply.status, reply.code()), (StatusCode::CONFLICT, "stage_has_discarded_tasks"));

    assert_eq!(restore(&app, &admin, &task, &project.implement, "ok").await.status, StatusCode::OK);
    assert_eq!(app.patch(&admin, &path, json!({ "is_final": false })).await.status, StatusCode::OK);
}

#[tokio::test]
async fn discarding_a_parent_does_not_touch_its_children() {
    let (app, admin, project) = setup().await;
    let parent = app.task(&admin, &project, &project.implement, "parent").await;
    let child = app.task(&admin, &project, &project.implement, "child").await;
    let view = app.view(&admin, &child).await;
    let linked = app.patch(&admin, &format!("/tasks/{}", key(&child)), json!({
        "version": view["version"], "parent": key(&parent),
    })).await;
    assert_eq!(linked.status, StatusCode::OK, "{:?}", linked.body);

    assert_eq!(discard(&app, &admin, &parent, &project.done, "no").await.status, StatusCode::OK);
    let view = app.view(&admin, &child).await;
    assert!(view["discard"].is_null());
    assert_eq!(view["stage"]["id"], json!(project.implement));
}

#[tokio::test]
async fn discard_and_restore_are_audited_with_the_delegated_token_and_hidden_from_outsiders() {
    let (app, admin, project) = setup().await;
    let other = app.project(&admin, "OTH").await;
    let editor = app.agent(&admin, "editor", editor_grants(&project)).await;
    let outsider = app.agent(&admin, "outsider", worker_grants(&other)).await;
    let task = app.task(&admin, &project, &project.implement, "x").await;

    let delegated = app.post(&editor, "/me/delegations", json!({
        "name": "sub", "expires_at": after_seconds(3600),
        "scopes": [{ "project_id": project.id, "permission": "edit" }],
    })).await;
    assert_eq!(delegated.status, StatusCode::CREATED, "{:?}", delegated.body);
    let token_id = delegated.body["id"].clone();
    let sub = Client {
        id: editor.id.clone(),
        auth: Auth::Bearer(delegated.body["token"].as_str().unwrap().to_string()),
    };

    assert_eq!(discard(&app, &sub, &task, &project.done, "dropped").await.status, StatusCode::OK);
    assert_eq!(restore(&app, &admin, &task, &project.implement, "needed").await.status, StatusCode::OK);

    let events = app.get(&admin, "/events?after=0").await.body;
    let events = events["events"].as_array().unwrap();
    let find = |kind: &str| events.iter().find(|event| event["type"] == kind).expect(kind);
    let discarded = find("task.discarded");
    assert_eq!(discarded["actor"]["id"], json!(editor.id));
    assert_eq!(discarded["delegated_token_id"], token_id);
    assert_eq!(discarded["data"]["reason"], "dropped");
    assert_eq!(discarded["data"]["from"]["id"], json!(project.implement));
    assert_eq!(discarded["data"]["to"]["id"], json!(project.done));
    let restored = find("task.restored");
    assert_eq!(restored["actor"]["id"], json!(admin.id));
    assert_eq!(restored["data"]["reason"], "needed");
    assert_eq!(restored["data"]["discard_reason"], "dropped");

    let hidden = app.get(&outsider, "/events?after=0").await.body;
    assert!(hidden["events"].as_array().unwrap().iter()
        .all(|event| !event["type"].as_str().unwrap().starts_with("task.")
            || event["project_id"] != json!(project.id)));
}
