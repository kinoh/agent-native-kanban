//! docs/DRAFT.md 9章の受け入れシナリオ。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn scenario_a_agents_collaborate_on_one_task() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let a = app.agent(&admin, "Agent A", worker_grants(&project)).await;
    let b = app.agent(&admin, "Agent B", worker_grants(&project)).await;
    let task = app.task(&admin, &project, &project.implement, "X").await;
    let key = task["key"].as_str().unwrap();

    // 両者が取得可能なタスクとして発見する
    for agent in [&a, &b] {
        let found = app.get(agent, &format!("/tasks?project={}&can_claim=true", project.id)).await;
        assert_eq!(found.body["tasks"][0]["key"], key);
    }

    let claim_id = app.claim(&a, &task).await;

    // B からは占有中と認識できる
    let seen = app.view(&b, &task).await;
    assert_eq!(seen["work"]["claimable"], false);
    assert_eq!(seen["work"]["occupant"]["id"], a.id.as_str());
    assert_eq!(seen["work"]["reasons"][0]["code"], "occupied_not_started");
    assert!(seen["work"]["claim_id"].is_null(), "the work right must not leak to others");
    let found = app.get(&b, &format!("/tasks?project={}&can_claim=true", project.id)).await;
    assert_eq!(found.body["total"], 0);

    // B の取得は理由付きで拒否される
    let denied = app.post(&b, &format!("/tasks/{key}/work/claim"), json!({})).await;
    assert_eq!(denied.status, StatusCode::CONFLICT);
    assert_eq!(denied.code(), "not_claimable");
    assert_eq!(denied.reason_codes(), ["occupied_not_started"]);

    app.post(&a, &format!("/tasks/{key}/work/start"), json!({ "claim_id": claim_id })).await;
    let done = app
        .post(&a, &format!("/tasks/{key}/work/complete"), json!({
            "claim_id": claim_id,
            "summary": "implemented",
            "evidence": [{ "name": "PR", "url": "https://example.test/pr/1" }],
        }))
        .await;
    assert_eq!(done.status, StatusCode::OK, "{:?}", done.body);

    // 結果は記録され、業務段階は遷移が行われるまで変わらない
    assert_eq!(done.body["stage"]["id"], project.implement.as_str());
    assert_eq!(done.body["work"]["readiness"], "done");
    assert!(done.body["work"]["occupant"].is_null());
    let context = app.get(&b, &format!("/tasks/{key}/context")).await;
    assert_eq!(context.body["comments"][0]["kind"], "completion");
    assert_eq!(context.body["artifacts"][0]["role"], "artifact");
}

