use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

use agent_native_kanban::api::admin::{SignupGrant, create_link};
use agent_native_kanban::api::{AppState, router};
use agent_native_kanban::db::{Db, new_id, now};
use agent_native_kanban::error::ApiError;
use agent_native_kanban::model::Permission;
use rusqlite::params;
use tracing_subscriber::EnvFilter;

const USAGE: &str = "\
usage:
  agent-native-kanban serve --db PATH --listen ADDR --public-url URL --web-dist DIR
  agent-native-kanban bootstrap --db PATH --public-url URL --org NAME --expires-hours N

serve      start the API and Web UI server
bootstrap  create an organization and print a one-time signup URL for its first administrator

logging is controlled by RUST_LOG (for example RUST_LOG=info)";

fn parse_flags(args: &[String], allowed: &[&str]) -> Result<HashMap<String, String>, String> {
    let mut flags = HashMap::new();
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        let name = flag.strip_prefix("--").filter(|name| allowed.contains(name));
        let name = name.ok_or_else(|| format!("unknown argument: {flag}"))?;
        let value = iter.next().ok_or_else(|| format!("missing value for {flag}"))?;
        flags.insert(name.to_string(), value.clone());
    }
    for name in allowed {
        if !flags.contains_key(*name) {
            return Err(format!("--{name} is required"));
        }
    }
    Ok(flags)
}

async fn serve(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args, &["db", "listen", "public-url", "web-dist"])?;
    let web_dist = PathBuf::from(&flags["web-dist"]);
    if !web_dist.join("index.html").is_file() {
        return Err(format!("{} has no index.html; build the web UI first", web_dist.display()));
    }
    let db = Db::open(PathBuf::from(&flags["db"]).as_path())?;
    let state = AppState::new(db, &flags["public-url"])?;
    let app = router(state, Some(web_dist));
    let listener = tokio::net::TcpListener::bind(&flags["listen"])
        .await
        .map_err(|e| format!("bind {}: {e}", flags["listen"]))?;
    tracing::info!(listen = %flags["listen"], public_url = %flags["public-url"], "server started");
    // SSE の接続は閉じないので、接続の終了を待たずに停止する。
    // 書き込みはトランザクション単位で完結しているので、途中で止めても不整合は残らない。
    tokio::select! {
        result = axum::serve(listener, app) => result.map_err(|e| format!("server: {e}")),
        signal = tokio::signal::ctrl_c() => {
            signal.map_err(|e| format!("shutdown signal: {e}"))?;
            tracing::info!("server stopped by signal");
            Ok(())
        }
    }
}

fn bootstrap(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args, &["db", "public-url", "org", "expires-hours"])?;
    let hours: i64 = flags["expires-hours"]
        .parse()
        .map_err(|_| "--expires-hours must be an integer".to_string())?;
    let db = Db::open(PathBuf::from(&flags["db"]).as_path())?;
    let state = AppState::new(db.clone(), &flags["public-url"])?;
    let org_name = flags["org"].clone();
    let describe = |e: ApiError| format!("{}: {}", e.code, e.message);
    let link = db
        .write_blocking(|conn| {
            let exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM organizations WHERE name = ?1)",
                [&org_name],
                |row| row.get(0),
            )?;
            if exists {
                return Err(ApiError::conflict("organization_exists", "organization already exists"));
            }
            let org_id = new_id();
            conn.execute(
                "INSERT INTO organizations (id, name, created_at) VALUES (?1, ?2, ?3)",
                params![org_id, org_name, now()],
            )?;
            create_link(
                conn,
                &org_id,
                None,
                &state.public_url,
                "first administrator",
                hours * 3600,
                1,
                &[SignupGrant { project_id: None, permission: Permission::Admin }],
            )
        })
        .map_err(describe)?;
    println!("{}", link["url"].as_str().unwrap_or_default());
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.split_first() {
        Some((command, rest)) if command == "serve" => serve(rest).await,
        Some((command, rest)) if command == "bootstrap" => bootstrap(rest),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
