# {{project_name}}

Built with Platform.

## Stack

- **Backend**: Python / FastAPI on port 8080
- **Database**: PostgreSQL 16
- **Observability**: OpenTelemetry (traces, logs, metrics)

## Local Development

```bash
pip install -r requirements.txt
uvicorn app.main:app --host 0.0.0.0 --port 8080 --reload
```

## API

| Endpoint    | Method | Description        |
|-------------|--------|--------------------|
| `/healthz`  | GET    | Health check       |
| `/api/...`  | *      | Application routes |

## Pipeline

- **Push to main**: builds app image
- **Merge request**: builds app + test images, runs E2E tests in ephemeral namespace
- **Auto-merge**: MR merges when CI passes, deploys to production namespace
