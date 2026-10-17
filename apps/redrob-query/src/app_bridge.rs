//! The Redrob Code app bridge: this app's tools, served to the shared Redrob Code daemon.
//!
//! `redrob-app-bridge` serves the tools over MCP on loopback and registers them with the daemon,
//! so the agent in any Redrob surface (a phone chat included) can list connections, connect, read
//! the schema, run SQL and put text into an editor tab while the app is open. The daemon is
//! optional: when it cannot be reached the failure is logged and the app runs without the bridge.
//!
//! Shape: the tool handlers run on the bridge's own threads, the app state lives on the UI thread.
//! Every handler pushes a [`BridgeRequest`] onto a queue, asks egui for a repaint and blocks on the
//! reply; once per frame [`QueryApp::serve_bridge`] drains the queue and runs each request against
//! the real app state through the same `Workspace`, `ProfilesPanel` and tab functions the panels
//! use. Engine answers (`Pending`) are polled on the following frames and the reply is sent the
//! moment they land, so a slow database holds up the handler thread, never a paint.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use redrob_app_bridge::{AppBridge, AppBridgeConfig, Tool};
use redrob_query_core::{
    ConnectionProfile, DatabaseKind, MetadataKind, MetadataNode, MetadataRequest, Pending,
    QueryRequest, QueryResultPage, Workspace,
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::app::QueryApp;
use crate::editor::QueryTab;
use crate::grid::format_value;
use crate::highlight::Dialect;
use crate::profiles::kind_name;

/// The name the daemon lists the tools under (`app:query`).
const APP_NAME: &str = "query";
/// How long a handler waits for the UI thread (and the engine behind it) before giving up.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);
/// The engine's own deadline for a bridged query: under [`REPLY_TIMEOUT`], so the engine's error
/// reaches the agent before the handler stops waiting.
const QUERY_TIMEOUT_MS: u64 = 25_000;
/// Rows `run_sql` answers with unless `max_rows` says otherwise.
const DEFAULT_MAX_ROWS: usize = 200;
/// The engine's own ceiling on a page.
const MAX_ROWS: usize = 10_000;

type Reply = Result<String, String>;

/// One tool call waiting for the UI thread.
pub struct BridgeRequest {
    tool: String,
    args: Value,
    reply: Sender<Reply>,
}

/// The last `run_sql` page, for `export_result`.
struct LastResult {
    page: QueryResultPage,
    connection: String,
}

/// A request whose engine answer has not landed yet; polled once per frame.
struct Waiting {
    reply: Sender<Reply>,
    job: Job,
}

enum Job {
    /// `connect`: done when the profiles panel reports the connection up (or gone).
    Connect { id: Uuid, name: String },
    /// `list_tables` / `describe_table`: one or two metadata loads in a row.
    Metadata(MetadataStep),
    /// `run_sql`.
    Query {
        pending: Pending<QueryResultPage>,
        limit: usize,
        connection: String,
    },
}

/// What a metadata step does with the nodes it loaded.
type MetadataThen = Box<dyn FnOnce(&Workspace, Vec<MetadataNode>) -> StepOutcome>;

/// A metadata load and what to do with its nodes: answer, or load once more.
struct MetadataStep {
    pending: Pending<Vec<MetadataNode>>,
    then: MetadataThen,
}

enum StepOutcome {
    Done(Reply),
    Next(MetadataStep),
}

/// What a tool did on the UI thread: answered, or started something to wait for.
enum Outcome {
    Reply(Reply),
    Wait(Job),
}

/// The bridge as the app holds it: the registration (once the daemon answered), the request
/// queue, the requests still waiting on the engine and the last result.
pub struct Bridge {
    /// The daemon is found or started on a thread so a slow start never delays the window.
    starting: Option<Receiver<Result<AppBridge, String>>>,
    handle: Option<AppBridge>,
    requests: Receiver<BridgeRequest>,
    waiting: Vec<Waiting>,
    last_result: Option<LastResult>,
}

impl Bridge {
    /// Serve the tools and register with the daemon, off the UI thread. The returned bridge is
    /// usable at once; the registration arrives on a later frame (or a logged failure does).
    pub fn start(ctx: &egui::Context) -> Self {
        let (queue, requests) = mpsc::channel::<BridgeRequest>();
        let tool_list = tools(&queue, ctx);
        let (started, starting) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("redrob-query-app-bridge".into())
            .spawn(move || {
                let result = AppBridge::start(AppBridgeConfig {
                    version: Some(env!("CARGO_PKG_VERSION").into()),
                    ..AppBridgeConfig::new(APP_NAME, tool_list)
                });
                let _ = started.send(result);
            });
        if let Err(error) = spawned {
            log::warn!("app bridge not started: {error}");
        }
        Self {
            starting: Some(starting),
            handle: None,
            requests,
            waiting: Vec::new(),
            last_result: None,
        }
    }

    /// Unregister from the daemon and stop the server. Idempotent.
    pub fn stop(&mut self) {
        self.starting = None;
        if let Some(handle) = self.handle.take() {
            handle.stop();
        }
    }

    fn poll_start(&mut self) {
        let Some(starting) = &self.starting else {
            return;
        };
        match starting.try_recv() {
            Ok(Ok(handle)) => {
                log::info!(
                    "app bridge registered with Redrob Code as app:{APP_NAME} at {}",
                    handle.mcp_url()
                );
                self.handle = Some(handle);
                self.starting = None;
            }
            Ok(Err(error)) => {
                log::info!("app bridge unavailable, running without Redrob Code: {error}");
                self.starting = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => self.starting = None,
        }
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---- the handler side: push a request, wait for the UI thread ----------------------------------

fn call(queue: &Sender<BridgeRequest>, ctx: &egui::Context, tool: &str, args: Value) -> Reply {
    let (reply, answer) = mpsc::channel();
    queue
        .send(BridgeRequest {
            tool: tool.to_owned(),
            args,
            reply,
        })
        .map_err(|_| "Redrob Query is shutting down".to_owned())?;
    ctx.request_repaint();
    match answer.recv_timeout(REPLY_TIMEOUT) {
        Ok(reply) => reply,
        Err(_) => Err(format!(
            "{tool}: Redrob Query did not answer within {} s",
            REPLY_TIMEOUT.as_secs()
        )),
    }
}

fn tool(
    queue: &Sender<BridgeRequest>,
    ctx: &egui::Context,
    name: &'static str,
    description: &str,
    schema: Value,
) -> Tool {
    let queue = queue.clone();
    let ctx = ctx.clone();
    Tool::new(name, description, schema, move |args| {
        call(&queue, &ctx, name, args)
    })
}

/// The `connection` argument every connection-bound tool takes.
fn connection_property() -> Value {
    json!({
        "type": "string",
        "description": "Connection id or name from list_connections; defaults to the connection the app has active."
    })
}

fn tools(queue: &Sender<BridgeRequest>, ctx: &egui::Context) -> Vec<Tool> {
    vec![
        tool(
            queue,
            ctx,
            "list_connections",
            "List the saved database connections as TSV: id, name, driver (PostgreSQL, MySQL, SQLite, MongoDB), connected (yes/no).",
            json!({ "type": "object", "properties": {} }),
        ),
        tool(
            queue,
            ctx,
            "connect",
            "Connect a saved connection by id or name (its password comes from the OS keyring); answers when it is up.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Connection id or name from list_connections." }
                },
                "required": ["id"]
            }),
        ),
        tool(
            queue,
            ctx,
            "list_tables",
            "List the tables, views or collections of a connected database as TSV (schema, name, kind), optionally one schema only.",
            json!({
                "type": "object",
                "properties": {
                    "connection": connection_property(),
                    "schema": { "type": "string", "description": "Only this schema (PostgreSQL / MySQL)." }
                }
            }),
        ),
        tool(
            queue,
            ctx,
            "describe_table",
            "Describe one table, view or collection: its columns as TSV (name, type, nullable); for MongoDB the sampled fields.",
            json!({
                "type": "object",
                "properties": {
                    "connection": connection_property(),
                    "table": { "type": "string", "description": "Table name, optionally schema-qualified as schema.table." }
                },
                "required": ["table"]
            }),
        ),
        tool(
            queue,
            ctx,
            "run_sql",
            "Run one statement on a connected database (SQL, or a Mongo JSON query on MongoDB) and answer the result as TSV: a header row, then at most max_rows rows (default 200), then a summary line with the row count, the time, whether more rows exist and any rows affected.",
            json!({
                "type": "object",
                "properties": {
                    "connection": connection_property(),
                    "sql": { "type": "string", "description": "The statement to run." },
                    "max_rows": { "type": "integer", "minimum": 1, "maximum": MAX_ROWS, "description": "Rows to return at most (default 200)." }
                },
                "required": ["sql"]
            }),
        ),
        tool(
            queue,
            ctx,
            "open_in_editor",
            "Open a new editor tab in Redrob Query with this text, bound to the given (or active) connection, without running it.",
            json!({
                "type": "object",
                "properties": {
                    "sql": { "type": "string", "description": "The text to put in the new tab." },
                    "connection": connection_property()
                },
                "required": ["sql"]
            }),
        ),
        tool(
            queue,
            ctx,
            "export_result",
            "Write the last run_sql result (or, when there is none, the active tab's result grid) to a file as CSV or TSV with a header row; answers the path and the row count.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Where to write the file." },
                    "format": { "type": "string", "enum": ["csv", "tsv"], "description": "csv or tsv (default csv)." }
                },
                "required": ["path"]
            }),
        ),
    ]
}

