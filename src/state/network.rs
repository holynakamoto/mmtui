use crate::state::messages::{NetworkRequest, NetworkResponse};
use log::{debug, error};
use ncaa_api::client::NcaaApi;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;

const SPINNER_CHARS: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
pub const ERROR_CHAR: char = '!';

#[derive(Debug, Copy, Clone)]
pub struct LoadingState {
    pub is_loading: bool,
    pub spinner_char: char,
}

impl Default for LoadingState {
    fn default() -> Self {
        Self { is_loading: false, spinner_char: ' ' }
    }
}

pub struct NetworkWorker {
    client: NcaaApi,
    requests: mpsc::Receiver<NetworkRequest>,
    responses: mpsc::Sender<NetworkResponse>,
    is_loading: Arc<AtomicBool>,
}

impl NetworkWorker {
    pub fn new(
        requests: mpsc::Receiver<NetworkRequest>,
        responses: mpsc::Sender<NetworkResponse>,
    ) -> Self {
        Self {
            client: NcaaApi::new(),
            requests,
            responses,
            is_loading: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn run(mut self) {
        while let Some(request) = self.requests.recv().await {
            self.start_loading_animation().await;

            let result = match request {
                NetworkRequest::LoadBracket => self.handle_load_bracket().await,
                NetworkRequest::RefreshScores => self.handle_refresh_scores().await,
                NetworkRequest::RefreshPrizePoolBalance { address } => {
                    self.handle_refresh_prize_pool_balance(address).await
                }
                NetworkRequest::LoadGameDetail { bracket_id, espn_id } => {
                    self.handle_load_game_detail(bracket_id, espn_id).await
                }
            };

            debug!("network request complete");
            self.stop_loading_animation(result.is_ok()).await;

            let response = result.unwrap_or_else(|err| NetworkResponse::Error {
                message: err.to_string(),
            });

            if let Err(e) = self.responses.send(response).await {
                error!("Failed to send network response: {e}");
                break;
            }
        }
    }

    async fn handle_load_bracket(&self) -> Result<NetworkResponse, ncaa_api::client::ApiError> {
        debug!("loading tournament bracket");
        let tournament = self.client.fetch_tournament().await?;
        Ok(NetworkResponse::BracketLoaded { tournament })
    }

    async fn handle_refresh_scores(&self) -> Result<NetworkResponse, ncaa_api::client::ApiError> {
        debug!("refreshing scores");
        let games = self.client.fetch_scoreboard().await?;
        Ok(NetworkResponse::BracketUpdated { games })
    }

    async fn handle_refresh_prize_pool_balance(&self, address: String) -> Result<NetworkResponse, ncaa_api::client::ApiError> {
        debug!("refreshing prize pool balance for {address}");
        let url = format!("https://mempool.space/api/address/{address}");
        
        #[derive(serde::Deserialize)]
        struct MempoolAddress {
            chain_stats: MempoolStats,
        }
        #[derive(serde::Deserialize)]
        struct MempoolStats {
            funded_txo_sum: u64,
            spent_txo_sum: u64,
        }

        let resp: MempoolAddress = reqwest::get(url)
            .await
            .map_err(|e| ncaa_api::client::ApiError::Other(e.to_string()))?
            .json()
            .await
            .map_err(|e| ncaa_api::client::ApiError::Other(e.to_string()))?;

        let balance_sat = resp.chain_stats.funded_txo_sum.saturating_sub(resp.chain_stats.spent_txo_sum);
        Ok(NetworkResponse::PrizePoolBalanceUpdated { balance_sat })
    }

    async fn handle_load_game_detail(
        &self,
        bracket_id: String,
        espn_id: Option<String>,
    ) -> Result<NetworkResponse, ncaa_api::client::ApiError> {
        let Some(eid) = espn_id else {
            debug!("game detail unavailable for bracket pos {bracket_id}: no ESPN ID yet (pre-Selection Sunday)");
            return Ok(NetworkResponse::Error {
                message: "Game detail not yet available — check back after Selection Sunday.".into(),
            });
        };
        debug!("loading game detail for bracket pos {bracket_id} (espn {eid})");
        let detail = self.client.fetch_game_detail(&eid).await?;
        Ok(NetworkResponse::GameDetailLoaded { detail })
    }

    async fn start_loading_animation(&self) {
        self.is_loading.store(true, Ordering::Relaxed);

        let mut loading_state =
            LoadingState { is_loading: true, spinner_char: SPINNER_CHARS[0] };
        let _ = self
            .responses
            .send(NetworkResponse::LoadingStateChanged { loading_state })
            .await;

        let responses = self.responses.clone();
        let is_loading = self.is_loading.clone();

        tokio::spawn(async move {
            let mut spinner_index = 1;
            let mut interval = tokio::time::interval(Duration::from_millis(33));
            loop {
                interval.tick().await;
                if !is_loading.load(Ordering::Relaxed) {
                    break;
                }
                loading_state.spinner_char = SPINNER_CHARS[spinner_index];
                spinner_index = (spinner_index + 1) % SPINNER_CHARS.len();
                let _ = responses
                    .send(NetworkResponse::LoadingStateChanged { loading_state })
                    .await;
            }
        });
    }

    async fn stop_loading_animation(&self, is_ok: bool) {
        self.is_loading.store(false, Ordering::Relaxed);
        tokio::time::sleep(Duration::from_millis(15)).await;

        let spinner_char = if is_ok { ' ' } else { ERROR_CHAR };
        let _ = self
            .responses
            .send(NetworkResponse::LoadingStateChanged {
                loading_state: LoadingState { is_loading: false, spinner_char },
            })
            .await;
    }
}
