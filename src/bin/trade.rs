// Smart Hyperliquid Maker Bot
// Goal: Generate volume efficiently while remaining flat with minimal PnL and smart microtrading around trend

use ethers::signers::LocalWallet;
use hyperliquid_rust_sdk::{
    BaseUrl, ClientCancelRequestCloid, ClientLimit, ClientOrder, ClientOrderRequest,
    ExchangeClient, ExchangeDataStatus, ExchangeResponseStatus, InfoClient, Message, Subscription,
};
use log::info;
use std::{
    collections::{HashMap, VecDeque},
    io::{self, Write},
    time::{Duration, Instant},
};
use tokio::sync::mpsc::unbounded_channel;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct BookSample {
    timestamp_ms: u64,
    mid_price: f64,
    best_bid: f64,
    best_ask: f64,
    bid_volume: f64,
    ask_volume: f64,
}

#[derive(Debug, Clone)]
struct OrderState {
    cloid: Uuid,
    px: f64,
    sz: f64,
    is_bid: bool,
    timestamp: Instant,
}

#[derive(Debug)]
struct BotState {
    active_orders: HashMap<String, OrderState>,
    position_size: f64,
    net_volume: f64,
    realized_pnl: f64,
    cooldown_until: Option<Instant>,
    open_price: Option<f64>,
    trend_score: f64,
    book_history: VecDeque<BookSample>,
}

fn round_to_tick(price: f64, tick_size: f64) -> f64 {
    (price / tick_size).round() * tick_size
}

fn compute_qty(price: f64, balance: f64, leverage: f64) -> f64 {
    let notional = balance * leverage;
    (notional / price * 1000.0).round() / 1000.0
}

fn print_metrics(state: &BotState, mid: f64, spread: f64) {
    println!(
        "[Bot] Pos: {:.3} | PnL: {:.3} | Vol: {:.2} | Mid: {:.2} | Spr: {:.4} | Trend: {:.2}",
        state.position_size, state.realized_pnl, state.net_volume, mid, spread, state.trend_score
    );
    io::stdout().flush().unwrap();
}

async fn cancel_order(exchange_client: &ExchangeClient, asset: &str, cloid: Uuid) {
    let req = ClientCancelRequestCloid {
        asset: asset.to_string(),
        cloid,
    };
    let _ = exchange_client.cancel_by_cloid(req, None).await;
}