#[tokio::test]
async fn scenario_b_agent_asks_a_human_and_work_continues_from_the_record() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let a = app.agent(&admin, "Agent A", worker_grants(&project)).await;
    let b = app.agent(&admin, "Agent B", worker_grants(&project)).await;
    let task = app.task(&admin, &project, &project.implement, "Billing").await;
    let key = task["key"].as_str().unwrap();

    let claim_id = app.claim(&a, &task).await;
    app.post(&a, &format!("/tasks/{key}/work/start"), json!({ "claim_id": claim_id })).await;
    let asked = app
        .post(&a, &format!("/tasks/{key}/comments"), json!({
            "kind": "question", "body": "税率はどちらを使う？", "addressee_id": admin.id,
        }))
        .await;
    assert_eq!(asked.status, StatusCode::CREATED, "{:?}", asked.body);
    app.post(&a, &format!("/tasks/{key}/work/pause"), json!({ "claim_id": claim_id })).await;

    // H は依頼と文脈を確認して回答する
    let inbox = app.get(&admin, "/me/inbox").await;
    assert_eq!(inbox.body["requests"][0]["task"]["key"], key);
    let context = app.get(&admin, &format!("/tasks/{key}/context")).await;
    assert_eq!(context.body["task"]["work"]["activity"], "paused");
    assert_eq!(context.body["task"]["work"]["reasons"][0]["code"], "occupied_paused");
    let answered = app
        .post(&admin, &format!("/tasks/{key}/comments"), json!({
            "kind": "answer", "body": "軽減税率を使う", "parent_id": asked.body["id"],
        }))
        .await;
    assert_eq!(answered.status, StatusCode::CREATED, "{:?}", answered.body);
    assert_eq!(app.get(&admin, "/me/inbox").await.body["requests"].as_array().unwrap().len(), 0);

    // A から B へ引き継ぐ
    let moved = app
        .post(&a, &format!("/tasks/{key}/work/transfer"), json!({
            "claim_id": claim_id, "to": b.id, "note": "税率の判断を反映して続きを実装する",
        }))
        .await;
    assert_eq!(moved.status, StatusCode::OK, "{:?}", moved.body);

    // B は A との会話なしで経緯を取得して続行できる
    let context = app.get(&b, &format!("/tasks/{key}/context")).await;
    let bodies: Vec<&str> = context.body["comments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|comment| comment["body"].as_str().unwrap())
        .collect();
    assert_eq!(bodies, [
        "税率はどちらを使う？",
        "軽減税率を使う",
        "税率の判断を反映して続きを実装する",
    ]);
    let new_claim = context.body["task"]["work"]["claim_id"].as_str().unwrap();
    let resumed =
        app.post(&b, &format!("/tasks/{key}/work/start"), json!({ "claim_id": new_claim })).await;
    assert_eq!(resumed.body["work"]["activity"], "working");
    assert_eq!(context.body["claims"][0]["end_reason"], "transferred");
}

#[tokio::test]
async fn scenario_c_review_stage_keeps_stage_and_work_separate() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let reviewer = app
        .agent(&admin, "Agent B", json!([
            { "project_id": project.id, "permission": "review" },
        ]))
        .await;
    let worker = app.agent(&admin, "Agent A", worker_grants(&project)).await;
    let approver = app
        .agent(&admin, "Approver", json!([
            { "project_id": project.id, "permission": "approve" },
        ]))
        .await;
    let task = app.task(&admin, &project, &project.implement, "X").await;
    let key = task["key"].as_str().unwrap();

    let moved = app.move_to(&admin, &task, &project.review).await;
    assert_eq!(moved.status, StatusCode::OK, "{:?}", moved.body);

    // レビュー作業が取得可能になるが、レビュー権限のない主体には取得できない
    let seen = app.view(&reviewer, &task).await;
    assert_eq!(seen["work"]["can_claim"], true);
    let denied = app.post(&worker, &format!("/tasks/{key}/work/claim"), json!({})).await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    assert_eq!(denied.body["error"]["details"]["required_permission"], "review");

    let claim_id = app.claim(&reviewer, &task).await;
    let started = app
        .post(&reviewer, &format!("/tasks/{key}/work/start"), json!({ "claim_id": claim_id }))
        .await;
    assert_eq!(started.body["stage"]["name"], "Review");
    assert_eq!(started.body["work"]["is_working"], true);
    assert_eq!(started.body["work"]["occupant"]["id"], reviewer.id.as_str());

    // 占有中は段階を動かせず、未完了のままでも承認できない
    assert_eq!(app.move_to(&approver, &task, &project.done).await.code(), "task_occupied");
    app.post(&reviewer, &format!("/tasks/{key}/comments"), json!({
        "kind": "review_result", "body": "問題なし",
    }))
    .await;
    let done = app
        .post(&reviewer, &format!("/tasks/{key}/work/complete"), json!({
            "claim_id": claim_id, "summary": "レビュー完了",
        }))
        .await;
    assert_eq!(done.body["stage"]["name"], "Review");

    // 承認権限でだけ次の段階へ進む
    let refused = app.move_to(&reviewer, &task, &project.done).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(refused.body["error"]["details"]["required_permission"], "approve");
    let approved = app.move_to(&approver, &task, &project.done).await;
    assert_eq!(approved.status, StatusCode::OK, "{:?}", approved.body);
    assert_eq!(approved.body["stage"]["name"], "Done");

    // 履歴で段階の変更と作業の変更を区別できる
    let history = app.get(&admin, &format!("/tasks/{key}/history")).await;
    let stage_events: Vec<_> = history.body["history"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["category"] == "stage")
        .collect();
    assert_eq!(stage_events.len(), 2);
    assert_eq!(stage_events[1]["actor"]["id"], approver.id.as_str());
}

