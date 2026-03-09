use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct ServiceCheck {
    ok: bool,
}

#[derive(Serialize)]
pub struct HealthChecks {
    service: ServiceCheck,
}

#[derive(Serialize)]
pub struct HealthCheck {
    status: &'static str,
    checks: HealthChecks,
}

pub async fn health_check() -> Json<HealthCheck> {
    Json(HealthCheck {
        status: "healthy",
        checks: HealthChecks {
            service: ServiceCheck { ok: true },
        },
    })
}