// ---- the UI side: run requests against the app state --------------------------------------------

impl QueryApp {
    /// Once per frame, after the panels have polled: pick up the registration, run the requests
    /// that arrived, and answer the ones whose engine result has landed.
    pub(crate) fn serve_bridge(&mut self) {
        self.bridge.poll_start();
        while let Ok(request) = self.bridge.requests.try_recv() {
            let outcome = self.bridge_dispatch(&request.tool, &request.args);
            match outcome {
                Outcome::Reply(reply) => {
                    let _ = request.reply.send(reply);
                }
                Outcome::Wait(job) => self.bridge.waiting.push(Waiting {
                    reply: request.reply,
                    job,
                }),
            }
        }
        let waiting = std::mem::take(&mut self.bridge.waiting);
        for Waiting { reply, job } in waiting {
            match self.bridge_advance(job) {
                Ok(answer) => {
                    let _ = reply.send(answer);
                }
                Err(job) => self.bridge.waiting.push(Waiting { reply, job }),
            }
        }
    }

    fn bridge_dispatch(&mut self, tool: &str, args: &Value) -> Outcome {
        match tool {
            "list_connections" => Outcome::Reply(Ok(self.bridge_list_connections())),
            "connect" => self.bridge_connect(args),
            "list_tables" => self.bridge_list_tables(args),
            "describe_table" => self.bridge_describe_table(args),
            "run_sql" => self.bridge_run_sql(args),
            "open_in_editor" => Outcome::Reply(self.bridge_open_in_editor(args)),
            "export_result" => Outcome::Reply(self.bridge_export_result(args)),
            other => Outcome::Reply(Err(format!("unknown tool {other}"))),
        }
    }

