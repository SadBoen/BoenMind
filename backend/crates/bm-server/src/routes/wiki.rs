//! WIKI 应用 REST 面：`/api/wiki/*`（bm-wiki 引擎门面）。
//!
//! 库位置 = 配置工作目录下 `wiki/`（随 working_dir/便携包随身，多 wiki 后置）。
//! 全部端点走全局 auth_middleware（BOENMIND_TOKEN）。错误契约：StoreError::NotFound
//! → 404（status 端点例外：exists=false 引导建库）；Invalid → 400。
//!
//! 写端点语义对齐 xu-wiki 命令（ingest/query-relation add/layers create），
//! 但 GUI 直调 REST 而非 CLI——引擎确定性、JSON 信封精神保留（hints 收敛为
//! 端内 message 字段）。

use axum::{
    Json, Router,
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use bm_wiki::{Layer, StoreError, WikiStore};
use serde::Deserialize;
use serde_json::json;

use crate::{ApiResult, api_error};

/// wiki 库根（working_dir/wiki）。
fn wiki_root(working_dir: &std::path::Path) -> std::path::PathBuf {
    working_dir.join("wiki")
}

/// 打开库（working_dir 读锁短暂持有后释放；store 自带上写锁）。
fn open_store(working_dir: &std::path::Path) -> Result<WikiStore, StoreError> {
    WikiStore::at(wiki_root(working_dir))
}

fn err_http(e: StoreError) -> (StatusCode, Json<serde_json::Value>) {
    match e {
        StoreError::NotFound(m) => api_error(StatusCode::NOT_FOUND, format!("wiki 库或节点不存在: {m}")),
        StoreError::Invalid(m) => api_error(StatusCode::BAD_REQUEST, m),
        StoreError::Io(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("io: {e}")),
    }
}

pub fn router() -> Router<crate::AppState> {
    Router::new()
        .route("/api/wiki/status", get(status))
        .route("/api/wiki/create", post(create))
        .route("/api/wiki/tree", get(tree))
        .route("/api/wiki/node/{uid}", get(read_node).patch(update_node))
        .route("/api/wiki/node/{uid}/patch", post(append_patch))
        .route("/api/wiki/ingest", post(ingest))
        .route("/api/wiki/query", get(query))
        .route("/api/wiki/expand", post(expand))
        .route("/api/wiki/relations/{uid}", get(relations_of))
        .route("/api/wiki/relations", post(add_relation).delete(remove_relation))
        .route("/api/wiki/lists", post(create_list))
        .route("/api/wiki/reports", post(create_report))
        .route("/api/wiki/entities", post(create_entity))
}

// ── status / create ─────────────────────────────────────────────────────────

/// GET /api/wiki/status — 库状态（不存在时 exists=false + root 供建库引导）。
pub async fn status(State(state): crate::SharedState) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let root = wiki_root(&config.working_dir);
    let root_str = root.display().to_string();
    drop(config);
    match WikiStore::at(root) {
        Ok(store) => match store.status() {
            Ok(s) => Ok(Json(json!({ "exists": true, "root": root_str, "counts": s.counts }))),
            Err(e) => Err(err_http(e)),
        },
        Err(_) => Ok(Json(json!({ "exists": false, "root": root_str, "counts": null }))),
    }
}

#[derive(Deserialize)]
pub struct CreateWikiBody {
    #[serde(default = "default_wiki_name")]
    pub name: String,
}

fn default_wiki_name() -> String {
    "my-wiki".into()
}

/// POST /api/wiki/create — 建库（幂等）。
pub async fn create(
    State(state): crate::SharedState,
    Json(body): Json<CreateWikiBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let root = wiki_root(&config.working_dir);
    drop(config);
    WikiStore::create(&root, &body.name).map_err(err_http)?;
    Ok(Json(json!({ "ok": true, "root": root.display().to_string() })))
}

// ── tree / node ─────────────────────────────────────────────────────────────

/// GET /api/wiki/tree — 四分区节点树。
pub async fn tree(State(state): crate::SharedState) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let store = open_store(&config.working_dir).map_err(err_http)?;
    drop(config);
    let t = store.tree().map_err(err_http)?;
    Ok(Json(json!({ "pages": t.pages, "lists": t.lists, "reports": t.reports, "entities": t.entities })))
}

