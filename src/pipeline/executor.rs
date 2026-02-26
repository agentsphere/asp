use std::collections::BTreeMap;
use std::time::Instant;

use base64::Engine;
use k8s_openapi::api::core::v1::{
    Container, EmptyDirVolumeSource, EnvVar, Pod, PodSpec, Secret, SecretVolumeSource, Volume,
    VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use kube::Api;
use kube::api::{DeleteParams, ListParams, LogParams, PostParams};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::token;
use crate::store::AppState;

use super::error::PipelineError;

// ---------------------------------------------------------------------------
// Background executor loop
// ---------------------------------------------------------------------------

/// Background task that polls for pending pipelines and executes them.
pub async fn run(state: AppState, mut shutdown: tokio::sync::watch::Receiver<()>) {
    tracing::info!("pipeline executor started");

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));

    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                tracing::info!("pipeline executor shutting down");
                break;
            }
            _ = interval.tick() => {
                if let Err(e) = poll_pending(&state).await {
                    tracing::error!(error = %e, "error polling pending pipelines");
                }
            }
            () = state.pipeline_notify.notified() => {
                // Immediate poll on notification
                if let Err(e) = poll_pending(&state).await {
                    tracing::error!(error = %e, "error polling pending pipelines (notified)");
                }
                // Reset interval to avoid immediate double-poll
                interval.reset();
            }
        }
    }
}

/// Find pending pipelines and spawn execution tasks.
async fn poll_pending(state: &AppState) -> Result<(), PipelineError> {
    let pending = sqlx::query_scalar!(
        r#"
        SELECT id FROM pipelines
        WHERE status = 'pending'
        ORDER BY created_at ASC
        LIMIT 5
        "#,
    )
    .fetch_all(&state.pool)
    .await?;

    for pipeline_id in pending {
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = execute_pipeline(&state, pipeline_id).await {
                tracing::error!(error = %e, %pipeline_id, "pipeline execution failed");
                let _ = mark_pipeline_failed(&state.pool, pipeline_id).await;
            }
        });
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Pipeline execution
// ---------------------------------------------------------------------------