    /// Advance one waiting job: `Ok` with the answer when it is done, `Err` with the job otherwise.
    fn bridge_advance(&mut self, job: Job) -> Result<Reply, Job> {
        match job {
            Job::Connect { id, name } => {
                if self.profiles.is_connected(id) {
                    Ok(Ok(format!("connected: {name} ({id})")))
                } else if self.profiles.is_connecting(id) {
                    Err(Job::Connect { id, name })
                } else {
                    let why = self.profiles.status.get(&id).map_or_else(
                        || "see the app's status line".to_owned(),
                        |s| s.message.clone(),
                    );
                    Ok(Err(format!("could not connect {name}: {why}")))
                }
            }
            Job::Metadata(mut step) => match step.pending.poll() {
                None => Err(Job::Metadata(step)),
                Some(Err(error)) => Ok(Err(error.to_string())),
                Some(Ok(nodes)) => match (step.then)(&self.workspace, nodes) {
                    StepOutcome::Done(reply) => Ok(reply),
                    StepOutcome::Next(next) => Err(Job::Metadata(next)),
                },
            },
            Job::Query {
                mut pending,
                limit,
                connection,
            } => match pending.poll() {
                None => Err(Job::Query {
                    pending,
                    limit,
                    connection,
                }),
                Some(Err(error)) => Ok(Err(error.to_string())),
                Some(Ok(page)) => {
                    let text = page_as_tsv(&page, limit, &connection);
                    self.notify(
                        format!(
                            "Redrob Code ran a query on {connection}: {} rows in {} ms",
                            page.stats.rows_returned, page.stats.elapsed_ms
                        ),
                        false,
                    );
                    self.bridge.last_result = Some(LastResult { page, connection });
                    Ok(Ok(text))
                }
            },
        }
    }

