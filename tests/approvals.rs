//! 指定承認者による遷移承認と、プロジェクト単位の承認設定。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn configured_approver_can_approve_without_approve_permission_and_reapproval_follows_project_setting() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let approver = app
        .agent(
            &admin,
            "Approver",
            json!([{ "project_id": project.id, "permission": "view" }]),
        )
        .await;
    let outsider = app
        .agent(
            &admin,
            "Outsider",
            json!([{ "project_id": project.id, "permission": "approve" }]),
        )
        .await;

    let rules = app
        .put(
            &admin,
            &format!("/projects/{}/transition-rules", project.id),
            json!({ "rules": [
                { "from_stage_id": project.implement, "to_stage_id": project.review,
                  "required_permission": "edit", "require_work_done": false, "require_approval": true },
                { "from_stage_id": project.review, "to_stage_id": project.implement,
                  "required_permission": "edit", "require_work_done": false, "require_approval": false }
            ] }),
        )
        .await;
    assert_eq!(rules.status, StatusCode::OK, "{:?}", rules.body);

    assert_eq!(
        app.patch(
            &admin,
            &format!("/projects/{}", project.id),
            json!({ "approver_required": true }),
        )
        .await
        .status,
        StatusCode::OK
    );
    let missing = app
        .post(
            &admin,
            "/tasks",
            json!({ "project_id": project.id, "stage_id": project.implement, "title": "missing" }),
        )
        .await;
    assert_eq!(missing.status, StatusCode::UNPROCESSABLE_ENTITY);

    let task = app
        .post(
            &admin,
            "/tasks",
            json!({
                "project_id": project.id, "stage_id": project.implement, "title": "release",
                "approver_id": approver.id,
            }),
        )
        .await;
    assert_eq!(task.status, StatusCode::CREATED, "{:?}", task.body);
    let key = task.body["key"].as_str().unwrap();

    let denied_move = app.move_to(&admin, &task.body, &project.review).await;
    assert_eq!(denied_move.code(), "transition_approval_unmet");
    let denied_approval = app
        .post(
            &outsider,
            &format!("/tasks/{key}/approve"),
            json!({ "to_stage_id": project.review, "version": 1 }),
        )
        .await;
    assert_eq!(denied_approval.status, StatusCode::FORBIDDEN);

    let approved = app
        .post(
            &approver,
            &format!("/tasks/{key}/approve"),
            json!({ "to_stage_id": project.review, "version": 1 }),
        )
        .await;
    assert_eq!(approved.status, StatusCode::OK, "{:?}", approved.body);
    assert_eq!(approved.body["approvals"][0]["approver"]["id"], approver.id);
    assert_eq!(app.move_to(&admin, &task.body, &project.review).await.status, StatusCode::OK);
    assert_eq!(app.move_to(&admin, &task.body, &project.implement).await.status, StatusCode::OK);

    let needs_reapproval = app.move_to(&admin, &task.body, &project.review).await;
    assert_eq!(needs_reapproval.code(), "transition_approval_unmet");
    let current = app.view(&approver, &task.body).await;
    assert_eq!(
        app.post(
            &approver,
            &format!("/tasks/{key}/approve"),
            json!({ "to_stage_id": project.review, "version": current["version"] }),
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(app.move_to(&admin, &task.body, &project.review).await.status, StatusCode::OK);
    assert_eq!(app.move_to(&admin, &task.body, &project.implement).await.status, StatusCode::OK);

    assert_eq!(
        app.patch(
            &admin,
            &format!("/projects/{}", project.id),
            json!({ "reapproval_required": false }),
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(app.move_to(&admin, &task.body, &project.review).await.status, StatusCode::OK);
}

#[tokio::test]
async fn optional_project_allows_unassigned_tasks_through_approval_transitions() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let rules = app
        .put(
            &admin,
            &format!("/projects/{}/transition-rules", project.id),
            json!({ "rules": [{
                "from_stage_id": project.implement, "to_stage_id": project.review,
                "required_permission": "edit", "require_work_done": false, "require_approval": true
            }] }),
        )
        .await;
    assert_eq!(rules.status, StatusCode::OK);
    let task = app.task(&admin, &project, &project.implement, "optional").await;
    assert!(task["approver"].is_null());
    let required = app
        .patch(
            &admin,
            &format!("/projects/{}", project.id),
            json!({ "approver_required": true }),
        )
        .await;
    assert_eq!(required.code(), "approver_required_tasks_missing");
    assert_eq!(app.move_to(&admin, &task, &project.review).await.status, StatusCode::OK);
}

#[tokio::test]
async fn changing_an_approver_requires_assign_permission() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let editor = app
        .agent(
            &admin,
            "Editor",
            json!([{ "project_id": project.id, "permission": "edit" }]),
        )
        .await;
    let task = app.task(&admin, &project, &project.implement, "task").await;
    let reply = app
        .patch(
            &editor,
            &format!("/tasks/{}", task["key"].as_str().unwrap()),
            json!({ "version": 1, "approver_id": editor.id }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
}