/// Execute a single pipeline: run each step as a K8s pod sequentially.
#[tracing::instrument(skip(state), fields(%pipeline_id), err)]
async fn execute_pipeline(state: &AppState, pipeline_id: Uuid) -> Result<(), PipelineError> {
    // Claim the pipeline by setting status to running
    let claimed = sqlx::query_scalar!(
        r#"
        UPDATE pipelines SET status = 'running', started_at = now()
        WHERE id = $1 AND status = 'pending'
        RETURNING project_id
        "#,
        pipeline_id,
    )
    .fetch_optional(&state.pool)
    .await?;

    let Some(project_id) = claimed else {
        tracing::debug!(%pipeline_id, "pipeline already claimed");
        return Ok(());
    };

    // Load pipeline metadata
    let pipeline = sqlx::query!(
        r#"
        SELECT pl.git_ref as "git_ref!: String",
               pl.commit_sha,
               pl.triggered_by,
               p.name as "project_name!: String",
               p.repo_path as "repo_path!: String"
        FROM pipelines pl
        JOIN projects p ON p.id = pl.project_id
        WHERE pl.id = $1
        "#,
        pipeline_id,
    )
    .fetch_one(&state.pool)
    .await?;

    let meta = PipelineMeta {
        git_ref: pipeline.git_ref,
        commit_sha: pipeline.commit_sha,
        project_name: pipeline.project_name,
        repo_path: pipeline.repo_path,
    };

    // Create registry auth Secret if registry is configured and we know who triggered it
    let registry_creds = if state.config.registry_url.is_some() {
        if let Some(user_id) = pipeline.triggered_by {
            match create_registry_secret(state, pipeline_id, user_id).await {
                Ok(creds) => Some(creds),
                Err(e) => {
                    tracing::warn!(error = %e, "failed to create registry secret, continuing without");
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    let registry_secret_name = registry_creds.as_ref().map(|(name, _)| name.as_str());
    let all_succeeded =
        run_all_steps(state, pipeline_id, project_id, &meta, registry_secret_name).await?;

    // Clean up registry auth Secret + token
    if let Some((_, ref token_hash)) = registry_creds {
        cleanup_registry_secret(state, pipeline_id, token_hash).await;
    }

    // Finalize pipeline
    let final_status = if all_succeeded { "success" } else { "failure" };
    sqlx::query!(
        "UPDATE pipelines SET status = $2, finished_at = now() WHERE id = $1",
        pipeline_id,
        final_status,
    )
    .execute(&state.pool)
    .await?;

    if all_succeeded {
        detect_and_write_deployment(state, pipeline_id, project_id).await;
    }

    fire_build_webhook(&state.pool, project_id, pipeline_id, final_status).await;
    tracing::info!(%pipeline_id, status = final_status, "pipeline finished");
    Ok(())
}

/// Parameters extracted from pipeline + project join query.
struct PipelineMeta {
    git_ref: String,
    commit_sha: Option<String>,
    project_name: String,
    repo_path: String,
}

/// A pipeline step row loaded from the database.
struct StepRow {
    id: Uuid,
    step_order: i32,
    name: String,
    image: String,
    commands: Vec<String>,
}

/// Run all steps for a pipeline. Returns true if all steps succeeded.
async fn run_all_steps(
    state: &AppState,
    pipeline_id: Uuid,
    project_id: Uuid,
    pipeline: &PipelineMeta,
    registry_secret: Option<&str>,
) -> Result<bool, PipelineError> {
    let steps = sqlx::query_as!(
        StepRow,
        r#"
        SELECT id, step_order, name, image, commands
        FROM pipeline_steps
        WHERE pipeline_id = $1
        ORDER BY step_order ASC
        "#,
        pipeline_id,
    )
    .fetch_all(&state.pool)
    .await?;

    let namespace = &state.config.pipeline_namespace;
    let pods: Api<Pod> = Api::namespaced(state.kube.clone(), namespace);

    for step in &steps {
        if is_cancelled(&state.pool, pipeline_id).await? {
            skip_remaining_steps(&state.pool, pipeline_id).await?;
            return Ok(false);
        }

        let succeeded = execute_single_step(
            state,
            &pods,
            pipeline_id,
            project_id,
            pipeline,
            step,
            registry_secret,
        )
        .await?;

        if !succeeded {
            skip_remaining_after(&state.pool, pipeline_id, step.step_order).await?;
            return Ok(false);
        }
    }

    Ok(true)
}

/// Execute one pipeline step as a K8s pod. Returns true on success.
async fn execute_single_step(
    state: &AppState,
    pods: &Api<Pod>,
    pipeline_id: Uuid,
    project_id: Uuid,
    pipeline: &PipelineMeta,
    step: &StepRow,
    registry_secret: Option<&str>,
) -> Result<bool, PipelineError> {
    let env_vars = build_env_vars(
        state,
        pipeline_id,
        project_id,
        &pipeline.project_name,
        &pipeline.git_ref,
        pipeline.commit_sha.as_deref(),
        &step.name,
    );

    let pod_name = format!("pl-{}-{}", &pipeline_id.to_string()[..8], slug(&step.name));
    let pod_spec = build_pod_spec(&PodSpecParams {
        pod_name: &pod_name,
        pipeline_id,
        project_id,
        step_name: &step.name,
        image: &step.image,
        commands: &step.commands,
        env_vars: &env_vars,
        repo_path: &pipeline.repo_path,
        git_ref: &pipeline.git_ref,
        registry_secret,
    });

    sqlx::query!(
        "UPDATE pipeline_steps SET status = 'running' WHERE id = $1",
        step.id
    )
    .execute(&state.pool)
    .await?;

    let start = Instant::now();
    let result = run_step(pods, &pod_name, &pod_spec, state, pipeline_id, &step.name).await;
    let duration_ms = i32::try_from(start.elapsed().as_millis()).unwrap_or(i32::MAX);

    match result {
        Ok(exit_code) => {
            let status = if exit_code == 0 { "success" } else { "failure" };
            let log_ref = format!("logs/pipelines/{pipeline_id}/{}.log", step.name);
            sqlx::query!(
                r#"UPDATE pipeline_steps SET status = $2, exit_code = $3, duration_ms = $4, log_ref = $5 WHERE id = $1"#,
                step.id, status, exit_code, duration_ms, log_ref,
            )
            .execute(&state.pool)
            .await?;
            Ok(exit_code == 0)
        }
        Err(e) => {
            tracing::error!(error = %e, step = %step.name, "step execution error");
            sqlx::query!(
                "UPDATE pipeline_steps SET status = 'failure', duration_ms = $2 WHERE id = $1",
                step.id,
                duration_ms,
            )
            .execute(&state.pool)
            .await?;
            Ok(false)
        }
    }
}

// ---------------------------------------------------------------------------
// Pod execution
// ---------------------------------------------------------------------------

/// Create a K8s pod, wait for completion, capture logs, clean up. Returns exit code.
async fn run_step(
    pods: &Api<Pod>,
    pod_name: &str,
    pod_spec: &Pod,
    state: &AppState,
    pipeline_id: Uuid,
    step_name: &str,
) -> Result<i32, PipelineError> {
    // Create the pod
    pods.create(&PostParams::default(), pod_spec).await?;

    // Wait for pod to finish
    let exit_code = wait_for_pod(pods, pod_name).await?;

    // Capture logs to MinIO
    capture_logs(pods, pod_name, state, pipeline_id, step_name).await;

    // Clean up pod
    let _ = pods.delete(pod_name, &DeleteParams::default()).await;

    Ok(exit_code)
}

/// Poll pod status until it reaches a terminal phase.
async fn wait_for_pod(pods: &Api<Pod>, pod_name: &str) -> Result<i32, PipelineError> {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        let pod = match pods.get(pod_name).await {
            Ok(p) => p,
            Err(kube::Error::Api(err)) if err.code == 404 => {
                return Err(PipelineError::Other(anyhow::anyhow!(
                    "pod {pod_name} disappeared"
                )));
            }
            Err(e) => return Err(e.into()),
        };

        let Some(status) = &pod.status else {
            continue;
        };
        let phase = status.phase.as_deref().unwrap_or("Unknown");

        match phase {
            "Succeeded" => return Ok(0),
            "Failed" => {
                let exit_code = extract_exit_code(status).unwrap_or(1);
                return Ok(exit_code);
            }
            "Pending" | "Running" => {}
            other => {
                tracing::warn!(pod = pod_name, phase = other, "unexpected pod phase");
            }
        }
    }
}

/// Extract the exit code from the first container's termination state.
fn extract_exit_code(status: &k8s_openapi::api::core::v1::PodStatus) -> Option<i32> {
    status
        .container_statuses
        .as_ref()?
        .first()?
        .state
        .as_ref()?
        .terminated
        .as_ref()
        .map(|t| t.exit_code)
}

/// Capture pod logs and write them to `MinIO`.
async fn capture_logs(
    pods: &Api<Pod>,
    pod_name: &str,
    state: &AppState,
    pipeline_id: Uuid,
    step_name: &str,
) {
    let log_params = LogParams {
        container: Some("step".into()),
        ..Default::default()
    };

    match pods.logs(pod_name, &log_params).await {
        Ok(logs) => {
            let path = format!("logs/pipelines/{pipeline_id}/{step_name}.log");
            if let Err(e) = state.minio.write(&path, logs.into_bytes()).await {
                tracing::error!(error = %e, %path, "failed to write logs to MinIO");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, pod = pod_name, "failed to read pod logs");
        }
    }
}

// ---------------------------------------------------------------------------
// Registry auth Secret for pipeline pods
// ---------------------------------------------------------------------------

/// Create a short-lived API token and a K8s Secret containing Docker config
/// JSON so that Kaniko/buildah steps can authenticate with the platform registry.
///
/// Returns `(secret_name, token_hash)` — the token hash is needed to clean up
/// the DB row after the pipeline finishes.
async fn create_registry_secret(
    state: &AppState,
    pipeline_id: Uuid,
    triggered_by: Uuid,
) -> Result<(String, String), PipelineError> {
    let registry_url = state
        .config
        .registry_url
        .as_deref()
        .ok_or_else(|| PipelineError::Other(anyhow::anyhow!("registry_url not configured")))?;

    // Create a short-lived API token (1 hour) for the triggering user
    let (raw_token, token_hash) = token::generate_api_token();

    sqlx::query!(
        r#"INSERT INTO api_tokens (id, user_id, name, token_hash, expires_at)
           VALUES ($1, $2, $3, $4, now() + interval '1 hour')"#,
        Uuid::new_v4(),
        triggered_by,
        format!("pipeline-{pipeline_id}"),
        token_hash,
    )
    .execute(&state.pool)
    .await?;

    // Look up the username for Docker config
    let user_name = sqlx::query_scalar!("SELECT name FROM users WHERE id = $1", triggered_by)
        .fetch_one(&state.pool)
        .await?;

    // Build Docker config JSON: {"auths":{"<registry>":{"auth":"<base64(user:token)>"}}}
    let basic_auth =
        base64::engine::general_purpose::STANDARD.encode(format!("{user_name}:{raw_token}"));
    let config_json = serde_json::json!({
        "auths": {
            registry_url: {
                "auth": basic_auth
            }
        }
    });

    let secret_name = format!("pl-registry-{}", &pipeline_id.to_string()[..8]);
    let namespace = &state.config.pipeline_namespace;

    let secret = Secret {
        metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
            name: Some(secret_name.clone()),
            labels: Some(BTreeMap::from([(
                "platform.io/pipeline".into(),
                pipeline_id.to_string(),
            )])),
            ..Default::default()
        },
        string_data: Some(BTreeMap::from([(
            "config.json".into(),
            config_json.to_string(),
        )])),
        type_: Some("Opaque".into()),
        ..Default::default()
    };

    let secrets: Api<Secret> = Api::namespaced(state.kube.clone(), namespace);
    secrets.create(&PostParams::default(), &secret).await?;

    tracing::debug!(%pipeline_id, %secret_name, "created registry auth secret");
    Ok((secret_name, token_hash))
}

/// Clean up the registry auth K8s Secret and the short-lived API token.
async fn cleanup_registry_secret(state: &AppState, pipeline_id: Uuid, token_hash: &str) {
    let secret_name = format!("pl-registry-{}", &pipeline_id.to_string()[..8]);
    let namespace = &state.config.pipeline_namespace;

    // Delete the K8s Secret
    let secrets: Api<Secret> = Api::namespaced(state.kube.clone(), namespace);
    if let Err(e) = secrets.delete(&secret_name, &DeleteParams::default()).await {
        tracing::warn!(error = %e, %secret_name, "failed to delete registry auth secret");
    }

    // Delete the short-lived API token from the DB
    if let Err(e) = sqlx::query!("DELETE FROM api_tokens WHERE token_hash = $1", token_hash)
        .execute(&state.pool)
        .await
    {
        tracing::warn!(error = %e, "failed to delete pipeline API token");
    }
}

// ---------------------------------------------------------------------------
// Pod spec builder
// ---------------------------------------------------------------------------

struct PodSpecParams<'a> {
    pod_name: &'a str,
    pipeline_id: Uuid,
    project_id: Uuid,
    step_name: &'a str,
    image: &'a str,
    commands: &'a [String],
    env_vars: &'a [EnvVar],
    repo_path: &'a str,
    git_ref: &'a str,
    /// K8s Secret name containing Docker config JSON for registry auth.
    registry_secret: Option<&'a str>,
}

/// Build the volumes and step container mounts for a pipeline pod.
fn build_volumes_and_mounts(
    repo_path: &str,
    registry_secret: Option<&str>,
) -> (Vec<Volume>, Vec<VolumeMount>) {
    let mut step_mounts = vec![VolumeMount {
        name: "workspace".into(),
        mount_path: "/workspace".into(),
        ..Default::default()
    }];

    let mut volumes = vec![
        Volume {
            name: "workspace".into(),
            empty_dir: Some(EmptyDirVolumeSource::default()),
            ..Default::default()
        },
        Volume {
            name: "repos".into(),
            host_path: Some(k8s_openapi::api::core::v1::HostPathVolumeSource {
                path: repo_path.into(),
                type_: Some("Directory".into()),
            }),
            ..Default::default()
        },
    ];

    // If a registry auth Secret is provided, mount it as Docker config
    if let Some(secret_name) = registry_secret {
        volumes.push(Volume {
            name: "docker-config".into(),
            secret: Some(SecretVolumeSource {
                secret_name: Some(secret_name.into()),
                ..Default::default()
            }),
            ..Default::default()
        });
        step_mounts.push(VolumeMount {
            name: "docker-config".into(),
            mount_path: "/kaniko/.docker".into(),
            read_only: Some(true),
            ..Default::default()
        });
    }

    (volumes, step_mounts)
}

fn build_pod_spec(p: &PodSpecParams<'_>) -> Pod {
    let script = p.commands.join(" && ");

    let labels = BTreeMap::from([
        ("platform.io/pipeline".into(), p.pipeline_id.to_string()),
        ("platform.io/step".into(), slug(p.step_name)),
        ("platform.io/project".into(), p.project_id.to_string()),
    ]);

    // Strip refs/heads/ prefix for git clone --branch
    let branch = p
        .git_ref
        .strip_prefix("refs/heads/")
        .or_else(|| p.git_ref.strip_prefix("refs/tags/"))
        .unwrap_or(p.git_ref);

    let (volumes, step_mounts) = build_volumes_and_mounts(p.repo_path, p.registry_secret);

    Pod {
        metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
            name: Some(p.pod_name.into()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(PodSpec {
            restart_policy: Some("Never".into()),
            init_containers: Some(vec![Container {
                name: "clone".into(),
                image: Some("alpine/git:latest".into()),
                command: Some(vec!["sh".into(), "-c".into()]),
                args: Some(vec![format!(
                    "git clone --depth 1 --branch {branch} file://{} /workspace",
                    p.repo_path
                )]),
                volume_mounts: Some(vec![
                    VolumeMount {
                        name: "workspace".into(),
                        mount_path: "/workspace".into(),
                        ..Default::default()
                    },
                    VolumeMount {
                        name: "repos".into(),
                        mount_path: p.repo_path.into(),
                        read_only: Some(true),
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            }]),
            containers: vec![Container {
                name: "step".into(),
                image: Some(p.image.into()),
                command: Some(vec!["sh".into(), "-c".into()]),
                args: Some(vec![script]),
                working_dir: Some("/workspace".into()),
                env: Some(p.env_vars.to_vec()),
                volume_mounts: Some(step_mounts),
                resources: Some(k8s_openapi::api::core::v1::ResourceRequirements {
                    limits: Some(BTreeMap::from([
                        ("cpu".into(), Quantity("1".into())),
                        ("memory".into(), Quantity("1Gi".into())),
                    ])),
                    requests: Some(BTreeMap::from([
                        ("cpu".into(), Quantity("250m".into())),
                        ("memory".into(), Quantity("256Mi".into())),
                    ])),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            volumes: Some(volumes),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn build_env_vars(
    state: &AppState,
    pipeline_id: Uuid,
    project_id: Uuid,
    project_name: &str,
    git_ref: &str,
    commit_sha: Option<&str>,
    step_name: &str,
) -> Vec<EnvVar> {
    build_env_vars_core(
        pipeline_id,
        project_id,
        project_name,
        git_ref,
        commit_sha,
        step_name,
        state.config.registry_url.as_deref(),
    )
}

/// Core env var builder with no dependency on `AppState`.
fn build_env_vars_core(
    pipeline_id: Uuid,
    project_id: Uuid,
    project_name: &str,
    git_ref: &str,
    commit_sha: Option<&str>,
    step_name: &str,
    registry_url: Option<&str>,
) -> Vec<EnvVar> {
    let branch = git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref);

    let mut vars = vec![
        env_var("PLATFORM_PROJECT_ID", &project_id.to_string()),
        env_var("PLATFORM_PROJECT_NAME", project_name),
        env_var("PIPELINE_ID", &pipeline_id.to_string()),
        env_var("STEP_NAME", step_name),
        env_var("COMMIT_REF", git_ref),
        env_var("COMMIT_BRANCH", branch),
        env_var("PROJECT", project_name),
    ];

    if let Some(sha) = commit_sha {
        vars.push(env_var("COMMIT_SHA", sha));
    }

    if let Some(registry) = registry_url {
        vars.push(env_var("REGISTRY", registry));
        // Kaniko and buildah look for Docker config at $DOCKER_CONFIG/config.json
        vars.push(env_var("DOCKER_CONFIG", "/kaniko/.docker"));
    }

    vars
}

fn env_var(name: &str, value: &str) -> EnvVar {
    EnvVar {
        name: name.into(),
        value: Some(value.into()),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Status helpers
// ---------------------------------------------------------------------------

async fn is_cancelled(pool: &PgPool, pipeline_id: Uuid) -> Result<bool, PipelineError> {
    let status = sqlx::query_scalar!("SELECT status FROM pipelines WHERE id = $1", pipeline_id,)
        .fetch_one(pool)
        .await?;

    Ok(status == "cancelled")
}

async fn skip_remaining_steps(pool: &PgPool, pipeline_id: Uuid) -> Result<(), PipelineError> {
    sqlx::query!(
        "UPDATE pipeline_steps SET status = 'skipped' WHERE pipeline_id = $1 AND status = 'pending'",
        pipeline_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn skip_remaining_after(
    pool: &PgPool,
    pipeline_id: Uuid,
    after_order: i32,
) -> Result<(), PipelineError> {
    sqlx::query!(
        r#"
        UPDATE pipeline_steps SET status = 'skipped'
        WHERE pipeline_id = $1 AND step_order > $2 AND status = 'pending'
        "#,
        pipeline_id,
        after_order,
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn mark_pipeline_failed(pool: &PgPool, pipeline_id: Uuid) -> Result<(), PipelineError> {
    sqlx::query!(
        "UPDATE pipelines SET status = 'failure', finished_at = now() WHERE id = $1 AND status IN ('pending', 'running')",
        pipeline_id,
    )
    .execute(pool)
    .await?;

    skip_remaining_steps(pool, pipeline_id).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Deployment handoff
// ---------------------------------------------------------------------------

/// If any step used a kaniko-like image, publish an `ImageBuilt` event (for production)
/// or directly upsert a preview deployment (for non-main branches).
async fn detect_and_write_deployment(state: &AppState, pipeline_id: Uuid, project_id: Uuid) {
    let image_steps = sqlx::query!(
        r#"
        SELECT name, image FROM pipeline_steps
        WHERE pipeline_id = $1 AND status = 'success' AND image ILIKE '%kaniko%'
        "#,
        pipeline_id,
    )
    .fetch_all(&state.pool)
    .await;

    let Ok(image_steps) = image_steps else {
        return;
    };

    if image_steps.is_empty() {
        return;
    }

    // Get the git_ref, commit SHA, and triggered_by for the pipeline
    let pipeline_meta = sqlx::query!(
        "SELECT git_ref, commit_sha, triggered_by FROM pipelines WHERE id = $1",
        pipeline_id,
    )
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();

    let Some(pipeline_meta) = pipeline_meta else {
        return;
    };

    let project_name = sqlx::query_scalar!("SELECT name FROM projects WHERE id = $1", project_id)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten();

    let registry = state
        .config
        .registry_url
        .as_deref()
        .unwrap_or("localhost:5000");
    let name = project_name.as_deref().unwrap_or("unknown");
    let tag = pipeline_meta.commit_sha.as_deref().unwrap_or("latest");
    let image_ref = format!("{registry}/{name}:{tag}");

    // Extract branch from git_ref
    let branch = pipeline_meta
        .git_ref
        .strip_prefix("refs/heads/")
        .unwrap_or(&pipeline_meta.git_ref);

    let is_main = matches!(branch, "main" | "master");

    if is_main {
        // Publish ImageBuilt event — the event bus handler will commit to
        // the ops repo and trigger deployment.
        let event = crate::store::eventbus::PlatformEvent::ImageBuilt {
            project_id,
            environment: "production".into(),
            image_ref: image_ref.clone(),
            pipeline_id,
            triggered_by: pipeline_meta.triggered_by,
        };
        if let Err(e) = crate::store::eventbus::publish(&state.valkey, &event).await {
            tracing::error!(error = %e, %project_id, "failed to publish ImageBuilt event");
        }

        tracing::info!(%project_id, %image_ref, "ImageBuilt event published from pipeline");
    } else {
        // Preview deployments bypass the event bus (no ops repo)
        if let Err(e) = upsert_preview_deployment(
            state,
            pipeline_id,
            project_id,
            branch,
            &image_ref,
            pipeline_meta.triggered_by,
        )
        .await
        {
            tracing::error!(error = %e, %project_id, %branch, "failed to upsert preview deployment");
        }
    }
}

/// Create or update a preview deployment for a non-main branch.
#[tracing::instrument(skip(state), fields(%pipeline_id, %project_id, %branch), err)]
async fn upsert_preview_deployment(
    state: &AppState,
    pipeline_id: Uuid,
    project_id: Uuid,
    branch: &str,
    image_ref: &str,
    triggered_by: Option<Uuid>,
) -> Result<(), anyhow::Error> {
    let branch_slug = crate::pipeline::slugify_branch(branch);

    sqlx::query!(
        r#"INSERT INTO preview_deployments
            (project_id, branch, branch_slug, image_ref, pipeline_id, created_by)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (project_id, branch_slug) DO UPDATE SET
            image_ref = EXCLUDED.image_ref,
            pipeline_id = EXCLUDED.pipeline_id,
            desired_status = 'active',
            current_status = 'pending',
            expires_at = now() + (preview_deployments.ttl_hours || ' hours')::interval,
            updated_at = now()"#,
        project_id,
        branch,
        branch_slug,
        image_ref,
        pipeline_id,
        triggered_by,
    )
    .execute(&state.pool)
    .await?;

    tracing::info!(
        %project_id,
        %branch,
        slug = %branch_slug,
        image = %image_ref,
        "preview deployment upserted"
    );

    // Fire webhook for preview event
    crate::api::webhooks::fire_webhooks(
        &state.pool,
        project_id,
        "deploy",
        &serde_json::json!({
            "action": "preview_created",
            "branch": branch,
            "branch_slug": branch_slug,
            "image_ref": image_ref,
            "pipeline_id": pipeline_id,
        }),
    )
    .await;

    Ok(())
}

// ---------------------------------------------------------------------------
// Webhook
// ---------------------------------------------------------------------------

async fn fire_build_webhook(pool: &PgPool, project_id: Uuid, pipeline_id: Uuid, status: &str) {
    let payload = serde_json::json!({
        "action": status,
        "pipeline_id": pipeline_id,
        "project_id": project_id,
    });
    crate::api::webhooks::fire_webhooks(pool, project_id, "build", &payload).await;
}

// ---------------------------------------------------------------------------
// Cancellation (called from API)
// ---------------------------------------------------------------------------

/// Cancel a running pipeline: delete K8s pods and mark as cancelled.
#[tracing::instrument(skip(state), fields(%pipeline_id), err)]
pub async fn cancel_pipeline(state: &AppState, pipeline_id: Uuid) -> Result<(), PipelineError> {
    // Mark pipeline as cancelled
    sqlx::query!(
        "UPDATE pipelines SET status = 'cancelled', finished_at = now() WHERE id = $1 AND status IN ('pending', 'running')",
        pipeline_id,
    )
    .execute(&state.pool)
    .await?;

    skip_remaining_steps(&state.pool, pipeline_id).await?;

    // Delete running pods by label selector
    let namespace = &state.config.pipeline_namespace;
    let pods: Api<Pod> = Api::namespaced(state.kube.clone(), namespace);
    let label = format!("platform.io/pipeline={pipeline_id}");
    let lp = ListParams::default().labels(&label);

    if let Ok(pod_list) = pods.list(&lp).await {
        for pod in pod_list {
            if let Some(name) = pod.metadata.name {
                let _ = pods.delete(&name, &DeleteParams::default()).await;
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

use super::slug;

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{
        ContainerState, ContainerStateTerminated, ContainerStatus, PodStatus,
    };

    // -- test-only helpers for kaniko detection / branch classification --

    fn is_kaniko_image(image: &str) -> bool {
        image.to_ascii_lowercase().contains("kaniko")
    }

    fn classify_branch(branch: &str) -> &'static str {
        if matches!(branch, "main" | "master") {
            "production"
        } else {
            "preview"
        }
    }

    fn build_image_ref(registry: &str, project_name: &str, tag: &str) -> String {
        format!("{registry}/{project_name}:{tag}")
    }

    // -- slug --

    #[test]
    fn slug_simple() {
        assert_eq!(slug("test"), "test");
    }

    #[test]
    fn slug_uppercase() {
        assert_eq!(slug("Build-Image"), "build-image");
    }

    #[test]
    fn slug_special_chars() {
        assert_eq!(slug("my step (1)"), "my-step--1");
    }

    #[test]
    fn slug_leading_trailing_special() {
        assert_eq!(slug("--test--"), "test");
    }

    #[test]
    fn slug_empty() {
        assert_eq!(slug(""), "");
    }

    #[test]
    fn slug_all_special() {
        assert_eq!(slug("!!!"), "");
    }

    // -- extract_exit_code --

    #[test]
    fn exit_code_from_terminated_container() {
        let status = PodStatus {
            container_statuses: Some(vec![ContainerStatus {
                name: "step".into(),
                ready: false,
                restart_count: 0,
                image: String::new(),
                image_id: String::new(),
                state: Some(ContainerState {
                    terminated: Some(ContainerStateTerminated {
                        exit_code: 42,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }]),
            ..Default::default()
        };
        assert_eq!(extract_exit_code(&status), Some(42));
    }

    #[test]
    fn exit_code_none_when_no_container_statuses() {
        let status = PodStatus {
            container_statuses: None,
            ..Default::default()
        };
        assert_eq!(extract_exit_code(&status), None);
    }

    #[test]
    fn exit_code_none_when_empty_statuses() {
        let status = PodStatus {
            container_statuses: Some(vec![]),
            ..Default::default()
        };
        assert_eq!(extract_exit_code(&status), None);
    }

    #[test]
    fn exit_code_none_when_no_terminated_state() {
        let status = PodStatus {
            container_statuses: Some(vec![ContainerStatus {
                name: "step".into(),
                ready: false,
                restart_count: 0,
                image: String::new(),
                image_id: String::new(),
                state: Some(ContainerState {
                    running: Some(Default::default()),
                    terminated: None,
                    ..Default::default()
                }),
                ..Default::default()
            }]),
            ..Default::default()
        };
        assert_eq!(extract_exit_code(&status), None);
    }

    #[test]
    fn exit_code_none_when_no_state() {
        let status = PodStatus {
            container_statuses: Some(vec![ContainerStatus {
                name: "step".into(),
                ready: false,
                restart_count: 0,
                image: String::new(),
                image_id: String::new(),
                state: None,
                ..Default::default()
            }]),
            ..Default::default()
        };
        assert_eq!(extract_exit_code(&status), None);
    }

    // -- build_pod_spec --

    #[test]
    fn build_pod_spec_structure() {
        let pipeline_id = Uuid::nil();
        let project_id = Uuid::nil();
        let pod = build_pod_spec(&PodSpecParams {
            pod_name: "pl-test-build",
            pipeline_id,
            project_id,
            step_name: "build",
            image: "rust:latest",
            commands: &["cargo build".into(), "cargo test".into()],
            env_vars: &[env_var("FOO", "bar")],
            repo_path: "/data/repos/owner/repo.git",
            git_ref: "refs/heads/main",
            registry_secret: None,
        });

        assert_eq!(pod.metadata.name.as_deref(), Some("pl-test-build"));

        let labels = pod.metadata.labels.as_ref().unwrap();
        assert_eq!(labels["platform.io/step"], "build");

        let spec = pod.spec.as_ref().unwrap();
        assert_eq!(spec.restart_policy.as_deref(), Some("Never"));

        let init = &spec.init_containers.as_ref().unwrap()[0];
        assert_eq!(init.image.as_deref(), Some("alpine/git:latest"));

        let container = &spec.containers[0];
        assert_eq!(container.image.as_deref(), Some("rust:latest"));
        assert_eq!(
            container.args.as_ref().unwrap()[0],
            "cargo build && cargo test"
        );

        let limits = container
            .resources
            .as_ref()
            .unwrap()
            .limits
            .as_ref()
            .unwrap();
        assert_eq!(limits["cpu"], Quantity("1".into()));
        assert_eq!(limits["memory"], Quantity("1Gi".into()));

        let volumes = spec.volumes.as_ref().unwrap();
        assert_eq!(volumes.len(), 2);
        assert_eq!(volumes[0].name, "workspace");
        assert_eq!(volumes[1].name, "repos");
    }

    #[test]
    fn build_pod_spec_strips_refs_heads_prefix() {
        let pod = build_pod_spec(&PodSpecParams {
            pod_name: "pl-test",
            pipeline_id: Uuid::nil(),
            project_id: Uuid::nil(),
            step_name: "test",
            image: "alpine:3.19",
            commands: &["echo hello".into()],
            env_vars: &[],
            repo_path: "/repos/test.git",
            git_ref: "refs/heads/feature-branch",
            registry_secret: None,
        });

        let init = &pod.spec.unwrap().init_containers.unwrap()[0];
        let clone_cmd = &init.args.as_ref().unwrap()[0];
        assert!(
            clone_cmd.contains("--branch feature-branch"),
            "should strip refs/heads/ prefix, got: {clone_cmd}"
        );
    }

    #[test]
    fn build_pod_spec_strips_refs_tags_prefix() {
        let pod = build_pod_spec(&PodSpecParams {
            pod_name: "pl-test",
            pipeline_id: Uuid::nil(),
            project_id: Uuid::nil(),
            step_name: "test",
            image: "alpine:3.19",
            commands: &["echo hello".into()],
            env_vars: &[],
            repo_path: "/repos/test.git",
            git_ref: "refs/tags/v1.0",
            registry_secret: None,
        });

        let init = &pod.spec.unwrap().init_containers.unwrap()[0];
        let clone_cmd = &init.args.as_ref().unwrap()[0];
        assert!(
            clone_cmd.contains("--branch v1.0"),
            "should strip refs/tags/ prefix, got: {clone_cmd}"
        );
    }

    #[test]
    fn build_pod_spec_bare_ref_used_as_is() {
        let pod = build_pod_spec(&PodSpecParams {
            pod_name: "pl-test",
            pipeline_id: Uuid::nil(),
            project_id: Uuid::nil(),
            step_name: "test",
            image: "alpine:3.19",
            commands: &["echo hello".into()],
            env_vars: &[],
            repo_path: "/repos/test.git",
            git_ref: "main",
            registry_secret: None,
        });

        let init = &pod.spec.unwrap().init_containers.unwrap()[0];
        let clone_cmd = &init.args.as_ref().unwrap()[0];
        assert!(
            clone_cmd.contains("--branch main"),
            "bare ref should be used directly, got: {clone_cmd}"
        );
    }

    #[test]
    fn build_pod_spec_empty_commands_produce_empty_script() {
        let pod = build_pod_spec(&PodSpecParams {
            pod_name: "pl-test",
            pipeline_id: Uuid::nil(),
            project_id: Uuid::nil(),
            step_name: "test",
            image: "alpine:3.19",
            commands: &[],
            env_vars: &[],
            repo_path: "/repos/test.git",
            git_ref: "main",
            registry_secret: None,
        });

        let container = &pod.spec.unwrap().containers[0];
        let script = &container.args.as_ref().unwrap()[0];
        assert!(
            script.is_empty(),
            "empty commands should produce empty script"
        );
    }

    #[test]
    fn build_pod_spec_resource_requests() {
        let pod = build_pod_spec(&PodSpecParams {
            pod_name: "pl-test",
            pipeline_id: Uuid::nil(),
            project_id: Uuid::nil(),
            step_name: "test",
            image: "alpine:3.19",
            commands: &["true".into()],
            env_vars: &[],
            repo_path: "/repos/test.git",
            git_ref: "main",
            registry_secret: None,
        });

        let container = &pod.spec.unwrap().containers[0];
        let requests = container
            .resources
            .as_ref()
            .unwrap()
            .requests
            .as_ref()
            .unwrap();
        assert_eq!(requests["cpu"], Quantity("250m".into()));
        assert_eq!(requests["memory"], Quantity("256Mi".into()));
    }

    #[test]
    fn build_pod_spec_working_dir_is_workspace() {
        let pod = build_pod_spec(&PodSpecParams {
            pod_name: "pl-test",
            pipeline_id: Uuid::nil(),
            project_id: Uuid::nil(),
            step_name: "test",
            image: "alpine:3.19",
            commands: &["true".into()],
            env_vars: &[],
            repo_path: "/repos/test.git",
            git_ref: "main",
            registry_secret: None,
        });

        let container = &pod.spec.unwrap().containers[0];
        assert_eq!(container.working_dir.as_deref(), Some("/workspace"));
    }

    #[test]
    fn build_pod_spec_labels_include_all_three() {
        let pipeline_id = Uuid::nil();
        let project_id = Uuid::max();
        let pod = build_pod_spec(&PodSpecParams {
            pod_name: "pl-test",
            pipeline_id,
            project_id,
            step_name: "build",
            image: "alpine:3.19",
            commands: &["true".into()],
            env_vars: &[],
            repo_path: "/repos/test.git",
            git_ref: "main",
            registry_secret: None,
        });

        let labels = pod.metadata.labels.as_ref().unwrap();
        assert_eq!(labels["platform.io/pipeline"], pipeline_id.to_string());
        assert_eq!(labels["platform.io/project"], project_id.to_string());
        assert_eq!(labels["platform.io/step"], "build");
    }

    // -- build_env_vars_core --

    fn find_env(vars: &[EnvVar], name: &str) -> Option<String> {
        vars.iter()
            .find(|v| v.name == name)
            .and_then(|v| v.value.clone())
    }

    #[test]
    fn env_vars_include_all_seven_standard_vars() {
        let vars = build_env_vars_core(
            Uuid::nil(),
            Uuid::nil(),
            "my-project",
            "refs/heads/main",
            None,
            "build",
            None,
        );
        assert!(find_env(&vars, "PLATFORM_PROJECT_ID").is_some());
        assert!(find_env(&vars, "PLATFORM_PROJECT_NAME").is_some());
        assert!(find_env(&vars, "PIPELINE_ID").is_some());
        assert!(find_env(&vars, "STEP_NAME").is_some());
        assert!(find_env(&vars, "COMMIT_REF").is_some());
        assert!(find_env(&vars, "COMMIT_BRANCH").is_some());
        assert!(find_env(&vars, "PROJECT").is_some());
    }

    #[test]
    fn env_vars_commit_sha_present_when_some() {
        let vars = build_env_vars_core(
            Uuid::nil(),
            Uuid::nil(),
            "proj",
            "refs/heads/main",
            Some("abc123"),
            "test",
            None,
        );
        assert_eq!(find_env(&vars, "COMMIT_SHA"), Some("abc123".into()));
    }

    #[test]
    fn env_vars_commit_sha_absent_when_none() {
        let vars = build_env_vars_core(
            Uuid::nil(),
            Uuid::nil(),
            "proj",
            "refs/heads/main",
            None,
            "test",
            None,
        );
        assert!(find_env(&vars, "COMMIT_SHA").is_none());
    }

    #[test]
    fn env_vars_registry_present_when_configured() {
        let vars = build_env_vars_core(
            Uuid::nil(),
            Uuid::nil(),
            "proj",
            "main",
            None,
            "test",
            Some("registry.example.com"),
        );
        assert_eq!(
            find_env(&vars, "REGISTRY"),
            Some("registry.example.com".into())
        );
    }

    #[test]
    fn env_vars_registry_absent_when_none() {
        let vars =
            build_env_vars_core(Uuid::nil(), Uuid::nil(), "proj", "main", None, "test", None);
        assert!(find_env(&vars, "REGISTRY").is_none());
    }

    #[test]
    fn env_vars_branch_strips_refs_heads_prefix() {
        let vars = build_env_vars_core(
            Uuid::nil(),
            Uuid::nil(),
            "proj",
            "refs/heads/feature/login",
            None,
            "test",
            None,
        );
        assert_eq!(
            find_env(&vars, "COMMIT_BRANCH"),
            Some("feature/login".into())
        );
        assert_eq!(
            find_env(&vars, "COMMIT_REF"),
            Some("refs/heads/feature/login".into())
        );
    }

    #[test]
    fn env_vars_bare_ref_used_as_branch() {
        let vars =
            build_env_vars_core(Uuid::nil(), Uuid::nil(), "proj", "main", None, "test", None);
        assert_eq!(find_env(&vars, "COMMIT_BRANCH"), Some("main".into()));
    }

    // -- is_kaniko_image --

    #[test]
    fn detect_kaniko_image_standard() {
        assert!(is_kaniko_image("gcr.io/kaniko-project/executor:latest"));
    }

    #[test]
    fn detect_kaniko_image_case_insensitive() {
        assert!(is_kaniko_image("gcr.io/Kaniko-Project/executor:v1"));
    }

    #[test]
    fn detect_kaniko_image_substring() {
        assert!(is_kaniko_image("my-registry/kaniko-custom:v1"));
    }

    #[test]
    fn detect_kaniko_image_false_for_alpine() {
        assert!(!is_kaniko_image("alpine:3.19"));
    }

    #[test]
    fn detect_kaniko_image_false_for_rust() {
        assert!(!is_kaniko_image("rust:1.85-slim"));
    }

    // -- classify_branch --

    #[test]
    fn branch_main_classified_as_production() {
        assert_eq!(classify_branch("main"), "production");
    }

    #[test]
    fn branch_master_classified_as_production() {
        assert_eq!(classify_branch("master"), "production");
    }

    #[test]
    fn branch_feature_classified_as_preview() {
        assert_eq!(classify_branch("feature/login"), "preview");
    }

    #[test]
    fn branch_develop_classified_as_preview() {
        assert_eq!(classify_branch("develop"), "preview");
    }

    // -- build_image_ref --

    #[test]
    fn image_ref_format() {
        let r = build_image_ref("registry.example.com", "my-app", "abc123");
        assert_eq!(r, "registry.example.com/my-app:abc123");
    }

    #[test]
    fn image_ref_latest_tag() {
        let r = build_image_ref("localhost:5000", "proj", "latest");
        assert_eq!(r, "localhost:5000/proj:latest");
    }

    // -- registry secret mount --

    #[test]
    fn pod_spec_without_registry_secret_has_two_volumes() {
        let pod = build_pod_spec(&PodSpecParams {
            pod_name: "pl-test",
            pipeline_id: Uuid::nil(),
            project_id: Uuid::nil(),
            step_name: "test",
            image: "alpine:3.19",
            commands: &["true".into()],
            env_vars: &[],
            repo_path: "/repos/test.git",
            git_ref: "main",
            registry_secret: None,
        });

        let spec = pod.spec.unwrap();
        assert_eq!(spec.volumes.as_ref().unwrap().len(), 2);
        let mounts = spec.containers[0].volume_mounts.as_ref().unwrap();
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].name, "workspace");
    }

    #[test]
    fn pod_spec_with_registry_secret_adds_docker_config() {
        let pod = build_pod_spec(&PodSpecParams {
            pod_name: "pl-test",
            pipeline_id: Uuid::nil(),
            project_id: Uuid::nil(),
            step_name: "build",
            image: "gcr.io/kaniko-project/executor:latest",
            commands: &["true".into()],
            env_vars: &[],
            repo_path: "/repos/test.git",
            git_ref: "main",
            registry_secret: Some("pl-registry-00000000"),
        });

        let spec = pod.spec.unwrap();

        // Should have 3 volumes: workspace, repos, docker-config
        let volumes = spec.volumes.as_ref().unwrap();
        assert_eq!(volumes.len(), 3);
        assert_eq!(volumes[2].name, "docker-config");
        let secret_vol = volumes[2].secret.as_ref().unwrap();
        assert_eq!(
            secret_vol.secret_name.as_deref(),
            Some("pl-registry-00000000")
        );

        // Step container should have 2 mounts: workspace + docker-config
        let mounts = spec.containers[0].volume_mounts.as_ref().unwrap();
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[1].name, "docker-config");
        assert_eq!(mounts[1].mount_path, "/kaniko/.docker");
        assert_eq!(mounts[1].read_only, Some(true));
    }

    #[test]
    fn env_vars_docker_config_set_when_registry_configured() {
        let vars = build_env_vars_core(
            Uuid::nil(),
            Uuid::nil(),
            "proj",
            "main",
            None,
            "test",
            Some("registry.example.com"),
        );
        assert_eq!(
            find_env(&vars, "DOCKER_CONFIG"),
            Some("/kaniko/.docker".into())
        );
    }

    #[test]
    fn env_vars_docker_config_absent_when_no_registry() {
        let vars =
            build_env_vars_core(Uuid::nil(), Uuid::nil(), "proj", "main", None, "test", None);
        assert!(find_env(&vars, "DOCKER_CONFIG").is_none());
    }

    // -- build_volumes_and_mounts --

    #[test]
    fn volumes_without_secret_has_two() {
        let (volumes, mounts) = build_volumes_and_mounts("/data/repos/test.git", None);
        assert_eq!(volumes.len(), 2);
        assert_eq!(volumes[0].name, "workspace");
        assert_eq!(volumes[1].name, "repos");
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].name, "workspace");
        assert_eq!(mounts[0].mount_path, "/workspace");
    }

    #[test]
    fn volumes_with_secret_has_three() {
        let (volumes, mounts) = build_volumes_and_mounts("/data/repos/test.git", Some("my-secret"));
        assert_eq!(volumes.len(), 3);
        assert_eq!(volumes[2].name, "docker-config");
        let secret_vol = volumes[2].secret.as_ref().unwrap();
        assert_eq!(secret_vol.secret_name.as_deref(), Some("my-secret"));
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[1].name, "docker-config");
        assert_eq!(mounts[1].mount_path, "/kaniko/.docker");
        assert_eq!(mounts[1].read_only, Some(true));
    }

    #[test]
    fn volumes_repos_host_path() {
        let repo_path = "/tmp/platform-e2e/repos/owner/repo.git";
        let (volumes, _) = build_volumes_and_mounts(repo_path, None);
        let host_path = volumes[1].host_path.as_ref().unwrap();
        assert_eq!(host_path.path, repo_path);
        assert_eq!(host_path.type_.as_deref(), Some("Directory"));
    }

    // -- env_var helper --

    #[test]
    fn env_var_sets_name_and_value() {
        let e = env_var("FOO", "bar");
        assert_eq!(e.name, "FOO");
        assert_eq!(e.value, Some("bar".into()));
    }

    #[test]
    fn env_var_empty_value() {
        let e = env_var("EMPTY", "");
        assert_eq!(e.name, "EMPTY");
        assert_eq!(e.value, Some(String::new()));
    }

    // -- extract_exit_code additional cases --

    #[test]
    fn exit_code_zero_success() {
        let status = PodStatus {
            container_statuses: Some(vec![ContainerStatus {
                name: "step".into(),
                ready: false,
                restart_count: 0,
                image: String::new(),
                image_id: String::new(),
                state: Some(ContainerState {
                    terminated: Some(ContainerStateTerminated {
                        exit_code: 0,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }]),
            ..Default::default()
        };
        assert_eq!(extract_exit_code(&status), Some(0));
    }

    #[test]
    fn exit_code_137_oom_killed() {
        let status = PodStatus {
            container_statuses: Some(vec![ContainerStatus {
                name: "step".into(),
                ready: false,
                restart_count: 0,
                image: String::new(),
                image_id: String::new(),
                state: Some(ContainerState {
                    terminated: Some(ContainerStateTerminated {
                        exit_code: 137,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }]),
            ..Default::default()
        };
        assert_eq!(extract_exit_code(&status), Some(137));
    }

    #[test]
    fn exit_code_only_first_container() {
        // When multiple containers exist, only the first is checked
        let status = PodStatus {
            container_statuses: Some(vec![
                ContainerStatus {
                    name: "step".into(),
                    ready: false,
                    restart_count: 0,
                    image: String::new(),
                    image_id: String::new(),
                    state: Some(ContainerState {
                        terminated: Some(ContainerStateTerminated {
                            exit_code: 1,
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                ContainerStatus {
                    name: "sidecar".into(),
                    ready: false,
                    restart_count: 0,
                    image: String::new(),
                    image_id: String::new(),
                    state: Some(ContainerState {
                        terminated: Some(ContainerStateTerminated {
                            exit_code: 0,
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        };
        assert_eq!(extract_exit_code(&status), Some(1));
    }

    #[test]
    fn exit_code_waiting_state_returns_none() {
        let status = PodStatus {
            container_statuses: Some(vec![ContainerStatus {
                name: "step".into(),
                ready: false,
                restart_count: 0,
                image: String::new(),
                image_id: String::new(),
                state: Some(ContainerState {
                    waiting: Some(Default::default()),
                    terminated: None,
                    ..Default::default()
                }),
                ..Default::default()
            }]),
            ..Default::default()
        };
        assert_eq!(extract_exit_code(&status), None);
    }

    // -- pod spec additional edge cases --

    #[test]
    fn build_pod_spec_multiple_commands_joined() {
        let pod = build_pod_spec(&PodSpecParams {
            pod_name: "pl-test",
            pipeline_id: Uuid::nil(),
            project_id: Uuid::nil(),
            step_name: "test",
            image: "alpine:3.19",
            commands: &["echo a".into(), "echo b".into(), "echo c".into()],
            env_vars: &[],
            repo_path: "/repos/test.git",
            git_ref: "main",
            registry_secret: None,
        });

        let container = &pod.spec.unwrap().containers[0];
        let script = &container.args.as_ref().unwrap()[0];
        assert_eq!(script, "echo a && echo b && echo c");
    }

    #[test]
    fn build_pod_spec_single_command() {
        let pod = build_pod_spec(&PodSpecParams {
            pod_name: "pl-test",
            pipeline_id: Uuid::nil(),
            project_id: Uuid::nil(),
            step_name: "test",
            image: "alpine:3.19",
            commands: &["cargo test".into()],
            env_vars: &[],
            repo_path: "/repos/test.git",
            git_ref: "main",
            registry_secret: None,
        });

        let container = &pod.spec.unwrap().containers[0];
        let script = &container.args.as_ref().unwrap()[0];
        assert_eq!(script, "cargo test");
    }

    #[test]
    fn build_pod_spec_init_container_has_repos_mount() {
        let pod = build_pod_spec(&PodSpecParams {
            pod_name: "pl-test",
            pipeline_id: Uuid::nil(),
            project_id: Uuid::nil(),
            step_name: "test",
            image: "alpine:3.19",
            commands: &["true".into()],
            env_vars: &[],
            repo_path: "/data/repos/owner/repo.git",
            git_ref: "main",
            registry_secret: None,
        });

        let init = &pod.spec.unwrap().init_containers.unwrap()[0];
        let mounts = init.volume_mounts.as_ref().unwrap();
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0].name, "workspace");
        assert_eq!(mounts[1].name, "repos");
        assert_eq!(mounts[1].mount_path, "/data/repos/owner/repo.git");
        assert_eq!(mounts[1].read_only, Some(true));
    }

    #[test]
    fn build_pod_spec_with_env_vars() {
        let pod = build_pod_spec(&PodSpecParams {
            pod_name: "pl-test",
            pipeline_id: Uuid::nil(),
            project_id: Uuid::nil(),
            step_name: "test",
            image: "alpine:3.19",
            commands: &["echo $FOO".into()],
            env_vars: &[env_var("FOO", "bar"), env_var("BAZ", "qux")],
            repo_path: "/repos/test.git",
            git_ref: "main",
            registry_secret: None,
        });

        let container = &pod.spec.unwrap().containers[0];
        let env = container.env.as_ref().unwrap();
        assert_eq!(env.len(), 2);
        assert_eq!(env[0].name, "FOO");
        assert_eq!(env[0].value, Some("bar".into()));
    }

    // -- env_vars_core more edge cases --

    #[test]
    fn env_vars_refs_tags_stripped_for_branch() {
        let vars = build_env_vars_core(
            Uuid::nil(),
            Uuid::nil(),
            "proj",
            "refs/tags/v1.0.0",
            None,
            "test",
            None,
        );
        // refs/tags/ is NOT stripped by the branch logic — only refs/heads/ is
        assert_eq!(
            find_env(&vars, "COMMIT_BRANCH"),
            Some("refs/tags/v1.0.0".into())
        );
        assert_eq!(
            find_env(&vars, "COMMIT_REF"),
            Some("refs/tags/v1.0.0".into())
        );
    }

    #[test]
    fn env_vars_project_name_preserved_exactly() {
        let vars = build_env_vars_core(
            Uuid::nil(),
            Uuid::nil(),
            "My-App-v2",
            "main",
            None,
            "build",
            None,
        );
        assert_eq!(find_env(&vars, "PROJECT"), Some("My-App-v2".into()));
        assert_eq!(
            find_env(&vars, "PLATFORM_PROJECT_NAME"),
            Some("My-App-v2".into())
        );
    }

    #[test]
    fn env_vars_step_name_preserved() {
        let vars = build_env_vars_core(
            Uuid::nil(),
            Uuid::nil(),
            "proj",
            "main",
            None,
            "deploy-production",
            None,
        );
        assert_eq!(
            find_env(&vars, "STEP_NAME"),
            Some("deploy-production".into())
        );
    }
}