async fn place_maker_order(
    client: &ExchangeClient,
    wallet: &LocalWallet,
    asset: &str,
    is_bid: bool,
    px: f64,
    sz: f64,
) -> Option<OrderState> {
    let cloid = Uuid::new_v4();
    let order = ClientOrderRequest {
        asset: asset.to_string(),
        is_buy: is_bid,
        reduce_only: false,
        limit_px: px,
        sz,
        cloid: Some(cloid),
        order_type: ClientOrder::Limit(ClientLimit { tif: "Gtc".into() }),
    };
    let resp = client.order(order, Some(wallet)).await;
    match resp {
        Ok(ExchangeResponseStatus::Ok(resp)) => {
            let st = &resp.data.unwrap().statuses[0];
            match st {
                ExchangeDataStatus::Resting(_) => Some(OrderState {
                    cloid,
                    px,
                    sz,
                    is_bid,
                    timestamp: Instant::now(),
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

fn update_trend(history: &VecDeque<BookSample>) -> f64 {
    let len = history.len();
    if len < 3 {
        return 0.0;
    }
    let recent: Vec<_> = history.iter().rev().take(5).collect();
    let first = recent.last().unwrap().mid_price;
    let last = recent.first().unwrap().mid_price;
    (last - first) / first * 100.0
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let wallet: LocalWallet = "0xdeb26f70c61403d275c440c406bb4a88631b92321c100d3a96148f5360549695"
        .parse()
        .unwrap();
    let client =
        ExchangeClient::new(None, wallet.clone(), Some(BaseUrl::Testnet), None, None).await?;
    let mut info = InfoClient::new(None, Some(BaseUrl::Testnet)).await?;
    let (tx, mut rx) = unbounded_channel();
    let _sub = info
        .subscribe(Subscription::L2Book { coin: "BTC".into() }, tx)
        .await?;

    let mut state = BotState {
        active_orders: HashMap::new(),
        position_size: 0.0,
        net_volume: 0.0,
        realized_pnl: 0.0,
        cooldown_until: None,
        open_price: None,
        trend_score: 0.0,
        book_history: VecDeque::with_capacity(50),
    };

    let tick = 0.1;
    let leverage = 20.0;
    let balance = 5.5;
    let max_pos = 0.01;
    let quote_interval = Duration::from_secs(2);
    let trend_threshold = 0.02;

    while let Some(Message::L2Book(book)) = rx.recv().await {
        let bids = &book.data.levels[0];
        let asks = &book.data.levels[1];
        if bids.is_empty() || asks.is_empty() {
            continue;
        }

        let bid_px = bids[0].px.parse::<f64>()?;
        let ask_px = asks[0].px.parse::<f64>()?;
        let mid = (bid_px + ask_px) / 2.0;
        let spread = ask_px - bid_px;

        state.book_history.push_back(BookSample {
            timestamp_ms: book.data.time,
            mid_price: mid,
            best_bid: bid_px,
            best_ask: ask_px,
            bid_volume: bids.iter().map(|x| x.sz.parse::<f64>().unwrap()).sum(),
            ask_volume: asks.iter().map(|x| x.sz.parse::<f64>().unwrap()).sum(),
        });
        if state.book_history.len() > 50 {
            state.book_history.pop_front();
        }

        state.trend_score = update_trend(&state.book_history);

        for (side, order) in state.active_orders.clone() {
            if order.timestamp.elapsed() > quote_interval {
                cancel_order(&client, "BTC", order.cloid).await;
                state.active_orders.remove(&side);
            }
        }

        if let Some(open_px) = state.open_price {
            let pnl = state.position_size * (mid - open_px);
            if pnl < -3.0 {
                for order in state.active_orders.values() {
                    cancel_order(&client, "BTC", order.cloid).await;
                }
                state.active_orders.clear();
                state.position_size = 0.0;
                state.open_price = None;
                continue;
            }
        }

        // Enter long bias in uptrend
        if state.trend_score > trend_threshold && state.position_size < max_pos {
            if !state.active_orders.contains_key("bid") {
                let px = round_to_tick(bid_px, tick);
                let sz = compute_qty(px, balance, leverage);
                if let Some(order) = place_maker_order(&client, &wallet, "BTC", true, px, sz).await
                {
                    state.active_orders.insert("bid".into(), order);
                    state.open_price = Some(px);
                }
            }
        }

        // Enter short bias in downtrend
        if state.trend_score < -trend_threshold && state.position_size > -max_pos {
            if !state.active_orders.contains_key("ask") {
                let px = round_to_tick(ask_px, tick);
                let sz = compute_qty(px, balance, leverage);
                if let Some(order) = place_maker_order(&client, &wallet, "BTC", false, px, sz).await
                {
                    state.active_orders.insert("ask".into(), order);
                    state.open_price = Some(px);
                }
            }
        }

        // If no trend, ping-pong both sides
        if state.trend_score.abs() < trend_threshold {
            if !state.active_orders.contains_key("bid") {
                let px = round_to_tick(bid_px, tick);
                let sz = compute_qty(px, balance, leverage);
                if let Some(order) = place_maker_order(&client, &wallet, "BTC", true, px, sz).await
                {
                    state.active_orders.insert("bid".into(), order);
                    state.open_price = Some(px);
                }
            }
            if !state.active_orders.contains_key("ask") {
                let px = round_to_tick(ask_px, tick);
                let sz = compute_qty(px, balance, leverage);
                if let Some(order) = place_maker_order(&client, &wallet, "BTC", false, px, sz).await
                {
                    state.active_orders.insert("ask".into(), order);
                    state.open_price = Some(px);
                }
            }
        }

        print_metrics(&state, mid, spread);
    }

    Ok(())
}