    /// The profile a tool means: `args[key]` as an id or a name, else the app's active one, else
    /// the one connection that is up.
    fn bridge_resolve(&self, args: &Value, key: &str) -> Result<ConnectionProfile, String> {
        let wanted = args.get(key).and_then(Value::as_str).map(str::trim);
        if let Some(wanted) = wanted.filter(|w| !w.is_empty()) {
            let by_id = wanted.parse::<Uuid>().ok();
            return self
                .profiles
                .profiles
                .iter()
                .find(|p| by_id == Some(p.id) || p.name.eq_ignore_ascii_case(wanted))
                .cloned()
                .ok_or_else(|| {
                    let known: Vec<&str> = self
                        .profiles
                        .profiles
                        .iter()
                        .map(|p| p.name.as_str())
                        .collect();
                    format!(
                        "no connection with id or name {wanted}; known: {}",
                        known.join(", ")
                    )
                });
        }
        self.active_profile()
            .cloned()
            .or_else(|| {
                self.profiles
                    .profiles
                    .iter()
                    .find(|p| self.profiles.is_connected(p.id))
                    .cloned()
            })
            .ok_or_else(|| {
                "no connection is active; pass connection (an id or name from list_connections)"
                    .to_owned()
            })
    }

    /// Like [`Self::bridge_resolve`], and the connection must be up.
    fn bridge_resolve_connected(
        &self,
        args: &Value,
        key: &str,
    ) -> Result<ConnectionProfile, String> {
        let profile = self.bridge_resolve(args, key)?;
        if !self.profiles.is_connected(profile.id) {
            return Err(format!(
                "{} is not connected; call connect with id {} first",
                profile.name, profile.id
            ));
        }
        Ok(profile)
    }

