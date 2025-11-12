use crate::{
    models::{ApiError, ListingRequest},
    pipeline::Pipeline,
    security::AuthContext,
};
use serde::Serialize;
use std::{collections::HashMap, sync::Arc};
use tokio::{
    sync::{Mutex, mpsc},
    task::JoinHandle,
};
use uuid::Uuid;

#[derive(Clone)]
pub struct JobQueue {
    tx: mpsc::Sender<Job>,
    statuses: Arc<Mutex<HashMap<Uuid, JobState>>>,
}

#[derive(Clone)]
struct Job {
    id: Uuid,
    request: ListingRequest,
    context: AuthContext,
}

#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Completed {
        result: crate::models::ListingResponse,
    },
    Failed {
        error: String,
        stage: Option<String>,
    },
}

#[derive(Clone, Serialize)]
pub struct JobInfo {
    pub id: String,
    #[serde(flatten)]
    pub state: JobState,
}

impl JobQueue {
    pub fn spawn(pipeline: Pipeline) -> (Self, JoinHandle<()>) {
        let (tx, mut rx) = mpsc::channel::<Job>(queue_capacity_from_env());
        let statuses = Arc::new(Mutex::new(HashMap::new()));
        let statuses_bg = statuses.clone();

        let handle = tokio::spawn(async move {
            while let Some(job) = rx.recv().await {
                {
                    let mut guard = statuses_bg.lock().await;
                    guard.insert(job.id, JobState::Running);
                }

                let result = pipeline.run(job.request, Some(job.context)).await;
                let mut guard = statuses_bg.lock().await;
                match result {
                    Ok(resp) => {
                        guard.insert(job.id, JobState::Completed { result: resp });
                    }
                    Err(err) => {
                        guard.insert(
                            job.id,
                            JobState::Failed {
                                error: err.detail().to_string(),
                                stage: Some(err.stage().to_string()),
                            },
                        );
                    }
                }
            }
        });

        (Self { tx, statuses }, handle)
    }

    pub async fn enqueue_listing(
        &self,
        request: ListingRequest,
        context: AuthContext,
    ) -> Result<Uuid, ApiError> {
        let id = Uuid::new_v4();
        {
            let mut guard = self.statuses.lock().await;
            guard.insert(id, JobState::Queued);
        }
        let job = Job {
            id,
            request,
            context,
        };
        self.tx.send(job).await.map_err(|_| ApiError {
            error: "queue_send_failed".into(),
            detail: Some("worker not available".into()),
        })?;
        Ok(id)
    }

    pub async fn get(&self, id: Uuid) -> Option<JobInfo> {
        let guard = self.statuses.lock().await;
        guard.get(&id).cloned().map(|state| JobInfo {
            id: id.to_string(),
            state,
        })
    }
}

fn queue_capacity_from_env() -> usize {
    std::env::var("QUEUE_CAPACITY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(64)
}
