//! ボード用の一覧に出すレビュー結果は、タスクの閲覧範囲を越えない。

mod common;

use common::*;
use serde_json::json;

#[tokio::test]
async fn task_list_returns_the_latest_free_form_review_result_or_null() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let reviewer = app.agent(&admin, "Reviewer", json!([
        { "project_id": project.id, "permission": "review" },
    ])).await;
    let reviewed = app
        .task(&admin, &project, &project.review, "reviewed")
        .await;
    let unreviewed = app
        .task(&admin, &project, &project.review, "unreviewed")
        .await;
    let key = reviewed["key"].as_str().unwrap();

    let first = app
        .post(
            &reviewer,
            &format!("/tasks/{key}/comments"),
            json!({
                "kind": "review_result", "body": "これは合否ではない任意の記録",
            }),
        )
        .await;
    assert_eq!(
        first.status,
        axum::http::StatusCode::CREATED,
        "{:?}",
        first.body
    );
    let second = app
        .post(
            &reviewer,
            &format!("/tasks/{key}/comments"),
            json!({
                "kind": "review_result", "body": "要確認: API の応答本文を読みやすくする",
            }),
        )
        .await;
    assert_eq!(
        second.status,
        axum::http::StatusCode::CREATED,
        "{:?}",
        second.body
    );
    app.db
        .write_blocking(|conn| {
            conn.execute(
                "UPDATE comments SET created_at = ?1 WHERE id = ?2",
                [
                    "2026-11-01T00:00:00.000Z",
                    first.body["id"].as_str().unwrap(),
                ],
            )?;
            conn.execute(
                "UPDATE comments SET created_at = ?1 WHERE id = ?2",
                [
                    "2026-11-02T00:00:00.000Z",
                    second.body["id"].as_str().unwrap(),
                ],
            )?;
            Ok(())
        })
        .unwrap();

    let list = app
        .get(&admin, &format!("/tasks?project={}", project.id))
        .await;
    let tasks = list.body["tasks"].as_array().unwrap();
    let visible = tasks
        .iter()
        .find(|task| task["key"] == reviewed["key"])
        .unwrap();
    assert_eq!(
        visible["latest_review_result"]["body"],
        "要確認: API の応答本文を読みやすくする"
    );
    assert_eq!(visible["latest_review_result"]["author"]["id"], reviewer.id);
    assert_eq!(
        visible["latest_review_result"]["created_at"],
        "2026-11-02T00:00:00.000Z"
    );
    let no_review = tasks
        .iter()
        .find(|task| task["key"] == unreviewed["key"])
        .unwrap();
    assert!(no_review["latest_review_result"].is_null());
}

#[tokio::test]
async fn task_limited_delegation_receives_review_results_only_for_visible_tasks() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "DEV").await;
    let agent = app.agent(&admin, "agent", worker_grants(&project)).await;
    let visible = app
        .task(&admin, &project, &project.implement, "visible")
        .await;
    let hidden = app
        .task(&admin, &project, &project.implement, "hidden")
        .await;
    for (task, body) in [
        (&visible, "表示してよいレビュー"),
        (&hidden, "漏らしてはいけないレビュー"),
    ] {
        let reply = app
            .post(
                &admin,
                &format!("/tasks/{}/comments", task["key"].as_str().unwrap()),
                json!({
                    "kind": "review_result", "body": body,
                }),
            )
            .await;
        assert_eq!(
            reply.status,
            axum::http::StatusCode::CREATED,
            "{:?}",
            reply.body
        );
    }

    let delegated = app.post(&agent, "/me/delegations", json!({
        "name": "visible task only",
        "expires_at": "2099-01-01T00:00:00.000Z",
        "scopes": [{ "project_id": project.id, "permission": "claim" }],
        "tasks": [visible["key"]],
    })).await;
    assert_eq!(delegated.status, axum::http::StatusCode::CREATED, "{:?}", delegated.body);
    let delegated = Client {
        id: agent.id.clone(),
        auth: Auth::Bearer(delegated.body["token"].as_str().unwrap().to_string()),
    };
    let list = app
        .get(&delegated, &format!("/tasks?project={}", project.id))
        .await;
    assert_eq!(list.body["total"], 1);
    assert_eq!(list.body["tasks"][0]["key"], visible["key"]);
    assert_eq!(
        list.body["tasks"][0]["latest_review_result"]["body"],
        "表示してよいレビュー"
    );
    assert!(!list.body.to_string().contains("漏らしてはいけないレビュー"));
}
