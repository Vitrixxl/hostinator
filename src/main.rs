use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;
use std::{env, error::Error, net::SocketAddr, sync::Arc, time::Duration};
use subtle::ConstantTimeEq;
use tokio::{net::TcpListener, process::Command, time::timeout};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

type HmacSha256 = Hmac<Sha256>;
type AppResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Debug)]
struct Config {
    hostinator_bin: String,
    webhook_secret: Option<String>,
    allowed_branches: Vec<String>,
    command_timeout: Duration,
}

#[derive(Debug, Deserialize)]
struct GithubPush {
    #[serde(rename = "ref")]
    git_ref: Option<String>,
    repository: Option<GithubRepository>,
}

#[derive(Debug, Deserialize)]
struct GithubRepository {
    clone_url: Option<String>,
    ssh_url: Option<String>,
    html_url: Option<String>,
    git_url: Option<String>,
}

impl Config {
    fn from_env() -> Self {
        let hostinator_bin =
            env::var("HOSTINATOR_BIN").unwrap_or_else(|_| "hostinator".to_string());
        let webhook_secret = env::var("HOSTINATOR_GITHUB_WEBHOOK_SECRET")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let allowed_branches = env::var("HOSTINATOR_ALLOWED_BRANCHES")
            .unwrap_or_else(|_| "main,master".to_string())
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        let command_timeout_secs = env::var("HOSTINATOR_COMMAND_TIMEOUT_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(900);

        Self {
            hostinator_bin,
            webhook_secret,
            allowed_branches,
            command_timeout: Duration::from_secs(command_timeout_secs),
        }
    }

    fn branch_is_allowed(&self, branch: &str) -> bool {
        self.allowed_branches
            .iter()
            .any(|allowed| allowed == branch)
    }
}

#[tokio::main]
async fn main() -> AppResult<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    load_env_file();

    let addr: SocketAddr = env::var("HOSTINATOR_WEBHOOK_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:7878".to_string())
        .parse()?;
    let config = Arc::new(Config::from_env());

    info!(%addr, "starting hostinator webhook");
    let app = Router::new()
        .route("/health", get(health))
        .route("/webhooks/github", post(github_webhook))
        .with_state(config);

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn load_env_file() {
    if let Ok(path) = env::var("HOSTINATOR_ENV_FILE") {
        let _ = dotenvy::from_path(path);
        return;
    }

    let _ = dotenvy::dotenv();
}

async fn health() -> &'static str {
    "ok\n"
}

async fn github_webhook(
    State(config): State<Arc<Config>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(secret) = config.webhook_secret.as_deref() {
        let signature = headers
            .get("x-hub-signature-256")
            .and_then(|value| value.to_str().ok());

        if !signature
            .map(|value| verify_signature(secret, &body, value))
            .unwrap_or(false)
        {
            warn!("rejected github webhook with invalid signature");
            return (StatusCode::UNAUTHORIZED, "invalid signature\n").into_response();
        }
    }

    let event = headers
        .get("x-github-event")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    if event == "ping" {
        return (StatusCode::OK, "pong\n").into_response();
    }

    if event != "push" {
        info!(event, "ignored github event");
        return (StatusCode::ACCEPTED, "ignored event\n").into_response();
    }

    let payload = match serde_json::from_slice::<GithubPush>(&body) {
        Ok(payload) => payload,
        Err(error) => {
            warn!(%error, "invalid github payload");
            return (StatusCode::BAD_REQUEST, "invalid payload\n").into_response();
        }
    };

    let branch = match payload
        .git_ref
        .as_deref()
        .and_then(|git_ref| git_ref.strip_prefix("refs/heads/"))
    {
        Some(branch) if !branch.is_empty() => branch.to_string(),
        _ => return (StatusCode::ACCEPTED, "ignored ref\n").into_response(),
    };

    if !config.branch_is_allowed(&branch) {
        info!(%branch, "ignored branch");
        return (StatusCode::ACCEPTED, "ignored branch\n").into_response();
    }

    let repos = match payload.repository {
        Some(repository) => repository.urls(),
        None => Vec::new(),
    };

    if repos.is_empty() {
        warn!("github payload did not contain repository urls");
        return (StatusCode::BAD_REQUEST, "missing repository\n").into_response();
    }

    let task_config = Arc::clone(&config);
    tokio::spawn(async move {
        if let Err(error) = run_update(task_config, repos, branch).await {
            error!(%error, "hostinator update failed");
        }
    });

    (StatusCode::ACCEPTED, "queued\n").into_response()
}

impl GithubRepository {
    fn urls(self) -> Vec<String> {
        [self.clone_url, self.ssh_url, self.html_url, self.git_url]
            .into_iter()
            .flatten()
            .filter(|value| !value.trim().is_empty())
            .collect()
    }
}

async fn run_update(config: Arc<Config>, repos: Vec<String>, branch: String) -> AppResult<()> {
    info!(%branch, repos = ?repos, "queueing hostinator update");

    let mut command = Command::new(&config.hostinator_bin);
    command.arg("webhook-update").arg("--branch").arg(&branch);
    for repo in repos {
        command.arg("--repo").arg(repo);
    }

    let output = match timeout(config.command_timeout, command.output()).await {
        Ok(result) => result?,
        Err(_) => {
            return Err(format!(
                "hostinator update timed out after {:?}",
                config.command_timeout
            )
            .into())
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !stdout.trim().is_empty() {
        info!(stdout = %stdout.trim(), "hostinator stdout");
    }

    if output.status.success() {
        if !stderr.trim().is_empty() {
            warn!(stderr = %stderr.trim(), "hostinator stderr");
        }
        info!(%branch, "hostinator update completed");
        return Ok(());
    }

    Err(format!(
        "hostinator exited with status {}: {}",
        output.status,
        stderr.trim()
    )
    .into())
}

fn verify_signature(secret: &str, body: &[u8], signature_header: &str) -> bool {
    let Some(signature_hex) = signature_header.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(expected) = hex::decode(signature_hex) else {
        return false;
    };

    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts keys of any length");
    mac.update(body);
    let actual = mac.finalize().into_bytes();
    actual.as_slice().ct_eq(expected.as_slice()).into()
}