/// GET /api/wiki/node/{uid} — 节点全文（node_view）。
pub async fn read_node(
    State(state): crate::SharedState,
    AxumPath(uid): AxumPath<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let store = open_store(&config.working_dir).map_err(err_http)?;
    drop(config);
    let node = store.read(&uid).map_err(err_http)?;
    Ok(Json(bm_wiki::node_view(&node)))
}

#[derive(Deserialize)]
pub struct UpdateNodeBody {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
}

/// PATCH /api/wiki/node/{uid} — 学习层修改（Page 拒绝：不可变）。
pub async fn update_node(
    State(state): crate::SharedState,
    AxumPath(uid): AxumPath<String>,
    Json(body): Json<UpdateNodeBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let store = open_store(&config.working_dir).map_err(err_http)?;
    drop(config);
    let node = store
        .update_node(&uid, body.title.as_deref(), body.body.as_deref())
        .map_err(err_http)?;
    Ok(Json(bm_wiki::node_view(&node)))
}

#[derive(Deserialize)]
pub struct PatchBody {
    pub op: String,
    pub delta: String,
}

/// POST /api/wiki/node/{uid}/patch — Page 修订追加（不可变原则的唯一通道）。
pub async fn append_patch(
    State(state): crate::SharedState,
    AxumPath(uid): AxumPath<String>,
    Json(body): Json<PatchBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let store = open_store(&config.working_dir).map_err(err_http)?;
    drop(config);
    let node = store.append_patch(&uid, &body.op, &body.delta).map_err(err_http)?;
    Ok(Json(bm_wiki::node_view(&node)))
}

// ── ingest / query / expand ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct IngestBody {
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub node_path: String,
    /// 源文件绝对路径（可选；md/txt，写 raws 副本 + source_hash 去重）。
    #[serde(default)]
    pub file: Option<String>,
}

/// POST /api/wiki/ingest — 文本/文件导入（Page 群；source_hash 去重）。
pub async fn ingest(
    State(state): crate::SharedState,
    Json(body): Json<IngestBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let store = open_store(&config.working_dir).map_err(err_http)?;
    drop(config);
    let req = bm_wiki::ingest::IngestRequest {
        title: body.title,
        content: body.content,
        node_path: body.node_path,
        source_file: body.file.map(std::path::PathBuf::from),
    };
    let res = store.ingest(req).map_err(err_http)?;
    Ok(Json(serde_json::to_value(&res).unwrap_or(json!({}))))
}

#[derive(Deserialize)]
pub struct QueryParams {
    #[serde(default)]
    pub keywords: String,
}

/// GET /api/wiki/query?keywords=a,b — 检索打分（score 降序）。
pub async fn query(
    State(state): crate::SharedState,
    Query(params): Query<QueryParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let store = open_store(&config.working_dir).map_err(err_http)?;
    drop(config);
    let kws: Vec<String> = params
        .keywords
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let hits = store.query(&kws).map_err(err_http)?;
    Ok(Json(json!({ "keywords": kws, "count": hits.len(), "hits": hits })))
}

#[derive(Deserialize)]
pub struct ExpandBody {
    pub uids: Vec<String>,
}

/// POST /api/wiki/expand — 多节点全文 + 触碰关系 LRU（对齐 xu expand 语义）。
pub async fn expand(
    State(state): crate::SharedState,
    Json(body): Json<ExpandBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let store = open_store(&config.working_dir).map_err(err_http)?;
    drop(config);
    let mut nodes = serde_json::Map::new();
    let mut found = 0;
    for uid in &body.uids {
        match store.read(uid) {
            Ok(node) => {
                let rels = store.relations(uid).unwrap_or_default();
                let rel_uids: Vec<String> = rels.iter().map(|r| r.to_uid.clone()).collect();
                let mut view = serde_json::to_value(bm_wiki::node_view(&node)).unwrap_or(json!({}));
                view["relations"] = serde_json::to_value(&rels).unwrap_or(json!([]));
                // 触碰（写回由 store 内部锁保护）
                let _ = store.touch_relations(uid, &rel_uids.iter().map(String::as_str).collect::<Vec<_>>());
                nodes.insert(uid.clone(), view);
                found += 1;
            }
            Err(_) => {
                nodes.insert(uid.clone(), json!({ "uid": uid, "error": "not found" }));
            }
        }
    }
    Ok(Json(json!({ "nodes": nodes, "found": found, "requested": body.uids.len() })))
}

