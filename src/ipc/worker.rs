use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

use crate::{
    ipc::{Request, Response},
    process_man::ProcessManager,
};

pub struct WorkItem {
    req: Request,
    res: oneshot::Sender<Response>,
}

impl WorkItem {
    pub fn new(req: Request, res: oneshot::Sender<Response>) -> Self {
        Self { req, res }
    }
}

pub async fn worker_loop(
    mut rx: mpsc::Receiver<WorkItem>, 
    process_manager: Arc<ProcessManager>
) {
    while let Some(work) = rx.recv().await {
        let res = work.req.process(process_manager.clone()).await;
        let _ = work.res.send(res);
    }
}