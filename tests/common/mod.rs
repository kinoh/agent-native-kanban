#![allow(dead_code)]

use agent_native_kanban::api::admin::{SignupGrant, create_link};
use agent_native_kanban::api::{AppState, router};
use agent_native_kanban::db::{Db, new_id, now};
use agent_native_kanban::model::Permission;
use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

pub const PASSWORD: &str = "correct horse battery";

pub struct App {
    pub router: Router,
    pub db: Db,
    pub state: AppState,
}

/// API を呼ぶ主体。トークンでも Cookie でも同じ経路を通る。
#[derive(Clone)]
pub struct Client {
    pub id: String,
    pub auth: Auth,
}

#[derive(Clone)]
pub enum Auth {
    Bearer(String),
    Cookie(String),
    None,
}

pub struct Reply {
    pub status: StatusCode,
    pub body: Value,
    pub set_cookie: Option<String>,
}

impl Reply {
    pub fn code(&self) -> &str {
        self.body["error"]["code"].as_str().unwrap_or("")
    }

    pub fn reason_codes(&self) -> Vec<String> {
        self.body["error"]["details"]["reasons"]
            .as_array()
            .map(|reasons| {
                reasons.iter().map(|r| r["code"].as_str().unwrap().to_string()).collect()
            })
            .unwrap_or_default()
    }
}

pub const ANON: Client = Client { id: String::new(), auth: Auth::None };

impl App {
    pub fn new() -> Self {
        let db = Db::open_in_memory().unwrap();
        let state = AppState::new(db.clone(), "http://kanban.test").unwrap();
        Self { router: router(state.clone(), None), db, state }
    }

