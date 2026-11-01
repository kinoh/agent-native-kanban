//! 認証・認可・サインアップリンク・変更フィード。

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::*;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn link(app: &App, admin: &Client, max_uses: i64) -> Value {
    let reply = app
        .post(admin, "/signup-links", json!({
            "note": "new members", "expires_in_seconds": 3600, "max_uses": max_uses, "grants": [],
        }))
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    reply.body
}

fn token_of(link: &Value) -> String {
    link["url"].as_str().unwrap().rsplit('/').next().unwrap().to_string()
}

async fn try_signup(app: &App, token: &str, email: &str) -> Reply {
    app.post(&ANON, &format!("/signup/{token}"), json!({
        "email": email, "password": PASSWORD, "display_name": "Member",
    }))
    .await
}

#[tokio::test]
async fn signup_link_can_be_shared_until_its_limit() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let created = link(&app, &admin, 2).await;
    assert!(created["url"].as_str().unwrap().starts_with("http://kanban.test/signup/snp_"));
    let token = token_of(&created);

    let info = app.get(&ANON, &format!("/signup/{token}")).await;
    assert_eq!(info.body["organization"], "Acme");
    assert_eq!(try_signup(&app, &token, "a@acme.test").await.status, StatusCode::CREATED);
    assert_eq!(try_signup(&app, &token, "a@acme.test").await.code(), "email_taken");
    assert_eq!(try_signup(&app, &token, "b@acme.test").await.status, StatusCode::CREATED);

    let exhausted = try_signup(&app, &token, "c@acme.test").await;
    assert_eq!(exhausted.status, StatusCode::GONE);
    assert_eq!(exhausted.code(), "signup_link_invalid");
}

