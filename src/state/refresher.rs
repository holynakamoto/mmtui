use crate::state::messages::NetworkRequest;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;

/// Periodic score refresh — every 30 seconds during the tournament.
/// Only sends RefreshScores; bracket structure is loaded once on startup.
pub struct PeriodicRefresher {
    network_requests: mpsc::Sender<NetworkRequest>,
}

impl PeriodicRefresher {
    pub fn new(network_requests: mpsc::Sender<NetworkRequest>) -> Self {
        Self { network_requests }
    }

    pub async fn run(self) {
        let mut scores_interval = interval(Duration::from_secs(30));
        // Skip the immediate first tick so startup loading isn't double-triggered.
        scores_interval.tick().await;

        loop {
            scores_interval.tick().await;
            let _ = self
                .network_requests
                .send(NetworkRequest::RefreshScores)
                .await;
        }
    }
}