    fn bridge_list_connections(&self) -> String {
        let mut out = String::from("id\tname\tdriver\tconnected\n");
        for profile in &self.profiles.profiles {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                profile.id,
                tsv_field(&profile.name),
                kind_name(profile.kind),
                if self.profiles.is_connected(profile.id) {
                    "yes"
                } else {
                    "no"
                }
            ));
        }
        out.push_str(&format!("-- {} connections", self.profiles.profiles.len()));
        out
    }

    fn bridge_connect(&mut self, args: &Value) -> Outcome {
        let profile = match self.bridge_resolve(args, "id") {
            Ok(p) => p,
            Err(e) => return Outcome::Reply(Err(e)),
        };
        if !profile.capabilities.connect {
            return Outcome::Reply(Err(profile
                .capabilities
                .unsupported_reason
                .clone()
                .unwrap_or_else(|| format!("{} cannot be connected", profile.name))));
        }
        if self.profiles.is_connected(profile.id) {
            return Outcome::Reply(Ok(format!(
                "already connected: {} ({})",
                profile.name, profile.id
            )));
        }
        self.profiles.connect(&self.workspace, profile.id);
        Outcome::Wait(Job::Connect {
            id: profile.id,
            name: profile.name,
        })
    }

    fn bridge_list_tables(&mut self, args: &Value) -> Outcome {
        let profile = match self.bridge_resolve_connected(args, "connection") {
            Ok(p) => p,
            Err(e) => return Outcome::Reply(Err(e)),
        };
        let schema = args
            .get("schema")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let name = profile.name.clone();
        let pending = self.workspace.load_metadata(MetadataRequest {
            connection_id: profile.id,
            parent_id: None,
        });
        Outcome::Wait(Job::Metadata(MetadataStep {
            pending,
            then: Box::new(move |_: &Workspace, nodes: Vec<MetadataNode>| {
                let mut out = String::from("schema\tname\tkind\n");
                let mut count = 0;
                for node in nodes.iter().filter(|n| {
                    schema.as_ref().is_none_or(|s| {
                        n.attributes
                            .get("schema")
                            .is_some_and(|have| have.eq_ignore_ascii_case(s))
                    })
                }) {
                    count += 1;
                    out.push_str(&format!(
                        "{}\t{}\t{}\n",
                        node.attributes.get("schema").map_or("", String::as_str),
                        tsv_field(&node.name),
                        kind_label(node.kind)
                    ));
                }
                out.push_str(&format!("-- {count} on {name}"));
                if let Some(schema) = &schema {
                    out.push_str(&format!(" in schema {schema}"));
                }
                StepOutcome::Done(Ok(out))
            }),
        }))
    }

    fn bridge_describe_table(&mut self, args: &Value) -> Outcome {
        let profile = match self.bridge_resolve_connected(args, "connection") {
            Ok(p) => p,
            Err(e) => return Outcome::Reply(Err(e)),
        };
        let Some(table) = args
            .get("table")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
        else {
            return Outcome::Reply(Err("table is required".to_owned()));
        };
        let connection_id = profile.id;
        let kind = profile.kind;
        // The roots first (to find the node the engine addresses the table by), then its columns.
        let pending = self.workspace.load_metadata(MetadataRequest {
            connection_id,
            parent_id: None,
        });
        Outcome::Wait(Job::Metadata(MetadataStep {
            pending,
            then: Box::new(move |workspace: &Workspace, roots: Vec<MetadataNode>| {
                let node = match find_table(&roots, &table, kind) {
                    Ok(node) => node,
                    Err(error) => return StepOutcome::Done(Err(error)),
                };
                let pending = workspace.load_metadata(MetadataRequest {
                    connection_id,
                    parent_id: Some(node.id.clone()),
                });
                StepOutcome::Next(MetadataStep {
                    pending,
                    then: Box::new(move |_: &Workspace, columns: Vec<MetadataNode>| {
                        let qualified = match node.attributes.get("schema") {
                            Some(schema) if !schema.is_empty() => {
                                format!("{schema}.{}", node.name)
                            }
                            _ => node.name.clone(),
                        };
                        let mut out = format!(
                            "{qualified} ({})\nname\ttype\tnullable\n",
                            kind_label(node.kind)
                        );
                        for column in &columns {
                            out.push_str(&format!(
                                "{}\t{}\t{}\n",
                                tsv_field(&column.name),
                                column.data_type.as_deref().unwrap_or(""),
                                match column.nullable {
                                    Some(true) => "yes",
                                    Some(false) => "no",
                                    None => "",
                                }
                            ));
                        }
                        out.push_str(&format!("-- {} columns", columns.len()));
                        StepOutcome::Done(Ok(out))
                    }),
                })
            }),
        }))
    }

    fn bridge_run_sql(&mut self, args: &Value) -> Outcome {
        let profile = match self.bridge_resolve_connected(args, "connection") {
            Ok(p) => p,
            Err(e) => return Outcome::Reply(Err(e)),
        };
        let Some(sql) = args
            .get("sql")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned)
        else {
            return Outcome::Reply(Err("sql is required".to_owned()));
        };
        let limit = args
            .get("max_rows")
            .and_then(Value::as_u64)
            .map_or(DEFAULT_MAX_ROWS, |n| {
                usize::try_from(n).unwrap_or(MAX_ROWS).clamp(1, MAX_ROWS)
            });
        let pending = self.workspace.execute_query(QueryRequest {
            connection_id: profile.id,
            query: sql,
            language: Dialect::for_kind(profile.kind).language(),
            parameters: Vec::new(),
            limit,
            offset: 0,
            timeout_ms: QUERY_TIMEOUT_MS,
        });
        Outcome::Wait(Job::Query {
            pending,
            limit,
            connection: profile.name,
        })
    }

    fn bridge_open_in_editor(&mut self, args: &Value) -> Reply {
        let Some(text) = args
            .get("sql")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned)
        else {
            return Err("sql is required".to_owned());
        };
        let before = self.tabs.len();
        match self.bridge_resolve(args, "connection") {
            Ok(profile) => self.open_tab_for(&profile, Some(text)),
            Err(_) => {
                // No connection to bind: a plain SQL tab the person picks a connection for.
                self.tab_counter += 1;
                let title = format!("Query {}", self.tab_counter);
                self.push_tab(QueryTab::new(title, text, None));
            }
        }
        if self.tabs.len() == before {
            return Err("could not open a tab: see the app's status line".to_owned());
        }
        let tab = &self.tabs[self.active];
        let connection = tab
            .connection_id
            .and_then(|id| self.profiles.profile(id))
            .map_or("no connection", |p| p.name.as_str());
        Ok(format!(
            "opened tab {} \"{}\" on {connection}",
            self.active + 1,
            tab.title
        ))
    }

    fn bridge_export_result(&mut self, args: &Value) -> Reply {
        let Some(path) = args
            .get("path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|p| !p.is_empty())
        else {
            return Err("path is required".to_owned());
        };
        let format = args
            .get("format")
            .and_then(Value::as_str)
            .unwrap_or("csv")
            .trim()
            .to_ascii_lowercase();
        let separator = match format.as_str() {
            "csv" => ',',
            "tsv" => '\t',
            other => return Err(format!("format must be csv or tsv, not {other}")),
        };
        let (page, source) = match &self.bridge.last_result {
            Some(last) => (&last.page, format!("run_sql on {}", last.connection)),
            None => match self.tabs.get(self.active).and_then(|t| t.result.as_ref()) {
                Some(grid) => (
                    &grid.page,
                    format!("tab \"{}\"", self.tabs[self.active].title),
                ),
                None => {
                    return Err(
                        "nothing to export: run_sql first, or run a query in the active tab"
                            .to_owned(),
                    );
                }
            },
        };
        let text = page_as_delimited(page, separator);
        std::fs::write(path, text).map_err(|error| format!("could not write {path}: {error}"))?;
        let rows = page.rows.len();
        let columns = page.columns.len();
        self.notify(format!("Redrob Code exported {rows} rows to {path}"), false);
        Ok(format!(
            "wrote {rows} rows x {columns} columns ({source}) as {format} to {path}"
        ))
    }
}