#[tokio::test]
async fn scenario_c_transition_requires_completed_work() {
    let app = App::new();
    let admin = app.organization("Acme", "admin@acme.test").await;
    let project = app.project(&admin, "ACME").await;
    let task = app.task(&admin, &project, &project.review, "X").await;
    let refused = app.move_to(&admin, &task, &project.done).await;
    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert_eq!(refused.code(), "transition_condition_unmet");
}

#[tokio::test]
async fn scenario_d_agent_survives_its_creator() {
    let app = App::new();
    let founder = app.organization("Acme", "founder@acme.test").await;
    let project = app.project(&founder, "ACME").await;
    let agent = app.agent(&founder, "Agent A", worker_grants(&project)).await;
    let task = app.task(&founder, &project, &project.implement, "Long running").await;
    let assigned = app
        .patch(&founder, &format!("/tasks/{}", task["key"].as_str().unwrap()), json!({
            "version": task["version"],
            "assignee_id": agent.id,
            "responsible_id": agent.id,
        }))
        .await;
    assert_eq!(assigned.status, StatusCode::OK, "{:?}", assigned.body);
    let claim_id = app.claim(&agent, &task).await;

    // 新しい管理者を迎えてから、作成者が退職する
    let link = app
        .post(&founder, "/signup-links", json!({
            "note": "successor", "expires_in_seconds": 3600, "max_uses": 1,
            "grants": [{ "project_id": null, "permission": "admin" }],
        }))
        .await;
    let successor =
        app.signup(link.body["url"].as_str().unwrap(), "next@acme.test", "Successor").await;
    let deleted = app.delete(&founder, &format!("/principals/{}", founder.id)).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{:?}", deleted.body);
    assert_eq!(app.get(&founder, "/me").await.status, StatusCode::UNAUTHORIZED);

    // エージェントの同一性・権限・担当・占有・履歴は維持される
    let me = app.get(&agent, "/me").await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["id"], agent.id.as_str());
    let seen = app.view(&agent, &task).await;
    assert_eq!(seen["assignee"]["id"], agent.id.as_str());
    assert_eq!(seen["work"]["claim_id"], claim_id.as_str());
    let history = app.get(&agent, &format!("/tasks/{}/history", task["key"].as_str().unwrap())).await;
    assert_eq!(history.body["history"][0]["actor"]["display_name"], "Admin");

    // 新しい管理者が設定を変更できる。実行系の交換はトークンの差し替えで、主体は変わらない
    let renamed = app
        .patch(&successor, &format!("/principals/{}", agent.id), json!({
            "description": "runs on the new platform",
        }))
        .await;
    assert_eq!(renamed.status, StatusCode::OK, "{:?}", renamed.body);
    let token = app
        .post(&successor, &format!("/principals/{}/tokens", agent.id), json!({ "name": "new runtime" }))
        .await;
    let replacement = Client {
        id: agent.id.clone(),
        auth: Auth::Bearer(token.body["token"].as_str().unwrap().to_string()),
    };
    let key = task["key"].as_str().unwrap();
    let started = app
        .post(&replacement, &format!("/tasks/{key}/work/start"), json!({ "claim_id": claim_id }))
        .await;
    assert_eq!(started.status, StatusCode::OK, "{:?}", started.body);
    assert_eq!(started.body["work"]["occupant"]["id"], agent.id.as_str());
}
