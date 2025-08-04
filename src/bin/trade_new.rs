use hyperliquid_rust_sdk::{BaseUrl, InfoClient, Message, Subscription};
use std::{
    collections::VecDeque,
    io::{self, Write},
    sync::Arc,
};
use tokio::sync::{mpsc::unbounded_channel, Mutex};
// Parameters for signal windows and thresholds
const TWAP_WINDOW: usize = 120;
const TRADE_WINDOW: usize = 80;
const DEVIATION_THRESHOLD: f64 = 0.002;
const AGGRESSIVE_SPREAD_TICKS: f64 = 0.5;
const BASE_QUOTE_SIZE: f64 = 1.0;
const POSITION_LIMIT: f64 = 5.0; // Max inventory
                                 // Market data samples
#[derive(Debug, Clone)]
pub struct BookSample {
    pub timestamp_ms: u64,
    pub mid_price: f64,
    pub best_bid: f64,
    pub best_ask: f64,
    pub bid_volume: f64,
    pub ask_volume: f64,
}
#[derive(Debug, Clone)]
pub struct TradeSample {
    pub price: f64,
    pub size: f64,
    pub is_buy: bool,
    pub timestamp_ms: u64,
}
// Internal position tracking
#[derive(Debug, Default, Clone)]
pub struct Position {
    pub base: f64,  // Asset holdings (e.g. BTC)
    pub quote: f64, // Quote currency (e.g. USD)
}
// State holding recent history and signals
#[derive(Debug, Default, Clone)]
pub struct SignalState {
    pub book_history: VecDeque<BookSample>,
    pub trade_history: VecDeque<TradeSample>,
    pub trend_score: f64,
    pub twap: f64,
    pub sliding_signal: f64,
    pub normalized_slide: f64,
    pub fill_score: f64,
    pub twap_deviation: f64,
    pub mean_revert_signal: String,
    pub best_bid: f64,
    pub best_ask: f64,
    pub volatility: f64,
    pub aggressive_mode: bool,
    pub position: Position, // track current inventory
}
// Compute standard deviation of mid-prices
pub fn compute_volatility(history: &VecDeque<BookSample>) -> f64 {
    let n = history.len();
    if n < 2 {
        return 0.0;
    }
    let mean = history.iter().map(|s| s.mid_price).sum::<f64>() / n as f64;
    let var = history
        .iter()
        .map(|s| (s.mid_price - mean).powi(2))
        .sum::<f64>()
        / n as f64;
    var.sqrt()
}
// Core signal processing engine
pub struct SignalEngine {
    pub state: SignalState,
}
impl SignalEngine {
    pub fn new() -> Self {
        Self {
            state: SignalState::default(),
        }
    }
    // Process each order-book update
    pub fn process_l2_book(
        &mut self,
        ts: u64,
        bid_px: f64,
        ask_px: f64,
        bid_vol: f64,
        ask_vol: f64,
    ) {
        // Add new book sample
        let mid = (bid_px + ask_px) / 2.0;
        self.state.book_history.push_back(BookSample {
            timestamp_ms: ts,
            mid_price: mid,
            best_bid: bid_px,
            best_ask: ask_px,
            bid_volume: bid_vol,
            ask_volume: ask_vol,
        });
        if self.state.book_history.len() > TWAP_WINDOW {
            self.state.book_history.pop_front();
        }
        // Update best prices

        self.state.best_bid = bid_px;
        self.state.best_ask = ask_px;
        // Compute signals:
        self.state.trend_score = compute_momentum(&self.state.book_history);
        self.state.twap = compute_twap(&self.state.book_history);
        self.state.twap_deviation = compute_twap_deviation(mid, self.state.twap);
        self.state.mean_revert_signal = interpret_mean_reversion(self.state.twap_deviation);
        self.state.volatility = compute_volatility(&self.state.book_history);
        // Determine aggressive mode (tight market & low vol)
        let current_spread = ask_px - bid_px;
        self.state.aggressive_mode = current_spread <= 2.0 && self.state.volatility < 10.0;
        // Compute order-flow imbalance (decay-weighted)
        let (slide, norm) = compute_decay_weighted_slide(&self.state.trade_history, ts);
        self.state.sliding_signal = slide;
        self.state.normalized_slide = norm;
        // Combine signals into final directional fill_score
        let trend_strength = self.state.trend_score.tanh();
        let micro_pressure = self.state.normalized_slide;
        self.state.fill_score = if trend_strength.abs() > 0.1 {
            trend_strength.signum()
        } else if micro_pressure.abs() > 0.4 {
            micro_pressure.signum()
        } else {
            0.0
        };
    }
    // Process trade executions for trade flow
    pub fn process_trade(&mut self, price: f64, size: f64, is_buy: bool, ts: u64) {
        self.state.trade_history.push_back(TradeSample {
            price,
            size,
            is_buy,
            timestamp_ms: ts,
        });
        if self.state.trade_history.len() > TRADE_WINDOW {
            self.state.trade_history.pop_front();
        }
    }
    // Print debug info
    pub fn print(&self) {
        let s = &self.state;
        println!(
"[Signal] Trend: {:.3} | TWAP: {:.2} | Slide: {:.3} | NormSlide: {:.3} | FillScore: {:.2} | Dev: {:.4} | Vol: {:.2} | Aggro: {}",
s.trend_score, s.twap, s.sliding_signal, s.normalized_slide,
s.fill_score, s.twap_deviation, s.volatility, s.aggressive_mode
);
        io::stdout().flush().unwrap();
    }
}
// === Signal computation helpers ===
fn compute_momentum(hist: &VecDeque<BookSample>) -> f64 {
    if hist.len() < 2 {
        return 0.0;
    }
    // Sum of last-10 price changes
    let recent: Vec<_> = hist.iter().rev().take(10).collect();
    recent
        .windows(2)
        .map(|w| w[0].mid_price - w[1].mid_price)
        .sum()
}
fn compute_twap(hist: &VecDeque<BookSample>) -> f64 {
    let n = hist.len().min(TWAP_WINDOW);
    if n == 0 {
        return 0.0;
    }
    hist.iter().rev().take(n).map(|b| b.mid_price).sum::<f64>() / n as f64
}
fn compute_decay_weighted_slide(trades: &VecDeque<TradeSample>, now: u64) -> (f64, f64) {
    let half_life_ms = 8000.0;
    let mut weighted_net = 0.0;
    let mut weighted_total = 0.0;
    for trade in trades {
        let age = (now as f64 - trade.timestamp_ms as f64).max(0.0);
        let weight = (-age.ln_1p() / half_life_ms).exp();
        let signed = if trade.is_buy { 1.0 } else { -1.0 };
        weighted_net += signed * trade.size * weight;
        weighted_total += trade.size * weight;
    }
    let norm = if weighted_total > 1e-6 {
        weighted_net / weighted_total
    } else {
        0.0
    };
    (weighted_net, norm)
}

