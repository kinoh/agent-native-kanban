//! サブエージェントへの権限の委譲。委譲は権限を狭めるだけで、主体も管理者権限も増やさない。

mod common;

use std::sync::atomic::{AtomicI64, Ordering};

use agent_native_kanban::db::{after_seconds, now};
use axum::http::{Method, StatusCode};
use common::*;
use serde_json::{Value, json};

const HOUR: i64 = 3600;

/// 後に発行するものほど早く切れる期限。再委譲が親の期限を超えないようにする。
static EARLIER: AtomicI64 = AtomicI64::new(0);

async fn try_delegate(app: &App, client: &Client, body: Value) -> Reply {
    app.post(client, "/me/delegations", body).await
}

/// 委譲トークンで動くクライアントと、そのトークンの ID を返す。
async fn delegate(app: &App, client: &Client, scopes: Value, tasks: Option<Value>) -> (Client, String) {
    let expires_at = after_seconds(HOUR - EARLIER.fetch_add(1, Ordering::Relaxed));
    let mut body = json!({ "name": "sub", "expires_at": expires_at, "scopes": scopes });
    if let Some(tasks) = tasks {
        body["tasks"] = tasks;
    }
    let reply = try_delegate(app, client, body).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    let child = Client {
        id: client.id.clone(),
        auth: Auth::Bearer(reply.body["token"].as_str().unwrap().to_string()),
    };
    (child, reply.body["id"].as_str().unwrap().to_string())
}

fn scopes(project: &Project, permissions: &[&str]) -> Value {
    json!(permissions
        .iter()
        .map(|permission| json!({ "project_id": project.id, "permission": permission }))
        .collect::<Vec<_>>())
}

fn grants(project: &Project, permissions: &[&str]) -> Value {
    scopes(project, permissions)
}

fn key(task: &Value) -> &str {
    task["key"].as_str().unwrap()
}

async fn child_task(app: &App, client: &Client, project: &Project, parent: &Value, title: &str) -> Reply {
    app.post(client, "/tasks", json!({
        "project_id": project.id, "stage_id": project.implement, "title": title, "parent": key(parent),
    }))
    .await
}

#[tokio::test]
async fn an_agent_without_admin_rights_can_hand_a_subagent_part_of_its_own_rights() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let orchestrator = app.agent(&admin, "orchestrator", grants(&project, &["claim", "review"])).await;
    let task = app.task(&admin, &project, &project.implement, "implement").await;

    let (implementer, _) = delegate(&app, &orchestrator, scopes(&project, &["claim"]), None).await;
    let me = app.get(&implementer, "/me").await;
    assert_eq!(me.body["id"], orchestrator.id.as_str(), "the subagent is not a new principal");
    assert_eq!(me.body["is_org_admin"], false);

    let claim = app.claim(&implementer, &task).await;
    let path = format!("/tasks/{}/work/complete", key(&task));
    let done = app.post(&implementer, &path, json!({ "claim_id": claim, "summary": "done" })).await;
    assert_eq!(done.status, StatusCode::OK, "{:?}", done.body);

    let review = app.task(&admin, &project, &project.review, "review").await;
    let refused = app.post(&implementer, &format!("/tasks/{}/work/claim", key(&review)), json!({})).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(refused.body["error"]["details"]["required_permission"], "review");
    assert_eq!(app.post(&orchestrator, &format!("/tasks/{}/work/claim", key(&review)), json!({})).await.status, StatusCode::OK);
}