#[tokio::test]
async fn signup_link_stops_working_after_expiry_or_revocation() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;

    let expired = link(&app, &admin, 5).await;
    let id = expired["id"].as_str().unwrap().to_string();
    app.db
        .write_blocking(|conn| {
            conn.execute(
                "UPDATE signup_links SET expires_at = '2000-01-01T00:00:00.000Z' WHERE id = ?1",
                [&id],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(try_signup(&app, &token_of(&expired), "a@acme.test").await.status, StatusCode::GONE);

    let revoked = link(&app, &admin, 5).await;
    let id = revoked["id"].as_str().unwrap();
    assert_eq!(app.delete(&admin, &format!("/signup-links/{id}")).await.status, StatusCode::NO_CONTENT);
    assert_eq!(try_signup(&app, &token_of(&revoked), "a@acme.test").await.status, StatusCode::GONE);
}

#[tokio::test]
async fn signup_links_need_a_bounded_lifetime_and_an_admin() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let agent = app.agent(&admin, "A", worker_grants(&project)).await;
    let body = |seconds: i64| json!({
        "note": "x", "expires_in_seconds": seconds, "max_uses": 1, "grants": [],
    });
    let too_long = app.post(&admin, "/signup-links", body(365 * 24 * 3600)).await;
    assert_eq!(too_long.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(app.post(&agent, "/signup-links", body(3600)).await.status, StatusCode::FORBIDDEN);
    assert_eq!(app.get(&agent, "/signup-links").await.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn signup_grants_the_permissions_of_the_link() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let created = app
        .post(&admin, "/signup-links", json!({
            "note": "team", "expires_in_seconds": 3600, "max_uses": 1,
            "grants": [{ "project_id": project.id, "permission": "claim" }],
        }))
        .await;
    let member = app.signup(created.body["url"].as_str().unwrap(), "m@acme.test", "Member").await;
    let task = app.task(&admin, &project, &project.implement, "X").await;
    app.claim(&member, &task).await;
    assert_eq!(app.post(&member, "/signup-links", json!({
        "note": "x", "expires_in_seconds": 3600, "max_uses": 1, "grants": [],
    })).await.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn password_login_and_change() {
    let app = App::new();
    let admin = app.organization("Acme", "Admin@Acme.test").await;

    let wrong = app
        .post(&ANON, "/auth/login", json!({ "email": "admin@acme.test", "password": "wrong password" }))
        .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    let unknown = app
        .post(&ANON, "/auth/login", json!({ "email": "nobody@acme.test", "password": "wrong password" }))
        .await;
    assert_eq!(unknown.body, wrong.body, "unknown and wrong must be indistinguishable");

    let login = app
        .post(&ANON, "/auth/login", json!({ "email": "ADMIN@acme.test", "password": PASSWORD }))
        .await;
    assert_eq!(login.status, StatusCode::OK, "{:?}", login.body);
    let cookie = login.set_cookie.unwrap();
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
    let session = Client {
        id: admin.id.clone(),
        auth: Auth::Cookie(cookie.split(';').next().unwrap().to_string()),
    };
    assert_eq!(app.get(&session, "/me").await.body["id"], admin.id.as_str());

    let changed = app
        .put(&session, "/me/password", json!({
            "current_password": PASSWORD, "new_password": "a much better passphrase",
        }))
        .await;
    assert_eq!(changed.status, StatusCode::NO_CONTENT, "{:?}", changed.body);
    assert_eq!(app.get(&session, "/me").await.status, StatusCode::OK, "the acting session stays");
    assert_eq!(app.get(&admin, "/me").await.status, StatusCode::UNAUTHORIZED, "other sessions end");
    let old = app
        .post(&ANON, "/auth/login", json!({ "email": "admin@acme.test", "password": PASSWORD }))
        .await;
    assert_eq!(old.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn guide_is_public_markdown_and_other_api_data_stays_private() {
    let app = App::new();
    let response = app
        .router
        .clone()
        .oneshot(Request::builder().uri("/api/v1/guide").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/markdown; charset=utf-8"
    );
    let guide = String::from_utf8(response.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert!(guide.contains("Authorization: Bearer"));
    assert!(guide.contains("claim_id"));
    assert!(guide.contains("work/complete"));
    assert!(guide.contains("work/release"));
    assert!(guide.contains("/tasks/{task}/stage"));

    assert_eq!(app.get(&ANON, "/me").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(app.get(&ANON, "/projects").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(app.get(&ANON, "/tasks").await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn guide_happy_path_is_executable_with_isolated_data() {
    let app = App::new();
    let admin = app.organization("Guide Org", "admin@guide.test").await;
    let project = app.project(&admin, "GUIDE").await;
    let task = app.task(&admin, &project, &project.implement, "Follow the guide").await;
    let key = task["key"].as_str().unwrap();

    // The two setup calls shown at the start of the guide create a principal and return its token once.
    let principal = app
        .post(
            &admin,
            "/principals",
            json!({
                "display_name": "Guide agent",
                "grants": [
                    { "project_id": project.id, "permission": "claim" },
                    { "project_id": project.id, "permission": "edit" },
                ],
            }),
        )
        .await;
    assert_eq!(principal.status, StatusCode::CREATED, "{:?}", principal.body);
    let principal_id = principal.body["id"].as_str().unwrap().to_string();
    let issued = app
        .post(
            &admin,
            &format!("/principals/{principal_id}/tokens"),
            json!({ "name": "guide-cli" }),
        )
        .await;
    assert_eq!(issued.status, StatusCode::CREATED, "{:?}", issued.body);
    let agent = Client {
        id: principal_id,
        auth: Auth::Bearer(issued.body["token"].as_str().unwrap().to_string()),
    };

    assert_eq!(app.get(&agent, "/me").await.status, StatusCode::OK);
    let projects = app.get(&agent, "/projects").await;
    assert_eq!(projects.status, StatusCode::OK);
    assert!(projects.body["projects"].as_array().unwrap().iter().any(|item| item["id"] == project.id));
    let listed = app.get(&agent, &format!("/tasks?project={}&can_claim=true&limit=50&cursor=0", project.id)).await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.body["tasks"][0]["key"], key);
    assert_eq!(app.get(&agent, &format!("/tasks/{key}/context")).await.status, StatusCode::OK);

    let claimed = app.post(&agent, &format!("/tasks/{key}/work/claim"), json!({})).await;
    assert_eq!(claimed.status, StatusCode::OK, "{:?}", claimed.body);
    let claim_id = claimed.body["work"]["claim_id"].as_str().unwrap().to_string();
    assert_eq!(
        app.post(&agent, &format!("/tasks/{key}/work/start"), json!({ "claim_id": claim_id })).await.status,
        StatusCode::OK
    );
    assert_eq!(
        app.post(&agent, &format!("/tasks/{key}/comments"), json!({ "kind": "progress", "body": "ready" })).await.status,
        StatusCode::CREATED
    );
    assert_eq!(
        app.post(&agent, &format!("/tasks/{key}/artifacts"), json!({
            "role": "artifact", "name": "report", "url": "https://example.invalid/report",
        })).await.status,
        StatusCode::CREATED
    );
    let completed = app
        .post(
            &agent,
            &format!("/tasks/{key}/work/complete"),
            json!({
                "claim_id": claim_id,
                "summary": "complete",
                "evidence": [{ "name": "report", "url": "https://example.invalid/report" }],
            }),
        )
        .await;
    assert_eq!(completed.status, StatusCode::OK, "{:?}", completed.body);
    assert_eq!(completed.body["stage"]["id"], project.implement);
    assert_eq!(completed.body["work"]["readiness"], "done");

    // The documented delegation request, listing, and issuer revocation all work from this bearer token.
    let delegated = app
        .post(
            &agent,
            "/me/delegations",
            json!({
                "name": "subtask-worker",
                "expires_at": "2030-01-01T00:00:00Z",
                "scopes": [{ "project_id": project.id, "permission": "claim" }],
                "tasks": [key],
            }),
        )
        .await;
    assert_eq!(delegated.status, StatusCode::CREATED, "{:?}", delegated.body);
    let token_id = delegated.body["id"].as_str().unwrap();
    let descendants = app.get(&agent, "/me/delegations").await;
    assert_eq!(descendants.status, StatusCode::OK);
    assert!(descendants.body["delegations"].as_array().unwrap().iter().any(|item| item["id"] == token_id));
    assert_eq!(app.delete(&agent, &format!("/tokens/{token_id}")).await.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn guide_project_setup_sequence_creates_detailed_related_work() {
    let app = App::new();
    let admin = app.organization("Guide setup Org", "guide-setup-admin@test").await;

    // This is the project/stages, detailed parent-child tasks, and dependency sequence
    // documented in GET /guide. It uses only values that the guide publishes.
    let project = app
        .post(
            &admin,
            "/projects",
            json!({
                "key": "SCORE",
                "name": "Score Pilot",
                "description": "Evaluate scoring changes with reproducible evidence.",
                "stages": [
                    { "name": "Backlog", "requires_work": false, "claim_permission": "claim", "is_final": false },
                    { "name": "Build", "requires_work": true, "claim_permission": "claim", "is_final": false },
                    { "name": "Done", "requires_work": false, "claim_permission": "claim", "is_final": true },
                ],
            }),
        )
        .await;
    assert_eq!(project.status, StatusCode::CREATED, "{:?}", project.body);
    assert!(project.body["id"].is_string());
    assert_eq!(project.body["stages"].as_array().unwrap().len(), 3);
    let project_id = project.body["id"].as_str().unwrap();
    let build_stage = project.body["stages"][1]["id"].as_str().unwrap();

    let added_stage = app
        .post(
            &admin,
            &format!("/projects/{project_id}/stages"),
            json!({
                "name": "Review", "requires_work": true, "claim_permission": "review", "is_final": false,
            }),
        )
        .await;
    assert_eq!(added_stage.status, StatusCode::CREATED, "{:?}", added_stage.body);
    assert_eq!(added_stage.body["stages"].as_array().unwrap().len(), 4);

    let approving_stage = app
        .post(
            &admin,
            &format!("/projects/{project_id}/stages"),
            json!({
                "name": "Approval", "requires_work": true, "claim_permission": "approve", "is_final": false,
            }),
        )
        .await;
    assert_eq!(approving_stage.status, StatusCode::CREATED, "{:?}", approving_stage.body);
    assert_eq!(approving_stage.body["stages"].as_array().unwrap().len(), 5);

    let parent = app
        .post(
            &admin,
            "/tasks",
            json!({
                "project_id": project_id,
                "stage_id": build_stage,
                "title": "Define scoring inputs",
                "purpose": "Make the scoring inputs reviewable.",
                "acceptance_criteria": "Inputs and their source are recorded.",
                "priority": "medium",
            }),
        )
        .await;
    assert_eq!(parent.status, StatusCode::CREATED, "{:?}", parent.body);
    assert_eq!(parent.body["key"], "SCORE-1");
    assert_eq!(parent.body["priority"], "medium");
    assert_eq!(parent.body["purpose"], "Make the scoring inputs reviewable.");
    assert_eq!(parent.body["acceptance_criteria"], "Inputs and their source are recorded.");

    let child = app
        .post(
            &admin,
            "/tasks",
            json!({
                "project_id": project_id,
                "stage_id": build_stage,
                "title": "Implement score import",
                "purpose": "Load the reviewed scoring inputs.",
                "acceptance_criteria": "A supplied input produces the recorded score.",
                "priority": "high",
                "parent": "SCORE-1",
            }),
        )
        .await;
    assert_eq!(child.status, StatusCode::CREATED, "{:?}", child.body);
    assert_eq!(child.body["key"], "SCORE-2");
    assert_eq!(child.body["parent_id"], parent.body["id"]);

    for priority in ["urgent", "low", "none"] {
        let task = app
            .post(
                &admin,
                "/tasks",
                json!({
                    "project_id": project_id,
                    "stage_id": build_stage,
                    "title": format!("{priority} priority task"),
                    "priority": priority,
                }),
            )
            .await;
        assert_eq!(task.status, StatusCode::CREATED, "{:?}", task.body);
        assert_eq!(task.body["priority"], priority);
    }

    let dependency = app
        .post(
            &admin,
            "/tasks/SCORE-2/dependencies",
            json!({
                "depends_on": "SCORE-1",
                "reason": "Import needs the reviewed input definition.",
            }),
        )
        .await;
    assert_eq!(dependency.status, StatusCode::CREATED, "{:?}", dependency.body);
    assert_eq!(dependency.body, Value::Null);

    let context = app.get(&admin, "/tasks/SCORE-2/context").await;
    assert_eq!(context.status, StatusCode::OK, "{:?}", context.body);
    assert_eq!(context.body["dependencies"][0]["task"]["key"], "SCORE-1");

    let guide = app
        .router
        .clone()
        .oneshot(Request::builder().uri("/api/v1/guide").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let guide = String::from_utf8(guide.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    for value in ["claim", "review", "approve", "urgent", "high", "medium", "low", "none"] {
        assert!(guide.contains(value), "guide must document {value}");
    }
    assert!(guide.contains("201 Created"));
    assert!(guide.contains("no response body"));
}

#[tokio::test]
async fn organization_administrators_rename_only_their_organization_and_the_change_is_audited() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let other = app.organization("Other", "admin@other.test").await;
    let member_link = link(&app, &admin, 1).await;
    let member = app.signup(member_link["url"].as_str().unwrap(), "member@acme.test", "Member").await;
    let before = app.get(&admin, "/me").await.body;

    assert_eq!(
        app.patch(&ANON, "/organization", json!({ "name": "Nope" })).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.patch(&member, "/organization", json!({ "name": "Nope" })).await.status,
        StatusCode::FORBIDDEN
    );
    for name in ["   ".to_string(), "x".repeat(101)] {
        let reply = app.patch(&admin, "/organization", json!({ "name": name })).await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", reply.body);
    }

    let renamed = app
        .patch(&admin, "/organization", json!({ "name": "  Renamed Acme  " }))
        .await;
    assert_eq!(renamed.status, StatusCode::OK, "{:?}", renamed.body);
    assert_eq!(renamed.body["id"], before["organization"]["id"]);
    assert_eq!(renamed.body["name"], "Renamed Acme");
    let current = app.get(&admin, "/me").await.body;
    assert_eq!(current["organization"]["id"], before["organization"]["id"]);
    assert_eq!(current["organization"]["name"], "Renamed Acme");

    let other_renamed = app.patch(&other, "/organization", json!({ "name": "Other renamed" })).await;
    assert_eq!(other_renamed.status, StatusCode::OK);
    assert_eq!(app.get(&admin, "/me").await.body["organization"]["name"], "Renamed Acme");

    let events = app.get(&admin, "/events?after=0").await.body;
    let event = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["type"] == "organization.updated")
        .unwrap();
    assert_eq!(event["actor"]["id"], admin.id.as_str());
    assert_eq!(event["data"]["organization_id"], before["organization"]["id"]);
    assert_eq!(event["data"]["name"], json!({ "from": "Acme", "to": "Renamed Acme" }));
}

#[tokio::test]
async fn human_can_rename_only_their_own_display_name_and_the_change_is_audited() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let member_link = link(&app, &admin, 1).await;
    let member = app.signup(member_link["url"].as_str().unwrap(), "member@acme.test", "Member").await;
    let before = app.get(&member, "/me").await.body;

    assert_eq!(
        app.patch(&ANON, "/me", json!({ "display_name": "Nope" })).await.status,
        StatusCode::UNAUTHORIZED
    );
    for body in [json!({ "display_name": "   " }), json!({ "display_name": "x".repeat(101) }), json!({ "display_name": "ok", "email": "other@acme.test" })] {
        let reply = app.patch(&member, "/me", body).await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", reply.body);
    }

    let renamed = app
        .patch(&member, "/me", json!({ "display_name": "  Updated member  " }))
        .await;
    assert_eq!(renamed.status, StatusCode::OK, "{:?}", renamed.body);
    assert_eq!(renamed.body["id"], before["id"]);
    assert_eq!(renamed.body["display_name"], "Updated member");
    assert_eq!(renamed.body["email"], before["email"]);
    assert_eq!(renamed.body["organization"], before["organization"]);
    assert_eq!(app.get(&member, "/me").await.body["display_name"], "Updated member");

    let admin_rename = app
        .patch(&admin, &format!("/principals/{}", member.id), json!({ "display_name": "Admin renamed" }))
        .await;
    assert_eq!(admin_rename.status, StatusCode::OK);
    assert_eq!(app.get(&member, "/me").await.body["display_name"], "Admin renamed");

    let agent = app.agent(&admin, "Agent", worker_grants(&project)).await;
    assert_eq!(
        app.patch(&agent, "/me", json!({ "display_name": "Agent renamed" })).await.status,
        StatusCode::FORBIDDEN
    );
    let events = app.get(&admin, "/events?after=0").await.body;
    let event = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| {
            event["type"] == "principal.updated"
                && event["data"]["principal_id"] == member.id.as_str()
                && event["data"]["display_name"]["to"] == "Updated member"
        })
        .unwrap();
    assert_eq!(event["actor"]["id"], member.id.as_str());
    assert_eq!(event["data"]["display_name"], json!({ "from": "Member", "to": "Updated member" }));
}

#[tokio::test]
async fn repeated_login_failures_are_throttled() {
    let app = App::new();
    app.organization("Acme", "admin@acme.test").await;
    for _ in 0..10 {
        let reply = app
            .post(&ANON, "/auth/login", json!({ "email": "admin@acme.test", "password": "wrong password" }))
            .await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    }
    let blocked = app
        .post(&ANON, "/auth/login", json!({ "email": "admin@acme.test", "password": PASSWORD }))
        .await;
    assert_eq!(blocked.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn cookie_requests_from_other_origins_are_rejected() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let body = json!({ "key": "XX", "name": "x", "stages": [] });
    let evil = app
        .request(&admin, axum::http::Method::POST, "/projects", Some(body.clone()), &[("origin", "http://evil.test")])
        .await;
    assert_eq!(evil.code(), "origin_rejected");
    let same = app
        .request(&admin, axum::http::Method::POST, "/projects", Some(body), &[("origin", "http://kanban.test")])
        .await;
    assert_eq!(same.status, StatusCode::UNPROCESSABLE_ENTITY, "reaches validation");
}

#[tokio::test]
async fn bearer_token_needs_no_browser_session_and_can_be_revoked() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let agent = app.agent(&admin, "A", worker_grants(&project)).await;
    assert_eq!(app.get(&agent, "/me").await.body["kind"], "agent");

    let tokens = app.get(&admin, &format!("/principals/{}/tokens", agent.id)).await;
    let id = tokens.body["tokens"][0]["id"].as_str().unwrap();
    assert!(!tokens.body.to_string().contains("ank_"), "secrets are never listed");
    assert_eq!(app.delete(&admin, &format!("/tokens/{id}")).await.status, StatusCode::NO_CONTENT);
    let reply = app.get(&agent, "/me").await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(reply.code(), "unauthenticated");
}

#[tokio::test]
async fn permissions_are_distinct_for_each_operation() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let viewer = app
        .agent(&admin, "Viewer", json!([{ "project_id": project.id, "permission": "view" }]))
        .await;
    let task = app.task(&admin, &project, &project.implement, "X").await;
    let key = task["key"].as_str().unwrap();

    let seen = app.view(&viewer, &task).await;
    assert_eq!(seen["work"]["claimable"], true);
    assert_eq!(seen["work"]["has_claim_permission"], false);
    assert_eq!(seen["work"]["can_claim"], false);
    let checks = [
        app.post(&viewer, &format!("/tasks/{key}/work/claim"), json!({})).await,
        app.post(&viewer, "/tasks", json!({ "project_id": project.id, "title": "t" })).await,
        app.patch(&viewer, &format!("/tasks/{key}"), json!({ "version": 1, "title": "t" })).await,
        app.patch(&viewer, &format!("/tasks/{key}"), json!({ "version": 1, "assignee_id": viewer.id })).await,
        app.post(&viewer, &format!("/tasks/{key}/comments"), json!({ "kind": "comment", "body": "x" })).await,
        app.move_to(&viewer, &task, &project.review).await,
    ];
    for reply in checks {
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{:?}", reply.body);
        assert_eq!(reply.code(), "permission_denied");
    }
}

#[tokio::test]
async fn unauthorized_data_is_not_disclosed() {
    let app = App::new();
    let acme = app.organization("Acme", "admin@acme.test").await;
    let other = app.organization("Other", "admin@other.test").await;
    let project = app.project(&acme, "ACME").await;
    let hidden = app.project(&acme, "SEC").await;
    let agent = app.agent(&acme, "A", worker_grants(&project)).await;
    let task = app.task(&acme, &hidden, &hidden.implement, "Secret plan").await;
    let key = task["key"].as_str().unwrap();
    let id = task["id"].as_str().unwrap();

    // 権限のないプロジェクトも別組織も、存在しないものと同じ応答になる
    for client in [&agent, &other] {
        for reference in [key, id] {
            let reply = app.get(client, &format!("/tasks/{reference}")).await;
            assert_eq!(reply.status, StatusCode::NOT_FOUND, "{reference}");
        }
        let reply = app.post(client, &format!("/tasks/{key}/work/claim"), json!({})).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND);
        assert_eq!(app.get(client, &format!("/projects/{}", hidden.id)).await.status, StatusCode::NOT_FOUND);
        let events = app.get(client, "/events?after=0").await;
        assert!(!events.body.to_string().contains("Secret plan"));
    }
    assert_eq!(app.get(&agent, "/tasks").await.body["total"], 0);
    assert_eq!(app.get(&agent, "/projects").await.body["projects"].as_array().unwrap().len(), 1);
    assert_eq!(app.get(&other, "/principals").await.body["principals"].as_array().unwrap().len(), 1);
    assert_eq!(app.get(&ANON, "/tasks").await.status, StatusCode::UNAUTHORIZED);

    // 他のプロジェクト向けの依頼先にも、見えない主体は指定できない
    let visible = app.task(&acme, &project, &project.implement, "Y").await;
    let foreign = app
        .post(&agent, &format!("/tasks/{}/comments", visible["key"].as_str().unwrap()), json!({
            "kind": "question", "body": "?", "addressee_id": other.id,
        }))
        .await;
    assert_eq!(foreign.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn the_last_human_admin_cannot_be_removed() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let deleted = app.delete(&admin, &format!("/principals/{}", admin.id)).await;
    assert_eq!(deleted.code(), "last_admin");
    let disabled = app
        .patch(&admin, &format!("/principals/{}", admin.id), json!({ "status": "disabled" }))
        .await;
    assert_eq!(disabled.code(), "last_admin");
    assert_eq!(app.get(&admin, "/me").await.status, StatusCode::OK);
}

#[tokio::test]
async fn project_admins_cannot_grant_beyond_their_project() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let other = app.project(&admin, "OTHER").await;
    let lead = app
        .agent(&admin, "Lead", json!([{ "project_id": project.id, "permission": "admin" }]))
        .await;
    let worker = app.agent(&admin, "Worker", json!([])).await;

    let path = format!("/principals/{}/grants", worker.id);
    let inside = app.post(&lead, &path, json!({ "project_id": project.id, "permission": "claim" })).await;
    assert_eq!(inside.status, StatusCode::CREATED, "{:?}", inside.body);
    let outside = app.post(&lead, &path, json!({ "project_id": other.id, "permission": "claim" })).await;
    assert_eq!(outside.status, StatusCode::FORBIDDEN);
    let org_wide = app.post(&lead, &path, json!({ "project_id": null, "permission": "admin" })).await;
    assert_eq!(org_wide.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn file_artifacts_are_private_and_downloaded_safely() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let outsider = app.organization("Other", "o@other.test").await;
    let agent = app.agent(&admin, "A", worker_grants(&project)).await;
    let task = app.task(&admin, &project, &project.implement, "X").await;
    let key = task["key"].as_str().unwrap();
    let Auth::Bearer(token) = &agent.auth else { unreachable!() };

    let upload = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/tasks/{key}/files?role=artifact&name=report.html"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "text/html")
        .body(Body::from("<script>alert(1)</script>"))
        .unwrap();
    let response = app.router.clone().oneshot(upload).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let id = serde_json::from_slice::<Value>(&bytes).unwrap()["id"].as_str().unwrap().to_string();

    let download = Request::builder()
        .uri(format!("/api/v1/artifacts/{id}/content"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(download).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers();
    assert!(headers[header::CONTENT_DISPOSITION].to_str().unwrap().starts_with("attachment"));
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(
        app.get(&outsider, &format!("/artifacts/{id}/content")).await.status,
        StatusCode::NOT_FOUND
    );

    let script = app
        .post(&agent, &format!("/tasks/{key}/artifacts"), json!({
            "role": "artifact", "name": "x", "url": "javascript:alert(1)",
        }))
        .await;
    assert_eq!(script.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn event_cursor_lets_a_consumer_catch_up_without_gaps() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let start = app.get(&admin, "/events?after=latest").await.body["cursor"].as_i64().unwrap();
    let task = app.task(&admin, &project, &project.implement, "X").await;
    let claim_id = app.claim(&admin, &task).await;
    let key = task["key"].as_str().unwrap();
    app.post(&admin, &format!("/tasks/{key}/work/start"), json!({ "claim_id": claim_id })).await;

    let first = app.get(&admin, &format!("/events?after={start}&limit=2")).await;
    let types: Vec<_> = first.body["events"].as_array().unwrap().iter()
        .map(|event| event["type"].as_str().unwrap().to_string()).collect();
    assert_eq!(types, ["task.created", "work.available"]);
    let cursor = first.body["cursor"].as_i64().unwrap();
    let second = app.get(&admin, &format!("/events?after={cursor}&project={}", project.id)).await;
    let types: Vec<_> = second.body["events"].as_array().unwrap().iter()
        .map(|event| event["type"].as_str().unwrap().to_string()).collect();
    assert_eq!(types, ["work.claimed", "work.unavailable", "work.started"]);
}

async fn next_frame(body: &mut Body) -> Option<String> {
    let frame = tokio::time::timeout(std::time::Duration::from_secs(5), body.frame()).await;
    let frame = frame.expect("stream produced nothing in time")?.unwrap();
    Some(String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap())
}

#[tokio::test]
async fn event_stream_delivers_changes_and_closes_when_the_token_is_revoked() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let agent = app.agent(&admin, "A", worker_grants(&project)).await;
    let Auth::Bearer(token) = &agent.auth else { unreachable!() };

    let request = Request::builder()
        .uri("/api/v1/events/stream?after=latest")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();

    app.task(&admin, &project, &project.implement, "Streamed").await;
    let frame = next_frame(&mut body).await.unwrap();
    assert!(frame.contains("task.created") && frame.contains("id:"), "{frame}");

    // 失効は次のイベントで検知され、それ以降は何も配信されない
    let tokens = app.get(&admin, &format!("/principals/{}/tokens", agent.id)).await;
    let id = tokens.body["tokens"][0]["id"].as_str().unwrap();
    app.delete(&admin, &format!("/tokens/{id}")).await;
    let mut leaked = String::new();
    while let Some(frame) = next_frame(&mut body).await {
        leaked.push_str(&frame);
        if leaked.contains("Secret after revoke") {
            break;
        }
        app.task(&admin, &project, &project.implement, "Secret after revoke").await;
    }
    assert!(!leaked.contains("Secret after revoke"));
}

#[tokio::test]
async fn project_keys_may_be_a_single_letter_and_invalid_keys_explain_the_rule() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "F").await;
    let task = app.task(&admin, &project, &project.implement, "X").await;
    assert_eq!(task["key"], "F-1");
    assert_eq!(app.get(&admin, "/tasks/F-1").await.status, StatusCode::OK);

    for key in ["", "1A", "f", "ABCDEFGHIJK", "A-B"] {
        let reply = app
            .post(&admin, "/projects", json!({
                "key": key,
                "name": "x",
                "stages": [{ "name": "s", "requires_work": true, "claim_permission": "claim", "is_final": false }],
            }))
            .await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{key}");
        assert_eq!(reply.code(), "invalid_project_key", "{key}");
    }
}
