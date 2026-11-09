//! Sentry ProGuard chunk upload protocol and event deobfuscation.
#![allow(clippy::result_large_err)]
use crate::{
    AppState,
    api::projects::{error_response as api_error, json_error},
    domain::{Id, User},
};
use axum::{
    Json,
    extract::{Multipart, Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
const MAX: usize = 32 * 1024 * 1024;
type HttpResult<T> = Result<T, Response>;

async fn user(state: &AppState, mut headers: HeaderMap) -> HttpResult<User> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| json_error(StatusCode::UNAUTHORIZED, "Bearer token required"))?;
    {
        let cookie = format!("{}={token}", crate::auth::SESSION_COOKIE)
            .parse()
            .map_err(|_| json_error(StatusCode::UNAUTHORIZED, "invalid token"))?;
        headers.insert("cookie", cookie);
    }
    crate::auth::session::authenticate_user(state, &headers)
        .await
        .map_err(|error| match error {
            crate::Error::Auth(_) => {
                json_error(StatusCode::UNAUTHORIZED, "authentication required")
            }
            error => api_error(error),
        })
}
async fn team(state: &AppState, headers: HeaderMap, slug: &str) -> HttpResult<Id> {
    let user = user(state, headers).await?;
    let team = state
        .teams
        .find_by_short_id(slug)
        .await
        .map_err(api_error)?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "team not found"))?;
    if !user.instance_role.can_manage_instance() {
        let role = state
            .teams
            .member_role(team.id, user.id)
            .await
            .map_err(api_error)?;
        if role != Some(crate::domain::TeamRole::Admin) {
            return Err(json_error(StatusCode::FORBIDDEN, "team admin required"));
        }
    }
    Ok(team.id)
}
pub async fn options(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
) -> HttpResult<Json<Value>> {
    team(&state, headers, &org).await?;
    Ok(Json(
        json!({"url": format!("{}/api/0/organizations/{org}/chunk-upload/", state.config.base_url.trim_end_matches('/')), "chunkSize": 1048576, "chunksPerRequest": 16, "maxChunks": 16, "maxSize": MAX, "maxWait": 10, "maxRequestSize": MAX, "maxFileSize": MAX, "concurrency": 1, "hashAlgorithm": "sha1", "compression": [], "accept": ["proguard"]}),
    ))
}
pub async fn upload(
    State(state): State<AppState>,
    Path(org): Path<String>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> HttpResult<Json<Value>> {
    let owner = team(&state, headers, &org).await?;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| json_error(StatusCode::BAD_REQUEST, "invalid multipart"))?
    {
        let checksum = field.file_name().unwrap_or_default().to_owned();
        let data = field
            .bytes()
            .await
            .map_err(|_| json_error(StatusCode::BAD_REQUEST, "invalid chunk"))?;
        if data.len() > 1048576 || sha1_smol::Sha1::from(&data[..]).digest().to_string() != checksum
        {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "invalid chunk checksum or size",
            ));
        }
        state
            .debug_files
            .put(owner, "chunk", &checksum, &STANDARD.encode(&data))
            .await
            .map_err(api_error)?;
    }
    Ok(Json(json!({})))
}
#[derive(Deserialize)]
pub struct Assemble {
    name: String,
    chunks: Vec<String>,
}
pub async fn assemble(
    State(state): State<AppState>,
    Path((org, slug)): Path<(String, String)>,
    headers: HeaderMap,
    Json(files): Json<std::collections::BTreeMap<String, Assemble>>,
) -> HttpResult<Json<Value>> {
    let owner = team(&state, headers, &org).await?;
    let project = state
        .projects
        .find_by_slug(&slug)
        .await
        .map_err(api_error)?
        .filter(|p| p.team_id == owner)
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "project not found"))?;
    let mut result = serde_json::Map::new();
    for (checksum, file) in files {
        let mut missing = Vec::new();
        let mut bytes = Vec::new();
        for chunk in file.chunks {
            match state
                .debug_files
                .get(owner, "chunk", &chunk)
                .await
                .map_err(api_error)?
            {
                Some(data) => {
                    let data = STANDARD.decode(data).map_err(|_| {
                        json_error(StatusCode::INTERNAL_SERVER_ERROR, "corrupt chunk")
                    })?;
                    if bytes.len() + data.len() > MAX {
                        return Err(json_error(
                            StatusCode::PAYLOAD_TOO_LARGE,
                            "mapping too large",
                        ));
                    }
                    bytes.extend(data);
                }
                None => missing.push(chunk),
            }
        }
        if !missing.is_empty() {
            result.insert(
                checksum,
                json!({"state":"not_found","missingChunks":missing}),
            );
            continue;
        }
        let mapping = proguard::ProguardMapping::new(&bytes);
        if sha1_smol::Sha1::from(&bytes[..]).digest().to_string() != checksum
            || !mapping.is_valid()
            || std::str::from_utf8(&bytes).is_err()
        {
            result.insert(checksum,json!({"state":"error","detail":"invalid ProGuard mapping or checksum","missingChunks":[]}));
            continue;
        }
        let uuid = file
            .name
            .strip_prefix("/proguard/")
            .and_then(|n| n.strip_suffix(".txt"))
            .and_then(|s| Id::parse_str(s).ok())
            .unwrap_or_else(|| mapping.uuid());
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| json_error(StatusCode::BAD_REQUEST, "mapping must be UTF-8"))?;
        state
            .debug_files
            .put(project.id, "mapping", &uuid.to_string(), text)
            .await
            .map_err(api_error)?;
        result.insert(checksum.clone(),json!({"state":"ok","missingChunks":[],"dif":{"id":uuid.to_string(),"uuid":uuid.to_string(),"debugId":uuid.to_string(),"objectName":file.name,"cpuName":"any","symbolType":"proguard", "sha1":checksum}}));
    }
    Ok(Json(Value::Object(result)))
}