fn compute_twap_deviation(p: f64, t: f64) -> f64 {
    if t.abs() < 1e-6 {
        0.0
    } else {
        (p - t) / t
    }
}
fn interpret_mean_reversion(d: f64) -> String {
    if d > DEVIATION_THRESHOLD {
        "Fade breakout".into()
    } else if d < -DEVIATION_THRESHOLD {
        "Scalp retracement".into()
    } else {
        "Neutral".into()
    }
}
// === Quote Construction ===
#[derive(Debug, Clone)]
pub struct QuoteProposal {
    pub side: String, // "Buy" or "Sell"
    pub price: f64,
    pub size: f64,
}
pub struct QuoteLayerManager;
impl QuoteLayerManager {
    pub fn new() -> Self {
        Self
    }
    pub fn build_quotes(signal: &SignalState) -> Vec<QuoteProposal> {
        let mut quotes = vec![];
        // Determine spread in ticks (wider if high volatility)
        let base_spread = if signal.aggressive_mode {
            AGGRESSIVE_SPREAD_TICKS
        } else {
            2.0
        };
        let spread_tick = base_spread * (1.0 + signal.volatility * 0.1).min(3.0);
        // Adaptive size (smaller in high-volatility)
        let vol_adj_size = BASE_QUOTE_SIZE * (1.0 / (1.0 + signal.volatility)).clamp(0.5, 2.0);
        if signal.aggressive_mode {
            // Quote both sides aggressively
            quotes.push(QuoteProposal {
                side: "Buy".into(),
                price: signal.best_bid + spread_tick,
                size: vol_adj_size * 1.5,
            });

            quotes.push(QuoteProposal {
                side: "Sell".into(),
                price: signal.best_ask - spread_tick,
                size: vol_adj_size * 1.5,
            });
        } else {
            // Quote only side suggested by fill_score
            if signal.fill_score > 0.1 {
                quotes.push(QuoteProposal {
                    side: "Buy".into(),
                    price: signal.best_bid + spread_tick,
                    size: vol_adj_size,
                });
            } else if signal.fill_score < -0.1 {
                quotes.push(QuoteProposal {
                    side: "Sell".into(),
                    price: signal.best_ask - spread_tick,
                    size: vol_adj_size,
                });
            }
        }
        quotes
    }
}
// === Risk Manager ===
pub struct RiskManager {
    pub max_position: f64,
}
impl RiskManager {
    pub fn new(max_position: f64) -> Self {
        Self { max_position }
    }
    // Evaluate and (optionally) execute or cancel quotes
    pub fn evaluate(&self, state: &mut SignalState, quotes: &[QuoteProposal]) {
        for q in quotes {
            let mut approved = true;
            // Simple position limit check:
            if q.side == "Buy" && state.position.base + q.size > self.max_position {
                approved = false;
            }
            if q.side == "Sell" && state.position.base - q.size < -self.max_position {
                approved = false;
            }

            if approved {
                println!("[Risk] Approved Quote: {:?}", q);
                // For demonstration, assume fill and update position
                if q.side == "Buy" {
                    state.position.base += q.size;
                    state.position.quote -= q.size * q.price;
                } else {
                    state.position.base -= q.size;
                    state.position.quote += q.size * q.price;
                }
            } else {
                println!("[Risk] Canceled Quote due to position limit: {:?}", q);
            }
        }
    }
}
// === Router for incoming messages ===
pub struct MessageRouter {
    signal: Arc<Mutex<SignalEngine>>,
    quote_mgr: Arc<QuoteLayerManager>,
    risk_mgr: Arc<RiskManager>,
}
impl MessageRouter {
    pub fn new(
        signal: Arc<Mutex<SignalEngine>>,
        quote_mgr: Arc<QuoteLayerManager>,
        risk_mgr: Arc<RiskManager>,
    ) -> Self {
        Self {
            signal,
            quote_mgr,
            risk_mgr,
        }
    }
    pub async fn handle(&self, msg: Message) {
        match msg {
            Message::L2Book(book) => {
                let bids = &book.data.levels[0];
                let asks = &book.data.levels[1];
                if bids.is_empty() || asks.is_empty() {
                    return;
                }
                // Parse top-of-book
                let bid_px = bids[0].px.parse::<f64>().unwrap_or(0.0);
                let ask_px = asks[0].px.parse::<f64>().unwrap_or(0.0);
                let bid_vol: f64 = bids.iter().map(|x| x.sz.parse().unwrap_or(0.0)).sum();
                let ask_vol: f64 = asks.iter().map(|x| x.sz.parse().unwrap_or(0.0)).sum();
                // Update signals
                let mut engine = self.signal.lock().await;
                engine.process_l2_book(book.data.time, bid_px, ask_px, bid_vol, ask_vol);
                engine.print();
                // Build and evaluate quotes
                let quotes = QuoteLayerManager::build_quotes(&engine.state);
                self.risk_mgr.evaluate(&mut engine.state, &quotes);
            }
            Message::Trades(trade_msg) => {
                let mut engine = self.signal.lock().await;
                // Update trade-based signals
                for t in trade_msg.data {
                    let price = t.px.parse::<f64>().unwrap_or(0.0);
                    let size = t.sz.parse::<f64>().unwrap_or(0.0);
                    let is_buy = t.side == "B";
                    engine.process_trade(price, size, is_buy, t.time);
                }
            }
            _ => {}
        }
    }
}
// === Main Execution ===
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let mut info_client = InfoClient::new(None, Some(BaseUrl::Mainnet)).await?;
    let (sender, mut receiver) = unbounded_channel();
    // Subscribe to L2 book and trades for BTC (example)
    info_client
        .subscribe(Subscription::L2Book { coin: "BTC".into() }, sender.clone())
        .await?;
    info_client
        .subscribe(Subscription::Trades { coin: "BTC".into() }, sender.clone())
        .await?;
    let signal_engine = Arc::new(Mutex::new(SignalEngine::new()));
    let quote_mgr = Arc::new(QuoteLayerManager::new());
    let risk_mgr = Arc::new(RiskManager::new(POSITION_LIMIT));
    let router = MessageRouter::new(signal_engine.clone(), quote_mgr, risk_mgr);
    // Event loop: route incoming messages
    while let Some(msg) = receiver.recv().await {
        router.handle(msg).await;
    }
    Ok(())
}
