use evot::automation::ExecutorCapabilities;
use napi::Result as NapiResult;
use napi_derive::napi;

fn auth() -> NapiResult<evot::auth::AuthState> {
    evot::auth::load_auth()
        .map_err(to_napi)?
        .ok_or_else(|| napi::Error::from_reason("not logged in"))
}

fn json<T: serde::Serialize>(value: &T) -> NapiResult<String> {
    serde_json::to_string(value).map_err(to_napi)
}

struct ExecutorContext {
    id: String,
    name: String,
    capabilities: ExecutorCapabilities,
    /// Chat the device delivers to when a task names no explicit target.
    delivery_target: String,
}

#[derive(serde::Serialize)]
struct DeliveryDefaults {
    /// The Feishu channel is linked. Delivery may still be unavailable when no
    /// default notification chat is configured.
    feishu_ready: bool,
    feishu_target: String,
}

fn executor(auth: &evot::auth::AuthState, env_file: Option<&str>) -> NapiResult<ExecutorContext> {
    let config = evot::conf::Config::load_with_env_file(env_file).map_err(to_napi)?;
    let id = evot::automation::executor_id(&auth.user.id);
    let name = evot::automation::executor_name(&id);
    let delivery_target = config
        .channels
        .feishu
        .as_ref()
        .map(|feishu| feishu.default_chat_id.trim().to_string())
        .unwrap_or_default();
    Ok(ExecutorContext {
        id,
        name,
        capabilities: ExecutorCapabilities::from_channels(&config.channels),
        delivery_target,
    })
}

async fn ensure_executor(
    auth: &evot::auth::AuthState,
    env_file: Option<&str>,
) -> NapiResult<ExecutorContext> {
    let executor = executor(auth, env_file)?;
    evot::automation::register_executor(auth, &executor.id, &executor.name, &executor.capabilities)
        .await
        .map_err(to_napi)?;
    Ok(executor)
}

/// Async so the config/auth file reads never block the CLI's JS thread.
#[napi]
pub async fn task_delivery_defaults(env_file: Option<String>) -> NapiResult<String> {
    let auth = auth()?;
    let executor = executor(&auth, env_file.as_deref())?;
    json(&DeliveryDefaults {
        feishu_ready: executor.capabilities.feishu_ready,
        feishu_target: executor.delivery_target,
    })
}

#[napi]
pub async fn task_list() -> NapiResult<String> {
    json(
        &evot::automation::list_tasks(&auth()?)
            .await
            .map_err(to_napi)?,
    )
}

#[napi]
pub async fn task_get(task_id: String) -> NapiResult<String> {
    json(
        &evot::automation::get_task(&auth()?, &task_id)
            .await
            .map_err(to_napi)?,
    )
}

#[napi]
pub async fn task_create(body_json: String, env_file: Option<String>) -> NapiResult<String> {
    let auth = auth()?;
    let mut body: serde_json::Value = serde_json::from_str(&body_json).map_err(to_napi)?;
    let executor = ensure_executor(&auth, env_file.as_deref()).await?;
    body["executor_id"] = serde_json::json!(executor.id.clone());
    bind_default_delivery(&mut body, &executor)?;
    json(
        &evot::automation::create_task(&auth, &body)
            .await
            .map_err(to_napi)?,
    )
}

#[napi]
pub async fn task_update(
    task_id: String,
    body_json: String,
    env_file: Option<String>,
) -> NapiResult<String> {
    let auth = auth()?;
    let mut body: serde_json::Value = serde_json::from_str(&body_json).map_err(to_napi)?;
    let executor = executor(&auth, env_file.as_deref())?;
    bind_default_delivery(&mut body, &executor)?;
    json(
        &evot::automation::update_task(&auth, &task_id, &body)
            .await
            .map_err(to_napi)?,
    )
}

/// Resolve an omitted delivery target once, here. A task that asks for delivery
/// but has no reachable chat fails now, at creation, instead of running for
/// weeks and failing at delivery time.
fn bind_default_delivery(
    body: &mut serde_json::Value,
    executor: &ExecutorContext,
) -> NapiResult<()> {
    let channel = body
        .get("delivery_channel")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if channel != "feishu" {
        return Ok(());
    }
    let target = body
        .get("delivery_target")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if !target.is_empty() {
        body["delivery_target"] = serde_json::json!(target);
        return Ok(());
    }
    if executor.delivery_target.is_empty() {
        return Err(napi::Error::from_reason(
            "Feishu delivery needs a default notification chat ID. Set one in settings, or give this task an explicit chat ID.",
        ));
    }
    body["delivery_target"] = serde_json::json!(executor.delivery_target.clone());
    Ok(())
}

#[napi]
pub async fn task_delete(task_id: String) -> NapiResult<()> {
    evot::automation::delete_task(&auth()?, &task_id)
        .await
        .map_err(to_napi)
}

#[napi]
pub async fn task_run(task_id: String, request_id: String) -> NapiResult<()> {
    evot::automation::run_task(&auth()?, &task_id, &request_id)
        .await
        .map_err(to_napi)
}

fn to_napi<E: std::fmt::Display>(error: E) -> napi::Error {
    napi::Error::new(napi::Status::GenericFailure, error.to_string())
}