#[tokio::test]
async fn review_and_approve_can_be_delegated_but_admin_and_wider_rights_cannot() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let other = app.project(&admin, "OPS").await;
    let lead = app.agent(&admin, "lead", grants(&project, &["review", "approve"])).await;

    delegate(&app, &lead, scopes(&project, &["review", "approve", "view"]), None).await;

    let wider = try_delegate(&app, &lead, json!({
        "name": "x", "expires_at": after_seconds(HOUR), "scopes": scopes(&project, &["edit"]),
    })).await;
    assert_eq!(wider.status, StatusCode::FORBIDDEN);
    assert_eq!(wider.body["error"]["details"]["required_permission"], "edit");

    let hidden = try_delegate(&app, &lead, json!({
        "name": "x", "expires_at": after_seconds(HOUR), "scopes": scopes(&other, &["view"]),
    })).await;
    assert_eq!(hidden.status, StatusCode::NOT_FOUND);

    let owner = app.agent(&admin, "owner", grants(&project, &["admin"])).await;
    let refused = try_delegate(&app, &owner, json!({
        "name": "x", "expires_at": after_seconds(HOUR), "scopes": scopes(&project, &["admin"]),
    })).await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    // admin を持つ主体も、それに含まれる個別の権限なら渡せる。
    delegate(&app, &owner, scopes(&project, &["edit"]), None).await;
}

#[tokio::test]
async fn delegated_tokens_must_expire_within_the_parent_token() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let agent = app.agent(&admin, "agent", grants(&project, &["claim"])).await;
    let claim = scopes(&project, &["claim"]);

    let missing = try_delegate(&app, &agent, json!({ "name": "x", "scopes": claim })).await;
    assert_eq!(missing.status, StatusCode::UNPROCESSABLE_ENTITY);
    let past = try_delegate(&app, &agent, json!({ "name": "x", "expires_at": after_seconds(-60), "scopes": claim })).await;
    assert_eq!(past.status, StatusCode::UNPROCESSABLE_ENTITY);

    // 無期限の親には長さの上限がない。
    let far = after_seconds(10 * 365 * 24 * HOUR);
    let long = try_delegate(&app, &agent, json!({ "name": "x", "expires_at": far, "scopes": claim })).await;
    assert_eq!(long.status, StatusCode::CREATED);

    let (child, _) = delegate(&app, &agent, claim.clone(), None).await;
    let beyond = try_delegate(&app, &child, json!({
        "name": "x", "expires_at": after_seconds(2 * HOUR), "scopes": claim,
    })).await;
    assert_eq!(beyond.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn the_database_refuses_a_delegated_token_without_expiry() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let error = app.db.write_blocking(|conn| {
        conn.execute("INSERT INTO api_tokens (id, principal_id, name, token_hash, created_by, created_at) VALUES ('root', ?1, 'r', 'h1', ?1, ?2)", (&admin.id, now()))?;
        conn.execute("INSERT INTO api_tokens (id, principal_id, name, token_hash, created_by, created_at, parent_token_id) VALUES ('sub', ?1, 's', 'h2', ?1, ?2, 'root')", (&admin.id, now()))?;
        Ok(())
    }).unwrap_err();
    assert!(error.message.contains("must expire"), "{}", error.message);
}

#[tokio::test]
async fn removing_a_right_from_the_principal_takes_effect_on_the_next_request() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let agent = app.agent(&admin, "agent", grants(&project, &["claim", "edit"])).await;
    let (child, _) = delegate(&app, &agent, scopes(&project, &["claim", "edit"]), None).await;
    let task = app.task(&admin, &project, &project.implement, "task").await;
    let hold = format!("/tasks/{}/work/hold", key(&task));
    assert_eq!(app.post(&child, &hold, json!({ "reason": "wait" })).await.status, StatusCode::OK);

    let listed = app.get(&admin, &format!("/principals/{}/grants", agent.id)).await;
    let edit = listed.body["grants"].as_array().unwrap().iter().find(|g| g["permission"] == "edit").unwrap();
    app.delete(&admin, &format!("/grants/{}", edit["id"].as_str().unwrap())).await;

    let unhold = format!("/tasks/{}/work/unhold", key(&task));
    assert_eq!(app.post(&child, &unhold, json!({})).await.status, StatusCode::FORBIDDEN);
    assert_eq!(app.get(&child, &format!("/tasks/{}", key(&task))).await.status, StatusCode::OK);
}