// ---- text helpers ------------------------------------------------------------------------------

fn kind_label(kind: MetadataKind) -> &'static str {
    match kind {
        MetadataKind::Server => "server",
        MetadataKind::Database => "database",
        MetadataKind::Schema => "schema",
        MetadataKind::Table => "table",
        MetadataKind::View => "view",
        MetadataKind::Collection => "collection",
        MetadataKind::Column => "column",
        MetadataKind::Index => "index",
    }
}

/// The root node `table` names: `schema.name` or `name`, case-insensitive when nothing matches
/// exactly; an unqualified name that several schemas have is refused with the candidates.
fn find_table(
    roots: &[MetadataNode],
    table: &str,
    kind: DatabaseKind,
) -> Result<MetadataNode, String> {
    let (schema, name) = match kind {
        DatabaseKind::PostgreSql | DatabaseKind::MySql => match table.split_once('.') {
            Some((schema, name)) => (Some(schema.trim()), name.trim()),
            None => (None, table),
        },
        _ => (None, table),
    };
    let unquote = |s: &str| s.trim_matches(['"', '`', '[', ']']).to_owned();
    let (schema, name) = (schema.map(unquote), unquote(name));
    let matching = |exact: bool| -> Vec<&MetadataNode> {
        roots
            .iter()
            .filter(|node| {
                let name_ok = if exact {
                    node.name == name
                } else {
                    node.name.eq_ignore_ascii_case(&name)
                };
                let schema_ok = schema.as_ref().is_none_or(|s| {
                    node.attributes.get("schema").is_some_and(|have| {
                        if exact {
                            have == s
                        } else {
                            have.eq_ignore_ascii_case(s)
                        }
                    })
                });
                name_ok && schema_ok
            })
            .collect()
    };
    let mut found = matching(true);
    if found.is_empty() {
        found = matching(false);
    }
    match found.as_slice() {
        [] => Err(format!(
            "no table, view or collection named {table}; list_tables shows them"
        )),
        [one] => Ok((*one).clone()),
        many => {
            let names: Vec<String> = many
                .iter()
                .map(|n| match n.attributes.get("schema") {
                    Some(s) => format!("{s}.{}", n.name),
                    None => n.name.clone(),
                })
                .collect();
            Err(format!(
                "{table} is ambiguous; qualify it as one of: {}",
                names.join(", ")
            ))
        }
    }
}