    pub async fn request(
        &self,
        client: &Client,
        method: Method,
        path: &str,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> Reply {
        let mut builder = Request::builder().method(method).uri(format!("/api/v1{path}"));
        match &client.auth {
            Auth::Bearer(token) => {
                builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
            }
            Auth::Cookie(cookie) => builder = builder.header(header::COOKIE, cookie),
            Auth::None => {}
        }
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let request = match body {
            Some(body) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        };
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let set_cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .map(|value| value.to_str().unwrap().to_string());
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Reply { status, body, set_cookie }
    }

    pub async fn get(&self, client: &Client, path: &str) -> Reply {
        self.request(client, Method::GET, path, None, &[]).await
    }

    pub async fn post(&self, client: &Client, path: &str, body: Value) -> Reply {
        self.request(client, Method::POST, path, Some(body), &[]).await
    }

    pub async fn patch(&self, client: &Client, path: &str, body: Value) -> Reply {
        self.request(client, Method::PATCH, path, Some(body), &[]).await
    }

    pub async fn put(&self, client: &Client, path: &str, body: Value) -> Reply {
        self.request(client, Method::PUT, path, Some(body), &[]).await
    }

    pub async fn delete(&self, client: &Client, path: &str) -> Reply {
        self.request(client, Method::DELETE, path, None, &[]).await
    }

    /// 組織と最初の管理者を、bootstrap と同じ経路で作る。
    pub async fn organization(&self, name: &str, email: &str) -> Client {
        let name = name.to_string();
        let public_url = self.state.public_url.clone();
        let link = self
            .db
            .write_blocking(|conn| {
                let org_id = new_id();
                conn.execute(
                    "INSERT INTO organizations (id, name, created_at) VALUES (?1, ?2, ?3)",
                    (&org_id, &name, now()),
                )?;
                create_link(
                    conn,
                    &org_id,
                    None,
                    &public_url,
                    "first administrator",
                    3600,
                    1,
                    &[SignupGrant { project_id: None, permission: Permission::Admin }],
                )
            })
            .unwrap();
        self.signup(link["url"].as_str().unwrap(), email, "Admin").await
    }

    pub async fn signup(&self, url: &str, email: &str, name: &str) -> Client {
        let token = url.rsplit('/').next().unwrap();
        let reply = self
            .post(&ANON, &format!("/signup/{token}"), json!({
                "email": email, "password": PASSWORD, "display_name": name,
            }))
            .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
        let cookie = reply.set_cookie.unwrap();
        Client {
            id: reply.body["principal_id"].as_str().unwrap().to_string(),
            auth: Auth::Cookie(cookie.split(';').next().unwrap().to_string()),
        }
    }

    /// 権限付きのエージェントを登録し、そのトークンで動くクライアントを返す。
    pub async fn agent(&self, admin: &Client, name: &str, grants: Value) -> Client {
        let created =
            self.post(admin, "/principals", json!({ "display_name": name, "grants": grants })).await;
        assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
        let id = created.body["id"].as_str().unwrap().to_string();
        let token = self
            .post(admin, &format!("/principals/{id}/tokens"), json!({ "name": "test" }))
            .await;
        assert_eq!(token.status, StatusCode::CREATED, "{:?}", token.body);
        Client { id, auth: Auth::Bearer(token.body["token"].as_str().unwrap().to_string()) }
    }

    /// 実装 → レビュー → 完了 のプロジェクト。レビューはレビュー権限で取得し、承認で次へ進む。
    pub async fn project(&self, admin: &Client, key: &str) -> Project {
        let reply = self
            .post(admin, "/projects", json!({
                "key": key,
                "name": format!("Project {key}"),
                "stages": [
                    { "name": "Backlog", "requires_work": false, "claim_permission": "claim", "is_final": false },
                    { "name": "Implement", "requires_work": true, "claim_permission": "claim", "is_final": false },
                    { "name": "Review", "requires_work": true, "claim_permission": "review", "is_final": false },
                    { "name": "Done", "requires_work": false, "claim_permission": "claim", "is_final": true },
                ],
            }))
            .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
        let stage = |index: usize| reply.body["stages"][index]["id"].as_str().unwrap().to_string();
        let project = Project {
            id: reply.body["id"].as_str().unwrap().to_string(),
            backlog: stage(0),
            implement: stage(1),
            review: stage(2),
            done: stage(3),
        };
        let rules = self
            .put(admin, &format!("/projects/{}/transition-rules", project.id), json!({ "rules": [
                { "from_stage_id": project.review, "to_stage_id": project.done,
                  "required_permission": "approve", "require_work_done": true },
            ]}))
            .await;
        assert_eq!(rules.status, StatusCode::OK, "{:?}", rules.body);
        project
    }

    pub async fn task(&self, client: &Client, project: &Project, stage: &str, title: &str) -> Value {
        let reply = self
            .post(client, "/tasks", json!({
                "project_id": project.id, "stage_id": stage, "title": title,
            }))
            .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
        reply.body
    }

    pub async fn claim(&self, client: &Client, task: &Value) -> String {
        let key = task["key"].as_str().unwrap();
        let reply = self.post(client, &format!("/tasks/{key}/work/claim"), json!({})).await;
        assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
        reply.body["work"]["claim_id"].as_str().unwrap().to_string()
    }

    pub async fn view(&self, client: &Client, task: &Value) -> Value {
        let reply = self.get(client, &format!("/tasks/{}", task["key"].as_str().unwrap())).await;
        assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
        reply.body
    }

    pub async fn move_to(&self, client: &Client, task: &Value, stage: &str) -> Reply {
        let current = self.view(client, task).await;
        let key = task["key"].as_str().unwrap();
        self.post(client, &format!("/tasks/{key}/stage"), json!({
            "to_stage_id": stage, "version": current["version"],
        }))
        .await
    }
}

pub struct Project {
    pub id: String,
    pub backlog: String,
    pub implement: String,
    pub review: String,
    pub done: String,
}

pub fn worker_grants(project: &Project) -> Value {
    json!([
        { "project_id": project.id, "permission": "view" },
        { "project_id": project.id, "permission": "claim" },
    ])
}