pub async fn deobfuscate(state: &AppState, project: Id, event: &mut Value) -> crate::Result<()> {
    let ids: Vec<String> = event
        .pointer("/debug_meta/images")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|i| i["type"] == "proguard")
        .filter_map(|i| {
            i["uuid"]
                .as_str()
                .and_then(|s| Id::parse_str(s).ok())
                .map(|id| id.to_string())
        })
        .collect();
    for id in ids {
        let Some(mapping) = state.debug_files.get(project, "mapping", &id).await? else {
            continue;
        };
        let mapper = proguard::ProguardMapper::from(mapping.as_str());
        if let Some(exceptions) = event
            .pointer_mut("/exception/values")
            .and_then(Value::as_array_mut)
        {
            for exception in exceptions {
                if let Some(name) = exception["type"]
                    .as_str()
                    .and_then(|n| mapper.remap_class(n))
                {
                    exception["type"] = json!(name);
                }
                remap_frames(&mapper, exception.pointer_mut("/stacktrace/frames"));
            }
        }
        remap_frames(&mapper, event.pointer_mut("/stacktrace/frames"));
        if let Some(threads) = event
            .pointer_mut("/threads/values")
            .and_then(Value::as_array_mut)
        {
            for thread in threads {
                remap_frames(&mapper, thread.pointer_mut("/stacktrace/frames"));
            }
        }
    }
    Ok(())
}
fn remap_frames(mapper: &proguard::ProguardMapper<'_>, frames: Option<&mut Value>) {
    let Some(frames) = frames.and_then(Value::as_array_mut) else {
        return;
    };
    *frames = frames
        .iter()
        .flat_map(|frame| {
            let class = frame["module"].as_str().unwrap_or_default();
            let function = frame["function"].as_str().unwrap_or_default();
            let input = proguard::StackFrame::new(
                class,
                function,
                frame["lineno"].as_u64().unwrap_or(0) as usize,
            );
            let mapped: Vec<_> = mapper
                .remap_frame(&input)
                .map(|mapped| {
                    let mut f = frame.clone();
                    f["module"] = json!(mapped.class());
                    f["function"] = json!(mapped.method());
                    f["lineno"] = json!(mapped.line());
                    if let Some(file) = mapped.file() {
                        f["filename"] = json!(file);
                    }
                    f
                })
                .collect();
            if mapped.is_empty() {
                let mut f = frame.clone();
                if let Some(class) = mapper.remap_class(class) {
                    f["module"] = json!(class);
                }
                vec![f]
            } else {
                mapped.into_iter().rev().collect()
            }
        })
        .collect();
}

/// Legacy sentry-cli / Android Gradle plugin ZIP upload.
pub async fn legacy_upload(
    State(state): State<AppState>,
    Path((org, slug)): Path<(String, String)>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> HttpResult<Json<Value>> {
    use std::io::Read;
    let owner = team(&state, headers, &org).await?;
    let project = state
        .projects
        .find_by_slug(&slug)
        .await
        .map_err(api_error)?
        .filter(|p| p.team_id == owner)
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "project not found"))?;
    let invalid = || {
        json_error(
            StatusCode::BAD_REQUEST,
            "expected ZIP with proguard/<uuid>.txt mappings",
        )
    };
    let mut mappings = Vec::new();
    let mut total = 0usize;
    while let Some(field) = multipart.next_field().await.map_err(|_| invalid())? {
        if field.name() != Some("file") {
            continue;
        }
        let bytes = field.bytes().await.map_err(|_| invalid())?;
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| invalid())?;
        if zip.len() > 100 {
            return Err(invalid());
        }
        for index in 0..zip.len() {
            let file = zip.by_index(index).map_err(|_| invalid())?;
            if file.is_dir() {
                continue;
            }
            let name = file.name().map_err(|_| invalid())?.into_owned();
            let uuid = name
                .trim_start_matches('/')
                .strip_prefix("proguard/")
                .and_then(|s| s.strip_suffix(".txt"))
                .and_then(|s| Id::parse_str(s).ok())
                .ok_or_else(invalid)?;
            let mut bytes = Vec::new();
            file.take((MAX - total + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|_| invalid())?;
            total += bytes.len();
            if total > MAX {
                return Err(json_error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "mapping too large",
                ));
            }
            if !proguard::ProguardMapping::new(&bytes).is_valid() {
                return Err(invalid());
            }
            let checksum = sha1_smol::Sha1::from(&bytes[..]).digest().to_string();
            let text = String::from_utf8(bytes).map_err(|_| invalid())?;
            mappings.push((uuid, name, checksum, text));
        }
    }
    if mappings.is_empty() {
        return Err(invalid());
    }
    let mut result = Vec::new();
    for (uuid, name, checksum, text) in mappings {
        state
            .debug_files
            .put(project.id, "mapping", &uuid.to_string(), &text)
            .await
            .map_err(api_error)?;
        result.push(json!({"id":uuid.to_string(),"uuid":uuid.to_string(),"debugId":uuid.to_string(),"objectName":name,"cpuName":"any","sha1":checksum,"symbolType":"proguard"}));
    }
    Ok(Json(json!(result)))
}