#[tokio::test]
async fn invalidating_any_ancestor_invalidates_all_descendants() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let agent = app.agent(&admin, "agent", grants(&project, &["claim"])).await;
    let claim = scopes(&project, &["claim"]);

    // 親の失効
    let (child, child_id) = delegate(&app, &agent, claim.clone(), None).await;
    let (grandchild, _) = delegate(&app, &child, claim.clone(), None).await;
    assert_eq!(app.get(&grandchild, "/me").await.status, StatusCode::OK);
    assert_eq!(app.delete(&agent, &format!("/tokens/{child_id}")).await.status, StatusCode::NO_CONTENT);
    assert_eq!(app.get(&child, "/me").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(app.get(&grandchild, "/me").await.status, StatusCode::UNAUTHORIZED);

    // 親の期限切れ
    let (child, child_id) = delegate(&app, &agent, claim.clone(), None).await;
    let (grandchild, _) = delegate(&app, &child, claim.clone(), None).await;
    app.db.write_blocking(|conn| {
        conn.execute("UPDATE api_tokens SET expires_at = ?1 WHERE id = ?2", (after_seconds(-1), &child_id))?;
        Ok(())
    }).unwrap();
    assert_eq!(app.get(&grandchild, "/me").await.status, StatusCode::UNAUTHORIZED);

    // 主体の無効化と、復帰後も失効が残らないこと
    let (child, _) = delegate(&app, &agent, claim.clone(), None).await;
    let path = format!("/principals/{}", agent.id);
    app.patch(&admin, &path, json!({ "status": "disabled" })).await;
    assert_eq!(app.get(&child, "/me").await.status, StatusCode::UNAUTHORIZED);
    app.patch(&admin, &path, json!({ "status": "active" })).await;
    assert_eq!(app.get(&child, "/me").await.status, StatusCode::OK);

    // 主体の削除
    assert_eq!(app.delete(&admin, &path).await.status, StatusCode::NO_CONTENT);
    assert_eq!(app.get(&child, "/me").await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_token_can_revoke_only_itself_and_what_it_issued() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let agent = app.agent(&admin, "agent", grants(&project, &["claim"])).await;
    let claim = scopes(&project, &["claim"]);
    let (first, first_id) = delegate(&app, &agent, claim.clone(), None).await;
    let (second, second_id) = delegate(&app, &agent, claim.clone(), None).await;

    let sibling = app.delete(&first, &format!("/tokens/{second_id}")).await;
    assert_eq!(sibling.status, StatusCode::FORBIDDEN);
    assert_eq!(app.get(&second, "/me").await.status, StatusCode::OK);

    assert_eq!(app.delete(&first, &format!("/tokens/{first_id}")).await.status, StatusCode::NO_CONTENT);
    assert_eq!(app.get(&first, "/me").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(app.delete(&agent, &format!("/tokens/{second_id}")).await.status, StatusCode::NO_CONTENT);
    assert_eq!(app.get(&second, "/me").await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn delegation_can_narrow_again_but_never_widen() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let agent = app.agent(&admin, "agent", grants(&project, &["claim", "edit"])).await;
    let (child, _) = delegate(&app, &agent, scopes(&project, &["claim", "edit"]), None).await;
    let (grandchild, _) = delegate(&app, &child, scopes(&project, &["claim"]), None).await;
    let (great, _) = delegate(&app, &grandchild, scopes(&project, &["claim"]), None).await;
    assert_eq!(app.get(&great, "/me").await.status, StatusCode::OK);

    let wider = try_delegate(&app, &grandchild, json!({
        "name": "x", "expires_at": after_seconds(60), "scopes": scopes(&project, &["edit"]),
    })).await;
    assert_eq!(wider.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn delegated_tokens_cannot_escape_through_credentials_or_admin_routes() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;

    // 人間も同じ操作で委譲でき、委譲トークンは人間本人の特権を受け継がない。
    let token = app.post(&admin, &format!("/principals/{}/tokens", admin.id), json!({ "name": "cli" })).await;
    let human = Client { id: admin.id.clone(), auth: Auth::Bearer(token.body["token"].as_str().unwrap().to_string()) };
    let (child, _) = delegate(&app, &human, scopes(&project, &["edit"]), None).await;

    let tokens = format!("/principals/{}/tokens", admin.id);
    let minted = app.post(&child, &tokens, json!({ "name": "escape" })).await;
    assert_eq!(minted.status, StatusCode::FORBIDDEN);
    assert_eq!(minted.code(), "delegated_token_restricted");
    assert_eq!(app.get(&child, &tokens).await.status, StatusCode::FORBIDDEN);

    let password = app.put(&child, "/me/password", json!({
        "current_password": PASSWORD, "new_password": "another long password",
    })).await;
    assert_eq!(password.status, StatusCode::FORBIDDEN);

    assert_eq!(app.get(&child, "/me").await.body["is_org_admin"], false);
    assert_eq!(app.post(&child, "/principals", json!({ "display_name": "x" })).await.status, StatusCode::FORBIDDEN);
    let grant = app.post(&child, &format!("/principals/{}/grants", admin.id), json!({
        "project_id": project.id, "permission": "approve",
    })).await;
    assert_eq!(grant.status, StatusCode::FORBIDDEN);
    assert_eq!(app.get(&child, "/signup-links").await.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn delegation_starts_from_a_token_not_a_browser_session() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let reply = try_delegate(&app, &admin, json!({
        "name": "x", "expires_at": after_seconds(HOUR), "scopes": scopes(&project, &["edit"]),
    })).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(reply.code(), "token_required");
}

#[tokio::test]
async fn the_grants_of_the_principal_stay_hidden_outside_the_delegation() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let secret = app.project(&admin, "SEC").await;
    let mut all = grants(&project, &["claim"]);
    all.as_array_mut().unwrap().extend(grants(&secret, &["claim"]).as_array().unwrap().clone());
    let agent = app.agent(&admin, "agent", all).await;
    let (child, _) = delegate(&app, &agent, scopes(&project, &["claim"]), None).await;

    let listed = app.get(&child, &format!("/principals/{}/grants", agent.id)).await;
    assert!(!listed.body.to_string().contains(&secret.id));
    assert!(!app.get(&child, "/me").await.body.to_string().contains(&secret.id));
    assert!(!app.get(&child, "/projects").await.body.to_string().contains(&secret.id));
}

#[tokio::test]
async fn a_subagent_can_continue_the_parents_work_only_with_the_claim_right() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let agent = app.agent(&admin, "agent", grants(&project, &["claim"])).await;
    let task = app.task(&admin, &project, &project.implement, "task").await;
    let claim = app.claim(&agent, &task).await;

    let (reader, _) = delegate(&app, &agent, scopes(&project, &["view"]), None).await;
    let seen = app.view(&reader, &task).await;
    assert!(seen["work"]["claim_id"].is_null(), "a view-only token must not receive the work right");
    let start = format!("/tasks/{}/work/start", key(&task));
    assert_eq!(app.post(&reader, &start, json!({ "claim_id": claim })).await.status, StatusCode::FORBIDDEN);

    let (worker, _) = delegate(&app, &agent, scopes(&project, &["claim"]), None).await;
    assert_eq!(app.view(&worker, &task).await["work"]["claim_id"], claim.as_str());
    assert_eq!(app.post(&worker, &start, json!({ "claim_id": claim })).await.status, StatusCode::OK);
}

#[tokio::test]
async fn a_task_limited_token_works_only_on_that_task_and_its_descendants() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let agent = app.agent(&admin, "agent", grants(&project, &["claim", "create", "edit"])).await;
    let root = app.task(&admin, &project, &project.implement, "root").await;
    let inner = child_task(&app, &admin, &project, &root, "inner").await.body;
    let outside = app.task(&admin, &project, &project.implement, "outside").await;

    let (child, _) = delegate(&app, &agent, scopes(&project, &["claim", "create", "edit"]), Some(json!([key(&root)]))).await;

    // 子孫は発行後に増えても範囲に入る。
    assert_eq!(app.get(&child, &format!("/tasks/{}", key(&inner))).await.status, StatusCode::OK);
    let created = child_task(&app, &child, &project, &inner, "created").await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    app.claim(&child, &created.body).await;

    for path in [
        format!("/tasks/{}", key(&outside)),
        format!("/tasks/{}/context", key(&outside)),
        format!("/tasks/{}/history", key(&outside)),
    ] {
        assert_eq!(app.get(&child, &path).await.status, StatusCode::NOT_FOUND, "{path}");
    }
    let claim = app.post(&child, &format!("/tasks/{}/work/claim", key(&outside)), json!({})).await;
    assert_eq!(claim.status, StatusCode::NOT_FOUND);
    let comment = app.post(&child, &format!("/tasks/{}/comments", key(&outside)), json!({ "kind": "comment", "body": "x" })).await;
    assert_eq!(comment.status, StatusCode::NOT_FOUND);

    let top_level = app.post(&child, "/tasks", json!({ "project_id": project.id, "stage_id": project.implement, "title": "x" })).await;
    assert_eq!(top_level.status, StatusCode::FORBIDDEN);
    assert_eq!(top_level.code(), "outside_delegated_tasks");
    let foreign_parent = child_task(&app, &child, &project, &outside, "x").await;
    assert_eq!(foreign_parent.status, StatusCode::NOT_FOUND);

    let depends = app.post(&child, &format!("/tasks/{}/dependencies", key(&root)), json!({ "depends_on": key(&outside), "reason": "x" })).await;
    assert_eq!(depends.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_task_limited_token_cannot_read_outside_through_lists_feeds_or_relations() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let agent = app.agent(&admin, "agent", grants(&project, &["claim"])).await;
    let parent = app.task(&admin, &project, &project.implement, "secret parent").await;
    let task = child_task(&app, &admin, &project, &parent, "visible").await.body;
    let outside = app.task(&admin, &project, &project.implement, "secret outside").await;

    let path = format!("/tasks/{}", key(&task));
    let depends = app.post(&admin, &format!("{path}/dependencies"), json!({ "depends_on": key(&outside), "reason": "needs" })).await;
    assert_eq!(depends.status, StatusCode::CREATED);
    for target in [&task, &outside] {
        let question = app.post(&admin, &format!("/tasks/{}/comments", key(target)), json!({
            "kind": "question", "body": format!("about {}", key(target)), "addressee_id": agent.id,
        })).await;
        assert_eq!(question.status, StatusCode::CREATED);
    }
    let file = app.request(&admin, Method::POST, &format!("/tasks/{}/files?role=attachment&name=a.txt", key(&outside)), Some(json!("data")), &[]).await;
    assert_eq!(file.status, StatusCode::CREATED);

    let (child, _) = delegate(&app, &agent, scopes(&project, &["claim"]), Some(json!([key(&task)]))).await;

    let list = app.get(&child, "/tasks").await;
    assert_eq!(list.body["total"], 1);
    assert_eq!(list.body["tasks"][0]["key"], key(&task));

    let context = app.get(&child, &format!("{path}/context")).await;
    assert_eq!(context.status, StatusCode::OK);
    assert!(context.body["parent"].is_null());
    assert_eq!(context.body["dependencies"].as_array().unwrap().len(), 0);
    assert_eq!(context.body["task"]["work"]["hidden_blockers"], 1);
    assert_eq!(context.body["task"]["work"]["blockers"].as_array().unwrap().len(), 0);

    let inbox = app.get(&child, "/me/inbox").await;
    assert_eq!(inbox.body["requests"].as_array().unwrap().len(), 1);
    assert_eq!(inbox.body["requests"][0]["task"]["key"], key(&task));

    let events = app.get(&child, "/events?after=0").await;
    let list = events.body["events"].as_array().unwrap();
    assert!(!list.is_empty());
    assert!(list.iter().all(|event| event["task_id"] == task["id"]), "{list:?}");

    let artifact = file.body["id"].as_str().unwrap();
    assert_eq!(app.get(&child, &format!("/artifacts/{artifact}/content")).await.status, StatusCode::NOT_FOUND);
    assert_eq!(app.get(&agent, &format!("/artifacts/{artifact}/content")).await.status, StatusCode::OK);
}

#[tokio::test]
async fn task_limits_are_kept_when_delegating_again_and_when_tasks_move_away() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let agent = app.agent(&admin, "agent", grants(&project, &["claim"])).await;
    let root = app.task(&admin, &project, &project.implement, "root").await;
    let inner = child_task(&app, &admin, &project, &root, "inner").await.body;
    let outside = app.task(&admin, &project, &project.implement, "outside").await;
    let claim = scopes(&project, &["claim"]);
    let (child, _) = delegate(&app, &agent, claim.clone(), Some(json!([key(&root)]))).await;

    let whole = try_delegate(&app, &child, json!({ "name": "x", "expires_at": after_seconds(60), "scopes": claim })).await;
    assert_eq!(whole.status, StatusCode::UNPROCESSABLE_ENTITY);
    let foreign = try_delegate(&app, &child, json!({
        "name": "x", "expires_at": after_seconds(60), "scopes": claim, "tasks": [key(&outside)],
    })).await;
    assert_eq!(foreign.status, StatusCode::NOT_FOUND);
    let (grandchild, _) = delegate(&app, &child, claim.clone(), Some(json!([key(&inner)]))).await;
    assert_eq!(app.get(&grandchild, &format!("/tasks/{}", key(&inner))).await.status, StatusCode::OK);

    // 親の範囲から外れたタスクは、子の範囲に書かれていても触れない。
    let current = app.view(&admin, &inner).await;
    let moved = app.patch(&admin, &format!("/tasks/{}", key(&inner)), json!({ "version": current["version"], "parent": null })).await;
    assert_eq!(moved.status, StatusCode::OK, "{:?}", moved.body);
    assert_eq!(app.get(&grandchild, &format!("/tasks/{}", key(&inner))).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tasks_must_match_the_projects_of_the_scopes() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let other = app.project(&admin, "OPS").await;
    let mut both = grants(&project, &["claim"]);
    both.as_array_mut().unwrap().extend(grants(&other, &["claim"]).as_array().unwrap().clone());
    let agent = app.agent(&admin, "agent", both.clone()).await;
    let task = app.task(&admin, &project, &project.implement, "task").await;
    let body = |scopes: Value, tasks: Value| json!({ "name": "x", "expires_at": after_seconds(60), "scopes": scopes, "tasks": tasks });

    assert_eq!(try_delegate(&app, &agent, body(scopes(&project, &["claim"]), json!([]))).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(try_delegate(&app, &agent, body(scopes(&other, &["claim"]), json!([key(&task)]))).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(try_delegate(&app, &agent, body(both, json!([key(&task)]))).await.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn operations_stay_attributed_to_the_principal_and_name_the_delegated_token() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let agent = app.agent(&admin, "agent", grants(&project, &["claim"])).await;
    let task = app.task(&admin, &project, &project.implement, "task").await;
    let (child, token_id) = delegate(&app, &agent, scopes(&project, &["claim"]), Some(json!([key(&task)]))).await;
    app.claim(&child, &task).await;

    let history = app.get(&admin, &format!("/tasks/{}/history", key(&task))).await;
    let events = history.body["history"].as_array().unwrap();
    let claimed = events.iter().find(|event| event["type"] == "work.claimed").unwrap();
    assert_eq!(claimed["actor"]["id"], agent.id.as_str());
    assert_eq!(claimed["delegated_token_id"], token_id.as_str());
    let created = events.iter().find(|event| event["type"] == "task.created").unwrap();
    assert!(created["delegated_token_id"].is_null());

    let audit = app.get(&admin, "/events?after=0").await;
    let issued = audit.body["events"].as_array().unwrap().iter().find(|event| event["type"] == "token.delegated").unwrap().clone();
    assert_eq!(issued["data"]["token_id"], token_id.as_str());
    assert_eq!(issued["data"]["tasks"][0], task["id"]);
    assert_eq!(issued["data"]["scopes"][0]["permission"], "claim");
}

#[tokio::test]
async fn token_lists_name_only_the_limited_tasks_the_reader_can_see() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let agent = app.agent(&admin, "agent", grants(&project, &["claim"])).await;
    let shown = app.task(&admin, &project, &project.implement, "shown").await;
    let hidden = app.task(&admin, &project, &project.implement, "hidden").await;
    let claim = scopes(&project, &["claim"]);
    let (_, token_id) = delegate(&app, &agent, claim, Some(json!([key(&shown), key(&hidden)]))).await;

    let listed = app.get(&admin, &format!("/principals/{}/tokens", agent.id)).await;
    let token = listed.body["tokens"].as_array().unwrap().iter().find(|t| t["id"] == token_id.as_str()).unwrap();
    assert_eq!(token["task_keys"], json!([key(&shown), key(&hidden)]));
    let mine = app.get(&agent, "/me/delegations").await;
    assert_eq!(mine.body["delegations"][0]["task_keys"], json!([key(&shown), key(&hidden)]));

    // プロジェクトの権限を失った読者には、キーを見せない。
    let grants = app.get(&admin, &format!("/principals/{}/grants", agent.id)).await;
    for grant in grants.body["grants"].as_array().unwrap() {
        app.delete(&admin, &format!("/grants/{}", grant["id"].as_str().unwrap())).await;
    }
    let mine = app.get(&agent, "/me/delegations").await;
    assert_eq!(mine.body["delegations"][0]["task_keys"], json!([]));
    assert_eq!(mine.body["delegations"][0]["tasks"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn issuers_and_administrators_can_see_delegations_without_their_secrets() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let agent = app.agent(&admin, "agent", grants(&project, &["claim"])).await;
    let (child, child_id) = delegate(&app, &agent, scopes(&project, &["claim"]), None).await;
    let (_, grandchild_id) = delegate(&app, &child, scopes(&project, &["claim"]), None).await;

    let mine = app.get(&agent, "/me/delegations").await;
    let ids: Vec<&str> = mine.body["delegations"].as_array().unwrap().iter().map(|d| d["id"].as_str().unwrap()).collect();
    assert_eq!(ids, [child_id.as_str(), grandchild_id.as_str()]);
    assert_eq!(mine.body["delegations"][0]["scopes"][0]["permission"], "claim");
    assert!(!mine.body.to_string().contains("ank_"));

    let theirs = app.get(&child, "/me/delegations").await;
    assert_eq!(theirs.body["delegations"].as_array().unwrap().len(), 1);

    let listed = app.get(&admin, &format!("/principals/{}/tokens", agent.id)).await;
    let tokens = listed.body["tokens"].as_array().unwrap();
    let delegated = tokens.iter().find(|t| t["id"] == child_id.as_str()).unwrap();
    assert!(delegated["parent_token_id"].is_string());
    assert_eq!(delegated["scopes"][0]["permission"], "claim");
    assert!(!listed.body.to_string().contains("ank_"));
}