// ── relations ───────────────────────────────────────────────────────────────

/// GET /api/wiki/relations/{uid} — 节点出边（LRU 顺序）。
pub async fn relations_of(
    State(state): crate::SharedState,
    AxumPath(uid): AxumPath<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let store = open_store(&config.working_dir).map_err(err_http)?;
    drop(config);
    let rels = store.relations(&uid).map_err(err_http)?;
    Ok(Json(bm_wiki::relations::relations_to_json(&rels)))
}

#[derive(Deserialize)]
pub struct RelationBody {
    pub from_uid: String,
    pub to_uid: String,
    pub relation_name: String,
    #[serde(default)]
    pub comment: String,
}

/// POST /api/wiki/relations — 添加/刷新关系（LRU 进队首，满 50 弹尾）。
pub async fn add_relation(
    State(state): crate::SharedState,
    Json(body): Json<RelationBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let store = open_store(&config.working_dir).map_err(err_http)?;
    drop(config);
    store
        .add_relation(&body.from_uid, &body.to_uid, &body.relation_name, &body.comment)
        .map_err(err_http)
        .map(Json)
}

#[derive(Deserialize)]
pub struct RemoveRelationBody {
    pub from_uid: String,
    pub to_uid: String,
    pub relation_name: String,
}

/// DELETE /api/wiki/relations — 删除关系。
pub async fn remove_relation(
    State(state): crate::SharedState,
    Json(body): Json<RemoveRelationBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let store = open_store(&config.working_dir).map_err(err_http)?;
    drop(config);
    let removed = store
        .remove_relation(&body.from_uid, &body.to_uid, &body.relation_name)
        .map_err(err_http)?;
    Ok(Json(json!({ "removed": removed })))
}

// ── layers ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LayerBody {
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub node_path: String,
    /// List 成员 UID 列表。
    #[serde(default)]
    pub members: Vec<String>,
    /// Report 证据链 [{ref_uid, note}]。
    #[serde(default)]
    pub references: Vec<RefBody>,
    /// Entity 回链源 Page。
    #[serde(default)]
    pub source_page: Option<String>,
}

#[derive(Deserialize)]
pub struct RefBody {
    pub ref_uid: String,
    #[serde(default)]
    pub note: String,
}

fn layer_create(body: LayerBody, layer: Layer) -> bm_wiki::layers::LayerCreate {
    bm_wiki::layers::LayerCreate {
        layer,
        title: body.title,
        body: body.body,
        node_path: body.node_path,
        members: body.members,
        references: body
            .references
            .into_iter()
            .map(|r| bm_wiki::RefEntry { ref_uid: r.ref_uid, note: r.note })
            .collect(),
        source_page: body.source_page,
    }
}

/// POST /api/wiki/lists — 创建 List。
pub async fn create_list(
    State(state): crate::SharedState,
    Json(body): Json<LayerBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let store = open_store(&config.working_dir).map_err(err_http)?;
    drop(config);
    let node = store.create_layer(layer_create(body, Layer::List)).map_err(err_http)?;
    Ok(Json(bm_wiki::node_view(&node)))
}

/// POST /api/wiki/reports — 创建 Report（强制 ≥1 证据链）。
pub async fn create_report(
    State(state): crate::SharedState,
    Json(body): Json<LayerBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let store = open_store(&config.working_dir).map_err(err_http)?;
    drop(config);
    let node = store.create_layer(layer_create(body, Layer::Report)).map_err(err_http)?;
    Ok(Json(bm_wiki::node_view(&node)))
}

/// POST /api/wiki/entities — 创建 Entity。
pub async fn create_entity(
    State(state): crate::SharedState,
    Json(body): Json<LayerBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let store = open_store(&config.working_dir).map_err(err_http)?;
    drop(config);
    let node = store.create_layer(layer_create(body, Layer::Entity)).map_err(err_http)?;
    Ok(Json(bm_wiki::node_view(&node)))
}
