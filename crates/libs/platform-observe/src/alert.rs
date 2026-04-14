// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Alert state machine, evaluation logic, and DB operations.
//!
//! HTTP CRUD handlers (list, create, get, update, delete) stay in the main
//! binary. This module provides the core evaluation engine callable from
//! any binary.

use std::collections::{HashMap, VecDeque};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use platform_types::ApiError;

// ---------------------------------------------------------------------------
// Alert query DSL
// ---------------------------------------------------------------------------

/// Parsed alert query. Format: `metric:<name> [labels:{json}] [agg:<func>] [window:<secs>]`
pub struct AlertQuery {
    pub metric_name: String,
    pub labels: Option<serde_json::Value>,
    pub aggregation: String,
    pub window_secs: i32,
}

pub fn parse_alert_query(query: &str) -> Result<AlertQuery, ApiError> {
    platform_types::validation::check_length("query", query, 1, 1000)?;

    let mut metric_name = None;
    let mut labels = None;
    let mut aggregation = "avg".to_string();
    let mut window_secs: i32 = 300;

    for part in query.split_whitespace() {
        if let Some(name) = part.strip_prefix("metric:") {
            platform_types::validation::check_length("metric_name", name, 1, 255)?;
            metric_name = Some(name.to_string());
        } else if let Some(json) = part.strip_prefix("labels:") {
            labels = Some(
                serde_json::from_str(json)
                    .map_err(|_| ApiError::BadRequest("invalid labels JSON in query".into()))?,
            );
        } else if let Some(agg) = part.strip_prefix("agg:") {
            if !["avg", "sum", "max", "min", "count"].contains(&agg) {
                return Err(ApiError::BadRequest(format!("unknown aggregation: {agg}")));
            }
            aggregation = agg.to_string();
        } else if let Some(w) = part.strip_prefix("window:") {
            window_secs = w
                .parse()
                .map_err(|_| ApiError::BadRequest("window must be an integer (seconds)".into()))?;
            if !(10..=86400).contains(&window_secs) {
                return Err(ApiError::BadRequest(
                    "window must be between 10 and 86400 seconds".into(),
                ));
            }
        }
    }

    let metric_name = metric_name
        .ok_or_else(|| ApiError::BadRequest("query must include metric:<name>".into()))?;

    Ok(AlertQuery {
        metric_name,
        labels,
        aggregation,
        window_secs,
    })
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

pub fn validate_condition(condition: &str) -> Result<(), ApiError> {
    if !["gt", "lt", "eq", "absent"].contains(&condition) {
        return Err(ApiError::BadRequest(
            "condition must be gt, lt, eq, or absent".into(),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// AlertRouter — maps (metric_name, project_id) to matching rule IDs
// ---------------------------------------------------------------------------

/// Key for the routing table: (`metric_name`, `project_id`).
/// `None` `project_id` = platform-level metrics only.
type RouteKey = (String, Option<Uuid>);

/// Read-only index mapping `(metric_name, project_id)` to rule IDs + label filters.
/// Built from DB, rebuilt on rule-change notification.
pub struct AlertRouter {
    routes: HashMap<RouteKey, Vec<(Uuid, Option<serde_json::Value>)>>,
}

impl AlertRouter {
    /// Build router from the database.
    pub async fn from_db(pool: &sqlx::PgPool) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        let rows = sqlx::query(
            "SELECT id, query, project_id FROM alert_rules WHERE enabled = true ORDER BY id",
        )
        .fetch_all(pool)
        .await?;

        let mut routes: HashMap<RouteKey, Vec<(Uuid, Option<serde_json::Value>)>> = HashMap::new();
        for row in &rows {
            let id: Uuid = row.get("id");
            let query_str: String = row.get("query");
            let project_id: Option<Uuid> = row.get("project_id");

            if let Ok(aq) = parse_alert_query(&query_str) {
                let key = (aq.metric_name, project_id);
                routes.entry(key).or_default().push((id, aq.labels));
            }
        }

        Ok(Self { routes })
    }

    /// Build an empty router (no rules loaded).
    pub fn empty() -> Self {
        Self {
            routes: HashMap::new(),
        }
    }

    /// Return rule IDs whose metric name and `project_id` match, and whose label
    /// filter (if any) is a subset of the record's labels.
    pub fn matching_rules(
        &self,
        name: &str,
        labels: &serde_json::Value,
        project_id: Option<Uuid>,
    ) -> Vec<Uuid> {
        let key = (name.to_string(), project_id);
        let Some(candidates) = self.routes.get(&key) else {
            return Vec::new();
        };

        candidates
            .iter()
            .filter(|(_, filter)| match filter {
                None => true,
                Some(f) => labels_subset(f, labels),
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// Whether the router has zero rules.
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }
}

/// Check if every key-value in `filter` exists in `labels`.
fn labels_subset(filter: &serde_json::Value, labels: &serde_json::Value) -> bool {
    let (Some(f_obj), Some(l_obj)) = (filter.as_object(), labels.as_object()) else {
        return false;
    };
    f_obj
        .iter()
        .all(|(k, v)| l_obj.get(k).is_some_and(|lv| lv == v))
}

// ---------------------------------------------------------------------------
// RuleDef — loaded from DB for the stream evaluator
// ---------------------------------------------------------------------------

/// In-memory definition of an alert rule, used by the stream evaluator.
pub(crate) struct RuleDef {
    #[allow(dead_code)]
    pub id: Uuid,
    pub name: String,
    pub condition: String,
    pub threshold: Option<f64>,
    pub for_seconds: i32,
    pub severity: String,
    pub project_id: Option<Uuid>,
    pub aggregation: String,
    pub window_secs: i32,
}

impl RuleDef {
    /// Load all enabled rules from the database.
    pub async fn load_all(pool: &sqlx::PgPool) -> Result<HashMap<Uuid, Self>, sqlx::Error> {
        use sqlx::Row;

        let rows = sqlx::query(
            "SELECT id, name, query, condition, threshold, for_seconds, severity, project_id \
             FROM alert_rules WHERE enabled = true ORDER BY id",
        )
        .fetch_all(pool)
        .await?;

        let mut defs = HashMap::with_capacity(rows.len());
        for row in &rows {
            let id: Uuid = row.get("id");
            let query_str: String = row.get("query");
            if let Ok(aq) = parse_alert_query(&query_str) {
                defs.insert(
                    id,
                    Self {
                        id,
                        name: row.get("name"),
                        condition: row.get("condition"),
                        threshold: row.get("threshold"),
                        for_seconds: row.get("for_seconds"),
                        severity: row.get("severity"),
                        project_id: row.get("project_id"),
                        aggregation: aq.aggregation,
                        window_secs: aq.window_secs,
                    },
                );
            }
        }

        Ok(defs)
    }
}

// ---------------------------------------------------------------------------
// RuleWindow — in-memory sliding window with incremental aggregation
// ---------------------------------------------------------------------------

/// Sliding window of `(timestamp, value)` samples for a single alert rule.
pub(crate) struct RuleWindow {
    samples: VecDeque<(DateTime<Utc>, f64)>,
    window_secs: i32,
    aggregation: String,
    running_sum: f64,
    count: usize,
    pub alert_state: AlertState,
}

impl RuleWindow {
    pub fn new(window_secs: i32, aggregation: &str) -> Self {
        Self {
            samples: VecDeque::new(),
            window_secs,
            aggregation: aggregation.to_string(),
            running_sum: 0.0,
            count: 0,
            alert_state: AlertState {
                first_triggered: None,
                firing: false,
            },
        }
    }

    /// Push a new sample into the window.
    pub fn push(&mut self, ts: DateTime<Utc>, value: f64) {
        self.samples.push_back((ts, value));
        self.running_sum += value;
        self.count += 1;
    }

    /// Evict samples older than `now - window_secs`.
    pub fn evict_expired(&mut self, now: DateTime<Utc>) {
        let cutoff = now - chrono::Duration::seconds(i64::from(self.window_secs));
        while let Some(&(ts, value)) = self.samples.front() {
            if ts < cutoff {
                self.samples.pop_front();
                self.running_sum -= value;
                self.count -= 1;
            } else {
                break;
            }
        }
    }

    /// Compute the aggregate over the current window. Returns `None` if empty.
    pub fn aggregate(&self) -> Option<f64> {
        if self.count == 0 {
            return None;
        }
        match self.aggregation.as_str() {
            #[allow(clippy::cast_precision_loss)]
            "avg" => Some(self.running_sum / self.count as f64),
            "sum" => Some(self.running_sum),
            #[allow(clippy::cast_precision_loss)]
            "count" => Some(self.count as f64),
            "max" => self.samples.iter().map(|(_, v)| *v).reduce(f64::max),
            "min" => self.samples.iter().map(|(_, v)| *v).reduce(f64::min),
            _ => None,
        }
    }

    /// Number of samples currently in the window (used in tests).
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// Stream alert evaluator — consumer group loop
// ---------------------------------------------------------------------------

/// Valkey stream key for alert samples.
pub const ALERT_STREAM_KEY: &str = "alert:samples";
/// Consumer group name.
const ALERT_GROUP: &str = "alert_eval";
/// Pub/sub channel published when alert rules change.
pub const ALERT_RULES_CHANGED_CHANNEL: &str = "alert:rules:changed";

/// Run the streaming alert evaluator (consumer group on `alert:samples`).
///
/// On startup creates the consumer group (idempotent), loads rule definitions,
/// recovers pending entries via XAUTOCLAIM, then enters the main XREADGROUP loop.
pub async fn stream_alert_evaluator(
    pool: sqlx::PgPool,
    valkey: fred::clients::Pool,
    cancel: tokio_util::sync::CancellationToken,
    max_window_secs: u32,
) {
    use fred::interfaces::StreamsInterface;

    tracing::info!("stream alert evaluator starting");

    // Create consumer group (idempotent — ignore BUSYGROUP error)
    let consumer_name = format!("eval-{}", &uuid::Uuid::new_v4().to_string()[..8]);

    let group_result: Result<(), _> = valkey
        .xgroup_create::<(), _, _, _>(
            ALERT_STREAM_KEY,
            ALERT_GROUP,
            "$",
            true, // MKSTREAM
        )
        .await;
    if let Err(ref e) = group_result {
        let msg = e.to_string();
        if !msg.contains("BUSYGROUP") {
            tracing::warn!(error = %e, "failed to create consumer group (may already exist)");
        }
    }

    // Load rule definitions
    let mut rule_defs = match RuleDef::load_all(&pool).await {
        Ok(d) => d,
        Err(e) => {
            tracing::error!(error = %e, "failed to load alert rules on startup");
            HashMap::new()
        }
    };

    let mut windows: HashMap<Uuid, RuleWindow> = HashMap::new();

    // Recover pending entries from dead consumers (XAUTOCLAIM)
    recover_pending(
        &pool,
        &valkey,
        &consumer_name,
        &rule_defs,
        &mut windows,
        max_window_secs,
    )
    .await;

    // Sweep interval for eviction + absent detection
    let mut sweep_interval = tokio::time::interval(std::time::Duration::from_secs(10));
    // Rule reload interval
    let mut rule_reload_interval = tokio::time::interval(std::time::Duration::from_secs(60));

    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                tracing::info!("stream alert evaluator shutting down");
                break;
            }
            entries = xreadgroup_block(&valkey, &consumer_name) => {
                for (entry_id, fields) in entries {
                    process_entry(
                        &pool, &valkey, &mut windows, &rule_defs, &fields, max_window_secs,
                    ).await;
                    // ACK — entry won't be redelivered
                    let _: Result<(), _> = valkey
                        .xack::<(), _, _, _>(ALERT_STREAM_KEY, ALERT_GROUP, &entry_id)
                        .await;
                }
            }
            _ = sweep_interval.tick() => {
                sweep(&pool, &valkey, &mut windows, &rule_defs, max_window_secs).await;
            }
            _ = rule_reload_interval.tick() => {
                if let Ok(new_defs) = RuleDef::load_all(&pool).await {
                    rule_defs = new_defs;
                }
            }
        }
    }
}

/// XREADGROUP with BLOCK 5s, COUNT 100. Returns `Vec<(entry_id, fields)>`.
async fn xreadgroup_block(
    valkey: &fred::clients::Pool,
    consumer: &str,
) -> Vec<(String, HashMap<String, String>)> {
    use fred::interfaces::StreamsInterface;
    use fred::types::streams::XReadResponse;

    let result: Result<XReadResponse<String, String, String, String>, _> = valkey
        .xreadgroup::<XReadResponse<String, String, String, String>, _, _, _, _>(
            ALERT_GROUP,
            consumer,
            Some(100),  // COUNT
            Some(5000), // BLOCK ms
            false,      // NOACK = false
            ALERT_STREAM_KEY,
            ">",
        )
        .await;

    match result {
        Ok(response) => parse_xread_response(response),
        Err(e) => {
            // BLOCK timeout returns empty, not an error — but log real errors
            let msg = format!("{e}");
            if !msg.contains("timed out") && !msg.contains("timeout") {
                tracing::debug!(error = %e, "xreadgroup returned error");
            }
            Vec::new()
        }
    }
}

/// Parse the fred `XReadResponse` into a flat list of `(entry_id, fields)`.
fn parse_xread_response(
    response: fred::types::streams::XReadResponse<String, String, String, String>,
) -> Vec<(String, HashMap<String, String>)> {
    let mut out = Vec::new();
    // XReadResponse is HashMap<stream_key, Vec<(entry_id, HashMap<field, value>)>>
    for (_stream_key, entries) in response {
        for (entry_id, field_map) in entries {
            out.push((entry_id, field_map));
        }
    }
    out
}

/// Process a single stream entry: parse fields, dispatch to `RuleWindow`, evaluate.
async fn process_entry(
    pool: &sqlx::PgPool,
    valkey: &fred::clients::Pool,
    windows: &mut HashMap<Uuid, RuleWindow>,
    rule_defs: &HashMap<Uuid, RuleDef>,
    fields: &HashMap<String, String>,
    max_window_secs: u32,
) {
    // Parse fields: r=<comma-separated rule_ids>, t=<timestamp_ms>, v=<value>
    let Some(rules_str) = fields.get("r") else {
        return;
    };
    let Some(ts_ms_str) = fields.get("t") else {
        return;
    };
    let Some(value_str) = fields.get("v") else {
        return;
    };

    let Ok(ts_ms) = ts_ms_str.parse::<i64>() else {
        return;
    };
    let Ok(value) = value_str.parse::<f64>() else {
        return;
    };

    let ts = DateTime::from_timestamp_millis(ts_ms).unwrap_or_else(Utc::now);
    let now = Utc::now();

    for rule_id_str in rules_str.split(',') {
        let Ok(rule_id) = Uuid::parse_str(rule_id_str.trim()) else {
            continue;
        };

        let Some(def) = rule_defs.get(&rule_id) else {
            continue;
        };

        // Cap window_secs to max_window_secs
        let effective_window = def.window_secs.min(max_window_secs.cast_signed());

        let window = windows
            .entry(rule_id)
            .or_insert_with(|| RuleWindow::new(effective_window, &def.aggregation));

        window.push(ts, value);
        window.evict_expired(now);

        let agg_value = window.aggregate();
        let condition_met = check_condition(&def.condition, def.threshold, agg_value);

        let rule_info = AlertRuleInfo {
            id: rule_id,
            name: &def.name,
            severity: &def.severity,
            project_id: def.project_id,
            for_seconds: def.for_seconds,
        };
        handle_alert_state(
            pool,
            valkey,
            condition_met,
            agg_value,
            now,
            &mut window.alert_state,
            &rule_info,
        )
        .await;
    }
}

/// Periodic sweep: evict expired samples, check absent conditions, trim stream.
async fn sweep(
    pool: &sqlx::PgPool,
    valkey: &fred::clients::Pool,
    windows: &mut HashMap<Uuid, RuleWindow>,
    rule_defs: &HashMap<Uuid, RuleDef>,
    max_window_secs: u32,
) {
    use fred::interfaces::StreamsInterface;

    let now = Utc::now();

    // 1. Evict expired samples and evaluate "absent" conditions
    for (rule_id, window) in windows.iter_mut() {
        window.evict_expired(now);

        if let Some(def) = rule_defs.get(rule_id)
            && def.condition == "absent"
        {
            let agg_value = window.aggregate();
            let condition_met = check_condition(&def.condition, def.threshold, agg_value);

            let rule_info = AlertRuleInfo {
                id: *rule_id,
                name: &def.name,
                severity: &def.severity,
                project_id: def.project_id,
                for_seconds: def.for_seconds,
            };
            handle_alert_state(
                pool,
                valkey,
                condition_met,
                agg_value,
                now,
                &mut window.alert_state,
                &rule_info,
            )
            .await;
        }
    }

    // 2. Check absent rules that have no window at all
    for (rule_id, def) in rule_defs {
        if def.condition == "absent" && !windows.contains_key(rule_id) {
            let window = windows.entry(*rule_id).or_insert_with(|| {
                RuleWindow::new(
                    def.window_secs.min(max_window_secs.cast_signed()),
                    &def.aggregation,
                )
            });

            let rule_info = AlertRuleInfo {
                id: *rule_id,
                name: &def.name,
                severity: &def.severity,
                project_id: def.project_id,
                for_seconds: def.for_seconds,
            };
            handle_alert_state(
                pool,
                valkey,
                true, // absent condition is met (no data)
                None,
                now,
                &mut window.alert_state,
                &rule_info,
            )
            .await;
        }
    }

    // 3. XTRIM MINID ~ to bound stream memory
    let cutoff_ms =
        (now - chrono::Duration::seconds(i64::from(max_window_secs))).timestamp_millis();
    let min_id = format!("{cutoff_ms}-0");
    let _: Result<(), _> = valkey
        .xtrim::<(), _, _>(ALERT_STREAM_KEY, ("MINID", "~", min_id.as_str()))
        .await;
}

/// Recover pending entries from dead consumers via XAUTOCLAIM.
async fn recover_pending(
    pool: &sqlx::PgPool,
    valkey: &fred::clients::Pool,
    consumer: &str,
    rule_defs: &HashMap<Uuid, RuleDef>,
    windows: &mut HashMap<Uuid, RuleWindow>,
    max_window_secs: u32,
) {
    use fred::interfaces::StreamsInterface;
    use fred::types::Value;

    // Claim entries pending > 30s from any consumer
    let result: Result<Value, _> = valkey
        .xautoclaim::<Value, _, _, _, _>(
            ALERT_STREAM_KEY,
            ALERT_GROUP,
            consumer,
            30_000, // min idle ms
            "0-0",  // start from beginning of PEL
            Some(100),
            false, // justid = false (we want full entries)
        )
        .await;

    let entries = match result {
        Ok(value) => parse_xautoclaim_result(value),
        Err(e) => {
            tracing::debug!(error = %e, "xautoclaim failed (stream may not exist yet)");
            Vec::new()
        }
    };

    if !entries.is_empty() {
        tracing::info!(
            count = entries.len(),
            "recovered pending alert stream entries"
        );
    }

    for (entry_id, fields) in &entries {
        process_entry(pool, valkey, windows, rule_defs, fields, max_window_secs).await;
        let _: Result<(), _> = valkey
            .xack::<(), _, _, _>(ALERT_STREAM_KEY, ALERT_GROUP, entry_id.as_str())
            .await;
    }
}

/// Parse XAUTOCLAIM result (array of `[cursor, entries, deleted_ids]`).
fn parse_xautoclaim_result(value: fred::types::Value) -> Vec<(String, HashMap<String, String>)> {
    // XAUTOCLAIM returns: [next_cursor, [[id, [field, value, ...]], ...], [deleted_ids]]
    let fred::types::Value::Array(arr) = value else {
        return Vec::new();
    };
    if arr.len() < 2 {
        return Vec::new();
    }

    let fred::types::Value::Array(entries_arr) = &arr[1] else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for entry in entries_arr {
        let fred::types::Value::Array(parts) = entry else {
            continue;
        };
        if parts.len() < 2 {
            continue;
        }

        // parts[0] = entry ID
        let entry_id = match &parts[0] {
            fred::types::Value::String(s) => s.to_string(),
            fred::types::Value::Bytes(b) => String::from_utf8_lossy(b).to_string(),
            _ => continue,
        };

        // parts[1] = array of [field, value, field, value, ...]
        let fred::types::Value::Array(field_values) = &parts[1] else {
            continue;
        };

        let mut fields = HashMap::new();
        let mut i = 0;
        while i + 1 < field_values.len() {
            let key = value_to_string(&field_values[i]);
            let val = value_to_string(&field_values[i + 1]);
            if let (Some(k), Some(v)) = (key, val) {
                fields.insert(k, v);
            }
            i += 2;
        }

        out.push((entry_id, fields));
    }

    out
}

fn value_to_string(v: &fred::types::Value) -> Option<String> {
    match v {
        fred::types::Value::String(s) => Some(s.to_string()),
        fred::types::Value::Bytes(b) => Some(String::from_utf8_lossy(b).to_string()),
        fred::types::Value::Integer(i) => Some(i.to_string()),
        fred::types::Value::Double(d) => Some(d.to_string()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Alert rule subscriber — rebuilds AlertRouter on rule changes
// ---------------------------------------------------------------------------

/// Subscribe to `alert:rules:changed` and rebuild the `AlertRouter` on each message.
pub async fn alert_rule_subscriber(
    pool: sqlx::PgPool,
    valkey: fred::clients::Pool,
    alert_router: std::sync::Arc<tokio::sync::RwLock<AlertRouter>>,
    cancel: tokio_util::sync::CancellationToken,
) {
    use fred::interfaces::{EventInterface, PubsubInterface};

    let subscriber = valkey.next().clone_new();
    if let Err(e) = subscriber.subscribe(ALERT_RULES_CHANGED_CHANNEL).await {
        tracing::error!(error = %e, "failed to subscribe to alert rules channel");
        return;
    }

    let mut msg_rx = subscriber.message_rx();

    tracing::info!("alert rule subscriber started");

    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                tracing::info!("alert rule subscriber shutting down");
                let _ = subscriber.unsubscribe(ALERT_RULES_CHANGED_CHANNEL).await;
                break;
            }
            msg = msg_rx.recv() => {
                match msg {
                    Ok(_) => {
                        match AlertRouter::from_db(&pool).await {
                            Ok(new_router) => {
                                *alert_router.write().await = new_router;
                                tracing::info!("alert router rebuilt after rule change");
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "failed to rebuild alert router");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "alert rule subscriber channel closed");
                        break;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Alert state machine
// ---------------------------------------------------------------------------

/// In-memory state for an alert rule during evaluation.
pub struct AlertState {
    pub first_triggered: Option<DateTime<Utc>>,
    pub firing: bool,
}

/// Result of evaluating the alert state transition.
pub struct AlertTransition {
    /// Whether the alert should fire (transition to firing).
    pub should_fire: bool,
    /// Whether the alert should resolve (was firing, condition cleared).
    pub should_resolve: bool,
}

/// Metadata about an alert rule, passed to `handle_alert_state`.
pub struct AlertRuleInfo<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub severity: &'a str,
    pub project_id: Option<Uuid>,
    pub for_seconds: i32,
}

/// Pure state machine for alert transitions. Returns what actions to take,
/// and mutates `state` in place.
pub fn next_alert_state(
    state: &mut AlertState,
    condition_met: bool,
    now: DateTime<Utc>,
    for_seconds: i32,
) -> AlertTransition {
    if condition_met {
        if state.first_triggered.is_none() {
            state.first_triggered = Some(now);
        }
        // Safety: first_triggered is guaranteed Some — set above
        let triggered_at = state.first_triggered.expect("set to Some above");
        let held_for = (now - triggered_at).num_seconds();
        if held_for >= i64::from(for_seconds) && !state.firing {
            state.firing = true;
            return AlertTransition {
                should_fire: true,
                should_resolve: false,
            };
        }
        AlertTransition {
            should_fire: false,
            should_resolve: false,
        }
    } else {
        let was_firing = state.firing;
        state.first_triggered = None;
        state.firing = false;
        AlertTransition {
            should_fire: false,
            should_resolve: was_firing,
        }
    }
}

/// Check whether a condition is met given the threshold and value.
pub fn check_condition(condition: &str, threshold: Option<f64>, value: Option<f64>) -> bool {
    match condition {
        "absent" => value.is_none(),
        "gt" => value.is_some_and(|v| threshold.is_some_and(|t| v > t)),
        "lt" => value.is_some_and(|v| threshold.is_some_and(|t| v < t)),
        "eq" => value.is_some_and(|v| threshold.is_some_and(|t| (v - t).abs() < f64::EPSILON)),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// DB operations
// ---------------------------------------------------------------------------

/// Query metric samples for alert evaluation.
pub async fn evaluate_metric(
    pool: &sqlx::PgPool,
    name: &str,
    labels: Option<&serde_json::Value>,
    agg: &str,
    window_secs: i32,
) -> Result<Option<f64>, sqlx::Error> {
    match agg {
        "avg" => {
            sqlx::query_scalar!(
                r#"SELECT AVG(ms.value) FROM metric_samples ms
                   JOIN metric_series ser ON ser.id = ms.series_id
                   WHERE ser.name = $1 AND ($2::jsonb IS NULL OR ser.labels @> $2)
                   AND ms.timestamp > now() - $3 * interval '1 second'"#,
                name,
                labels,
                f64::from(window_secs),
            )
            .fetch_one(pool)
            .await
        }
        "sum" => {
            sqlx::query_scalar!(
                r#"SELECT SUM(ms.value) FROM metric_samples ms
                   JOIN metric_series ser ON ser.id = ms.series_id
                   WHERE ser.name = $1 AND ($2::jsonb IS NULL OR ser.labels @> $2)
                   AND ms.timestamp > now() - $3 * interval '1 second'"#,
                name,
                labels,
                f64::from(window_secs),
            )
            .fetch_one(pool)
            .await
        }
        "max" => {
            sqlx::query_scalar!(
                r#"SELECT MAX(ms.value) FROM metric_samples ms
                   JOIN metric_series ser ON ser.id = ms.series_id
                   WHERE ser.name = $1 AND ($2::jsonb IS NULL OR ser.labels @> $2)
                   AND ms.timestamp > now() - $3 * interval '1 second'"#,
                name,
                labels,
                f64::from(window_secs),
            )
            .fetch_one(pool)
            .await
        }
        "min" => {
            sqlx::query_scalar!(
                r#"SELECT MIN(ms.value) FROM metric_samples ms
                   JOIN metric_series ser ON ser.id = ms.series_id
                   WHERE ser.name = $1 AND ($2::jsonb IS NULL OR ser.labels @> $2)
                   AND ms.timestamp > now() - $3 * interval '1 second'"#,
                name,
                labels,
                f64::from(window_secs),
            )
            .fetch_one(pool)
            .await
        }
        "count" => {
            let count: Option<i64> = sqlx::query_scalar!(
                r#"SELECT COUNT(ms.value) FROM metric_samples ms
                   JOIN metric_series ser ON ser.id = ms.series_id
                   WHERE ser.name = $1 AND ($2::jsonb IS NULL OR ser.labels @> $2)
                   AND ms.timestamp > now() - $3 * interval '1 second'"#,
                name,
                labels,
                f64::from(window_secs),
            )
            .fetch_one(pool)
            .await?;
            #[allow(clippy::cast_precision_loss)]
            Ok(count.map(|c| c as f64))
        }
        _ => Ok(None),
    }
}

/// Insert a "firing" alert event.
pub async fn fire_alert(
    pool: &sqlx::PgPool,
    rule_id: Uuid,
    value: Option<f64>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"INSERT INTO alert_events (rule_id, status, value, message)
        VALUES ($1, 'firing', $2, 'Alert condition met')"#,
        rule_id,
        value,
    )
    .execute(pool)
    .await?;

    tracing::warn!(rule_id = %rule_id, ?value, "alert firing");
    Ok(())
}

/// Resolve the most recent firing event for this rule.
pub async fn resolve_alert(pool: &sqlx::PgPool, rule_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"UPDATE alert_events SET status = 'resolved', resolved_at = now()
        WHERE rule_id = $1 AND status = 'firing' AND resolved_at IS NULL"#,
        rule_id,
    )
    .execute(pool)
    .await?;

    tracing::info!(rule_id = %rule_id, "alert resolved");
    Ok(())
}

/// Handle alert state transition with explicit pool/valkey params.
///
/// Publishes to a dedicated `"alert:fired"` Valkey channel (not `PlatformEvent`).
/// The main binary's eventbus subscribes to this channel separately.
pub async fn handle_alert_state(
    pool: &sqlx::PgPool,
    valkey: &fred::clients::Pool,
    condition_met: bool,
    value: Option<f64>,
    now: DateTime<Utc>,
    alert_state: &mut AlertState,
    rule_info: &AlertRuleInfo<'_>,
) {
    let transition = next_alert_state(alert_state, condition_met, now, rule_info.for_seconds);
    if transition.should_fire {
        if let Err(e) = fire_alert(pool, rule_info.id, value).await {
            tracing::error!(error = %e, rule_id = %rule_info.id, "failed to persist alert firing");
        }
        // Publish to dedicated "alert:fired" channel
        let payload = serde_json::json!({
            "rule_id": rule_info.id,
            "project_id": rule_info.project_id,
            "severity": rule_info.severity,
            "value": value,
            "alert_name": rule_info.name,
        });
        let _ = platform_types::valkey::publish(valkey, "alert:fired", &payload.to_string()).await;
    }
    if transition.should_resolve
        && let Err(e) = resolve_alert(pool, rule_info.id).await
    {
        tracing::error!(error = %e, rule_id = %rule_info.id, "failed to resolve alert");
    }
}

// ---------------------------------------------------------------------------
// Evaluation loop — background task
// ---------------------------------------------------------------------------

/// Run the alert evaluation loop until shutdown.
pub async fn evaluate_alerts_loop(
    pool: sqlx::PgPool,
    valkey: fred::clients::Pool,
    cancel: tokio_util::sync::CancellationToken,
) {
    tracing::info!("alert evaluator started");
    let mut alert_states: std::collections::HashMap<Uuid, AlertState> =
        std::collections::HashMap::new();

    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                tracing::info!("alert evaluator shutting down");
                break;
            }
            () = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                match evaluate_all(&pool, &valkey, &mut alert_states).await {
                    Ok(()) => {}
                    Err(e) => {
                        tracing::error!(error = %e, "alert evaluation cycle failed");
                    }
                }
            }
        }
    }
}

#[allow(clippy::implicit_hasher)]
async fn evaluate_all(
    pool: &sqlx::PgPool,
    valkey: &fred::clients::Pool,
    alert_states: &mut std::collections::HashMap<Uuid, AlertState>,
) -> Result<(), anyhow::Error> {
    use sqlx::Row;

    let rules = sqlx::query(
        "SELECT id, name, query, condition, threshold, for_seconds, severity, project_id \
         FROM alert_rules WHERE enabled = true ORDER BY id LIMIT 500",
    )
    .fetch_all(pool)
    .await?;

    if rules.len() >= 500 {
        tracing::warn!("alert rule limit reached (500) — some rules may not be evaluated");
    }

    let rule_timeout = std::time::Duration::from_secs(10);
    for rule in &rules {
        let rule_id: Uuid = rule.get("id");
        let rule_name: String = rule.get("name");

        match tokio::time::timeout(
            rule_timeout,
            evaluate_one_rule(pool, valkey, alert_states, rule),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!(
                    rule_id = %rule_id, rule_name = %rule_name,
                    error = %e, "alert rule evaluation failed"
                );
            }
            Err(_elapsed) => {
                tracing::warn!(
                    rule_id = %rule_id, rule_name = %rule_name,
                    "alert rule evaluation timed out (10s)"
                );
            }
        }
    }

    Ok(())
}

async fn evaluate_one_rule(
    pool: &sqlx::PgPool,
    valkey: &fred::clients::Pool,
    alert_states: &mut std::collections::HashMap<Uuid, AlertState>,
    rule: &sqlx::postgres::PgRow,
) -> Result<(), anyhow::Error> {
    use sqlx::Row;

    let rule_id: Uuid = rule.get("id");
    let rule_name: String = rule.get("name");
    let rule_query: String = rule.get("query");
    let rule_condition: String = rule.get("condition");
    let rule_threshold: Option<f64> = rule.get("threshold");
    let rule_for_seconds: i32 = rule.get("for_seconds");
    let rule_severity: String = rule.get("severity");
    let rule_project_id: Option<Uuid> = rule.get("project_id");

    let aq = parse_alert_query(&rule_query)?;

    let value = evaluate_metric(
        pool,
        &aq.metric_name,
        aq.labels.as_ref(),
        &aq.aggregation,
        aq.window_secs,
    )
    .await?;

    let condition_met = check_condition(&rule_condition, rule_threshold, value);

    let now = Utc::now();
    let as_entry = alert_states.entry(rule_id).or_insert(AlertState {
        first_triggered: None,
        firing: false,
    });

    let rule_info = AlertRuleInfo {
        id: rule_id,
        name: &rule_name,
        severity: &rule_severity,
        project_id: rule_project_id,
        for_seconds: rule_for_seconds,
    };
    handle_alert_state(
        pool,
        valkey,
        condition_met,
        value,
        now,
        as_entry,
        &rule_info,
    )
    .await;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_query() {
        let q = parse_alert_query("metric:cpu_usage agg:avg window:300").unwrap();
        assert_eq!(q.metric_name, "cpu_usage");
        assert_eq!(q.aggregation, "avg");
        assert_eq!(q.window_secs, 300);
        assert!(q.labels.is_none());
    }

    #[test]
    fn parse_query_with_labels() {
        let q = parse_alert_query(r#"metric:http_errors labels:{"method":"GET"} agg:sum"#).unwrap();
        assert_eq!(q.metric_name, "http_errors");
        assert_eq!(q.aggregation, "sum");
        assert!(q.labels.is_some());
    }

    #[test]
    fn parse_query_defaults() {
        let q = parse_alert_query("metric:mem_usage").unwrap();
        assert_eq!(q.aggregation, "avg");
        assert_eq!(q.window_secs, 300);
    }

    #[test]
    fn parse_query_missing_metric() {
        assert!(parse_alert_query("agg:sum").is_err());
    }

    #[test]
    fn parse_query_invalid_agg() {
        assert!(parse_alert_query("metric:foo agg:median").is_err());
    }

    #[test]
    fn condition_gt() {
        assert!(check_condition("gt", Some(10.0), Some(15.0)));
        assert!(!check_condition("gt", Some(10.0), Some(5.0)));
    }

    #[test]
    fn condition_lt() {
        assert!(check_condition("lt", Some(10.0), Some(5.0)));
        assert!(!check_condition("lt", Some(10.0), Some(15.0)));
    }

    #[test]
    fn condition_eq() {
        assert!(check_condition("eq", Some(10.0), Some(10.0)));
        assert!(!check_condition("eq", Some(10.0), Some(10.1)));
    }

    #[test]
    fn condition_absent() {
        assert!(check_condition("absent", None, None));
        assert!(!check_condition("absent", None, Some(5.0)));
    }

    #[test]
    fn condition_gt_no_value_returns_false() {
        assert!(!check_condition("gt", Some(10.0), None));
    }

    #[test]
    fn condition_gt_no_threshold_returns_false() {
        assert!(!check_condition("gt", None, Some(15.0)));
    }

    #[test]
    fn condition_lt_no_value_returns_false() {
        assert!(!check_condition("lt", Some(10.0), None));
    }

    #[test]
    fn condition_eq_no_value_returns_false() {
        assert!(!check_condition("eq", Some(10.0), None));
    }

    #[test]
    fn condition_unknown_returns_false() {
        assert!(!check_condition("unknown", Some(10.0), Some(15.0)));
        assert!(!check_condition("", Some(10.0), Some(15.0)));
    }

    #[test]
    fn condition_eq_near_epsilon() {
        let v = 10.0;
        let close = v + f64::EPSILON * 0.5;
        assert!(check_condition("eq", Some(v), Some(close)));
    }

    #[test]
    fn condition_nan_returns_false() {
        assert!(!check_condition("gt", Some(10.0), Some(f64::NAN)));
        assert!(!check_condition("lt", Some(10.0), Some(f64::NAN)));
        assert!(!check_condition("eq", Some(10.0), Some(f64::NAN)));
    }

    #[test]
    fn condition_infinity_gt_threshold() {
        assert!(check_condition("gt", Some(10.0), Some(f64::INFINITY)));
    }

    // -- validate_condition --

    #[test]
    fn validate_condition_valid_values() {
        assert!(validate_condition("gt").is_ok());
        assert!(validate_condition("lt").is_ok());
        assert!(validate_condition("eq").is_ok());
        assert!(validate_condition("absent").is_ok());
    }

    #[test]
    fn validate_condition_invalid_values() {
        assert!(validate_condition("gte").is_err());
        assert!(validate_condition("").is_err());
        assert!(validate_condition("GT").is_err());
    }

    // -- parse_alert_query edge cases --

    #[test]
    fn parse_query_window_at_min_boundary() {
        let q = parse_alert_query("metric:cpu window:10").unwrap();
        assert_eq!(q.window_secs, 10);
    }

    #[test]
    fn parse_query_window_at_max_boundary() {
        let q = parse_alert_query("metric:cpu window:86400").unwrap();
        assert_eq!(q.window_secs, 86400);
    }

    #[test]
    fn parse_query_window_below_min_rejected() {
        assert!(parse_alert_query("metric:cpu window:9").is_err());
    }

    #[test]
    fn parse_query_window_above_max_rejected() {
        assert!(parse_alert_query("metric:cpu window:86401").is_err());
    }

    #[test]
    fn parse_query_window_non_integer_rejected() {
        assert!(parse_alert_query("metric:cpu window:abc").is_err());
    }

    #[test]
    fn parse_query_all_aggregations() {
        for agg in &["avg", "sum", "max", "min", "count"] {
            let q = parse_alert_query(&format!("metric:cpu agg:{agg}")).unwrap();
            assert_eq!(q.aggregation, *agg);
        }
    }

    #[test]
    fn parse_query_empty_rejected() {
        assert!(parse_alert_query("").is_err());
    }

    #[test]
    fn parse_query_invalid_labels_json() {
        assert!(parse_alert_query("metric:cpu labels:not-json").is_err());
    }

    // -- alert state machine (next_alert_state) --

    #[test]
    fn alert_inactive_to_pending_on_condition_met() {
        let now = Utc::now();
        let mut state = AlertState {
            first_triggered: None,
            firing: false,
        };
        let t = next_alert_state(&mut state, true, now, 60);
        assert!(state.first_triggered.is_some());
        assert!(!state.firing);
        assert!(!t.should_fire);
        assert!(!t.should_resolve);
    }

    #[test]
    fn alert_pending_to_firing_after_hold_period() {
        let now = Utc::now();
        let mut state = AlertState {
            first_triggered: Some(now - chrono::Duration::seconds(120)),
            firing: false,
        };
        let t = next_alert_state(&mut state, true, now, 60);
        assert!(state.firing);
        assert!(t.should_fire);
        assert!(!t.should_resolve);
    }

    #[test]
    fn alert_pending_resets_when_condition_clears() {
        let now = Utc::now();
        let mut state = AlertState {
            first_triggered: Some(now - chrono::Duration::seconds(30)),
            firing: false,
        };
        let t = next_alert_state(&mut state, false, now, 60);
        assert!(state.first_triggered.is_none());
        assert!(!state.firing);
        assert!(!t.should_fire);
        assert!(!t.should_resolve);
    }

    #[test]
    fn alert_firing_resolves_when_condition_clears() {
        let now = Utc::now();
        let mut state = AlertState {
            first_triggered: Some(now - chrono::Duration::seconds(300)),
            firing: true,
        };
        let t = next_alert_state(&mut state, false, now, 60);
        assert!(!state.firing);
        assert!(state.first_triggered.is_none());
        assert!(!t.should_fire);
        assert!(t.should_resolve);
    }

    #[test]
    fn alert_firing_stays_firing_while_condition_holds() {
        let now = Utc::now();
        let mut state = AlertState {
            first_triggered: Some(now - chrono::Duration::seconds(300)),
            firing: true,
        };
        let t = next_alert_state(&mut state, true, now, 60);
        assert!(state.firing);
        assert!(!t.should_fire);
        assert!(!t.should_resolve);
    }

    #[test]
    fn alert_already_firing_no_duplicate_notification() {
        let now = Utc::now();
        let mut state = AlertState {
            first_triggered: Some(now - chrono::Duration::seconds(600)),
            firing: true,
        };
        for _ in 0..5 {
            let t = next_alert_state(&mut state, true, now, 60);
            assert!(!t.should_fire);
        }
    }

    #[test]
    fn parse_query_multiple_spaces_between_parts() {
        let q = parse_alert_query("metric:cpu_usage   agg:sum   window:600").unwrap();
        assert_eq!(q.metric_name, "cpu_usage");
        assert_eq!(q.aggregation, "sum");
        assert_eq!(q.window_secs, 600);
    }

    #[test]
    fn parse_query_metric_only_uses_defaults() {
        let q = parse_alert_query("metric:memory_usage").unwrap();
        assert_eq!(q.metric_name, "memory_usage");
        assert_eq!(q.aggregation, "avg");
        assert_eq!(q.window_secs, 300);
        assert!(q.labels.is_none());
    }

    #[test]
    fn parse_query_labels_valid_json_object() {
        let q =
            parse_alert_query(r#"metric:errors labels:{"env":"prod","service":"api"} agg:count"#)
                .unwrap();
        assert_eq!(q.aggregation, "count");
        let labels = q.labels.unwrap();
        assert_eq!(labels["env"], "prod");
        assert_eq!(labels["service"], "api");
    }

    #[test]
    fn parse_query_labels_array_json() {
        let q = parse_alert_query(r"metric:test labels:[1,2,3]").unwrap();
        let labels = q.labels.unwrap();
        assert!(labels.is_array());
    }

    #[test]
    fn parse_query_too_long_rejected() {
        let long_query = format!("metric:{}", "x".repeat(1001));
        assert!(parse_alert_query(&long_query).is_err());
    }

    #[test]
    fn parse_query_unknown_prefix_ignored() {
        let q = parse_alert_query("metric:cpu foo:bar baz:qux").unwrap();
        assert_eq!(q.metric_name, "cpu");
        assert_eq!(q.aggregation, "avg");
    }

    #[test]
    fn condition_eq_both_none_returns_false() {
        assert!(!check_condition("eq", None, None));
    }

    #[test]
    fn condition_lt_equal_values_returns_false() {
        assert!(!check_condition("lt", Some(10.0), Some(10.0)));
    }

    #[test]
    fn condition_gt_equal_values_returns_false() {
        assert!(!check_condition("gt", Some(10.0), Some(10.0)));
    }

    #[test]
    fn condition_absent_with_some_threshold_returns_false() {
        assert!(!check_condition("absent", Some(10.0), Some(5.0)));
    }

    #[test]
    fn condition_gt_negative_values() {
        assert!(check_condition("gt", Some(-10.0), Some(-5.0)));
        assert!(!check_condition("gt", Some(-5.0), Some(-10.0)));
    }

    #[test]
    fn alert_state_pending_exactly_at_hold_period() {
        let now = Utc::now();
        let mut state = AlertState {
            first_triggered: Some(now - chrono::Duration::seconds(60)),
            firing: false,
        };
        let t = next_alert_state(&mut state, true, now, 60);
        assert!(state.firing);
        assert!(t.should_fire);
    }

    #[test]
    fn alert_state_with_zero_hold_period() {
        let now = Utc::now();
        let mut state = AlertState {
            first_triggered: None,
            firing: false,
        };
        let t = next_alert_state(&mut state, true, now, 0);
        assert!(state.firing);
        assert!(t.should_fire);
    }

    #[test]
    fn validate_condition_whitespace_rejected() {
        assert!(validate_condition(" gt ").is_err());
    }

    // -- AlertRouter --

    #[test]
    fn alert_router_empty() {
        let router = AlertRouter::empty();
        assert!(router.is_empty());
        assert!(
            router
                .matching_rules("cpu", &serde_json::json!({}), None)
                .is_empty()
        );
    }

    #[test]
    fn alert_router_exact_match() {
        let id1 = Uuid::new_v4();
        let mut routes = HashMap::new();
        routes.insert(("cpu".to_string(), None), vec![(id1, None)]);
        let router = AlertRouter { routes };

        assert_eq!(
            router.matching_rules("cpu", &serde_json::json!({}), None),
            vec![id1]
        );
        // Different metric — no match
        assert!(
            router
                .matching_rules("mem", &serde_json::json!({}), None)
                .is_empty()
        );
    }

    #[test]
    fn alert_router_project_scoping() {
        let id1 = Uuid::new_v4();
        let pid = Uuid::new_v4();
        let mut routes = HashMap::new();
        routes.insert(("cpu".to_string(), Some(pid)), vec![(id1, None)]);
        let router = AlertRouter { routes };

        // Match with correct project_id
        assert_eq!(
            router.matching_rules("cpu", &serde_json::json!({}), Some(pid)),
            vec![id1]
        );
        // No match with None project_id
        assert!(
            router
                .matching_rules("cpu", &serde_json::json!({}), None)
                .is_empty()
        );
        // No match with different project_id
        assert!(
            router
                .matching_rules("cpu", &serde_json::json!({}), Some(Uuid::new_v4()))
                .is_empty()
        );
    }

    #[test]
    fn alert_router_label_filter_subset() {
        let id1 = Uuid::new_v4();
        let filter = serde_json::json!({"env": "prod"});
        let mut routes = HashMap::new();
        routes.insert(("cpu".to_string(), None), vec![(id1, Some(filter))]);
        let router = AlertRouter { routes };

        // Labels superset of filter — match
        assert_eq!(
            router.matching_rules(
                "cpu",
                &serde_json::json!({"env": "prod", "host": "web1"}),
                None,
            ),
            vec![id1]
        );
        // Labels missing filter key — no match
        assert!(
            router
                .matching_rules("cpu", &serde_json::json!({"host": "web1"}), None)
                .is_empty()
        );
        // Labels wrong value — no match
        assert!(
            router
                .matching_rules("cpu", &serde_json::json!({"env": "dev"}), None)
                .is_empty()
        );
    }

    #[test]
    fn alert_router_multiple_rules_same_metric() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let mut routes = HashMap::new();
        routes.insert(
            ("cpu".to_string(), None),
            vec![(id1, None), (id2, Some(serde_json::json!({"env": "prod"})))],
        );
        let router = AlertRouter { routes };

        // Both match when labels include env=prod
        let matches = router.matching_rules("cpu", &serde_json::json!({"env": "prod"}), None);
        assert_eq!(matches.len(), 2);

        // Only the no-filter rule matches
        let matches = router.matching_rules("cpu", &serde_json::json!({}), None);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0], id1);
    }

    // -- labels_subset --

    #[test]
    fn labels_subset_empty_filter_matches() {
        assert!(labels_subset(
            &serde_json::json!({}),
            &serde_json::json!({"env": "prod"}),
        ));
    }

    #[test]
    fn labels_subset_non_object_returns_false() {
        assert!(!labels_subset(
            &serde_json::json!("string"),
            &serde_json::json!({"env": "prod"}),
        ));
        assert!(!labels_subset(
            &serde_json::json!({"env": "prod"}),
            &serde_json::json!([1, 2]),
        ));
    }

    // -- RuleWindow --

    #[test]
    fn rule_window_push_and_aggregate_avg() {
        let now = Utc::now();
        let mut w = RuleWindow::new(300, "avg");
        w.push(now, 10.0);
        w.push(now, 20.0);
        w.push(now, 30.0);
        assert_eq!(w.len(), 3);
        let avg = w.aggregate().unwrap();
        assert!((avg - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn rule_window_aggregate_sum() {
        let now = Utc::now();
        let mut w = RuleWindow::new(300, "sum");
        w.push(now, 5.0);
        w.push(now, 15.0);
        assert!((w.aggregate().unwrap() - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn rule_window_aggregate_count() {
        let now = Utc::now();
        let mut w = RuleWindow::new(300, "count");
        w.push(now, 1.0);
        w.push(now, 2.0);
        w.push(now, 3.0);
        assert!((w.aggregate().unwrap() - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn rule_window_aggregate_max() {
        let now = Utc::now();
        let mut w = RuleWindow::new(300, "max");
        w.push(now, 5.0);
        w.push(now, 25.0);
        w.push(now, 15.0);
        assert!((w.aggregate().unwrap() - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn rule_window_aggregate_min() {
        let now = Utc::now();
        let mut w = RuleWindow::new(300, "min");
        w.push(now, 15.0);
        w.push(now, 5.0);
        w.push(now, 25.0);
        assert!((w.aggregate().unwrap() - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn rule_window_empty_returns_none() {
        let w = RuleWindow::new(300, "avg");
        assert!(w.aggregate().is_none());
        assert_eq!(w.len(), 0);
    }

    #[test]
    fn rule_window_evict_expired() {
        let now = Utc::now();
        let old = now - chrono::Duration::seconds(400);
        let recent = now - chrono::Duration::seconds(100);
        let mut w = RuleWindow::new(300, "avg");
        w.push(old, 100.0);
        w.push(recent, 20.0);
        assert_eq!(w.len(), 2);

        w.evict_expired(now);
        assert_eq!(w.len(), 1);
        assert!((w.aggregate().unwrap() - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn rule_window_evict_all_expired() {
        let now = Utc::now();
        let old1 = now - chrono::Duration::seconds(400);
        let old2 = now - chrono::Duration::seconds(350);
        let mut w = RuleWindow::new(300, "sum");
        w.push(old1, 10.0);
        w.push(old2, 20.0);

        w.evict_expired(now);
        assert_eq!(w.len(), 0);
        assert!(w.aggregate().is_none());
    }

    #[test]
    fn rule_window_running_sum_tracks_eviction() {
        let now = Utc::now();
        let old = now - chrono::Duration::seconds(400);
        let mut w = RuleWindow::new(300, "sum");
        w.push(old, 50.0);
        w.push(now, 30.0);
        assert!((w.aggregate().unwrap() - 80.0).abs() < f64::EPSILON);

        w.evict_expired(now);
        assert!((w.aggregate().unwrap() - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn rule_window_unknown_aggregation_returns_none() {
        let now = Utc::now();
        let mut w = RuleWindow::new(300, "p99");
        w.push(now, 10.0);
        assert!(w.aggregate().is_none());
    }

    // -- process_entry field parsing --

    #[test]
    fn parse_xread_response_empty() {
        let response = fred::types::streams::XReadResponse::new();
        assert!(parse_xread_response(response).is_empty());
    }

    // -- value_to_string --

    #[test]
    fn value_to_string_variants() {
        assert_eq!(
            value_to_string(&fred::types::Value::String("hello".into())),
            Some("hello".to_string())
        );
        assert_eq!(
            value_to_string(&fred::types::Value::Integer(42)),
            Some("42".to_string())
        );
        assert_eq!(
            value_to_string(&fred::types::Value::Double(3.14)),
            Some("3.14".to_string())
        );
        assert!(value_to_string(&fred::types::Value::Null).is_none());
    }

    // -- parse_xautoclaim_result --

    #[test]
    fn xautoclaim_empty_array() {
        let val = fred::types::Value::Array(vec![]);
        assert!(parse_xautoclaim_result(val).is_empty());
    }

    #[test]
    fn xautoclaim_non_array() {
        let val = fred::types::Value::Null;
        assert!(parse_xautoclaim_result(val).is_empty());
    }

    #[test]
    fn xautoclaim_single_entry() {
        // Simulate: [cursor, [[id, [k1, v1, k2, v2]]], []]
        let entry = fred::types::Value::Array(vec![
            fred::types::Value::String("1234-0".into()),
            fred::types::Value::Array(vec![
                fred::types::Value::String("r".into()),
                fred::types::Value::String("abc".into()),
                fred::types::Value::String("t".into()),
                fred::types::Value::String("999".into()),
            ]),
        ]);
        let val = fred::types::Value::Array(vec![
            fred::types::Value::String("0-0".into()),
            fred::types::Value::Array(vec![entry]),
            fred::types::Value::Array(vec![]),
        ]);
        let result = parse_xautoclaim_result(val);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "1234-0");
        assert_eq!(result[0].1.get("r").unwrap(), "abc");
        assert_eq!(result[0].1.get("t").unwrap(), "999");
    }
}