/// One TSV field: tabs and line breaks escaped so a row stays one line.
fn tsv_field(text: &str) -> String {
    text.replace('\t', "\\t")
        .replace('\r', "")
        .replace('\n', "\\n")
}

/// One CSV field: quoted when it holds the separator, a quote or a line break.
fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_owned()
    }
}

/// The page as TSV with a header, then the summary line the agent reads the outcome from.
fn page_as_tsv(page: &QueryResultPage, limit: usize, connection: &str) -> String {
    let mut out = page_as_delimited(page, '\t');
    let returned = page.stats.rows_returned;
    if page.stats.truncated {
        out.push_str(&format!(
            "-- first {returned} rows in {} ms on {connection}; more rows exist (truncated at max_rows {limit}, up to {MAX_ROWS})",
            page.stats.elapsed_ms
        ));
    } else {
        out.push_str(&format!(
            "-- {returned} rows in {} ms on {connection}; complete",
            page.stats.elapsed_ms
        ));
    }
    if page.stats.rows_affected > 0 {
        out.push_str(&format!("; {} rows affected", page.stats.rows_affected));
    }
    out
}

/// The page as delimited text with a header row, every row on one line.
fn page_as_delimited(page: &QueryResultPage, separator: char) -> String {
    let field = |text: &str| -> String {
        if separator == '\t' {
            tsv_field(text)
        } else {
            csv_field(text)
        }
    };
    let sep = separator.to_string();
    let mut out = String::new();
    if !page.columns.is_empty() {
        let header: Vec<String> = page.columns.iter().map(|c| field(&c.name)).collect();
        out.push_str(&header.join(sep.as_str()));
        out.push('\n');
    }
    for row in &page.rows {
        let cells: Vec<String> = row.iter().map(|v| field(&format_value(v))).collect();
        out.push_str(&cells.join(sep.as_str()));
        out.push('\n');
    }
    out
}
