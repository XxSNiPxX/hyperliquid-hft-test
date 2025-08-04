use ethers::signers::LocalWallet;
use hyperliquid_rust_sdk::{
    BaseUrl, ClientLimit, ClientOrder, ClientOrderRequest, ExchangeClient, ExchangeDataStatus,
    ExchangeResponseStatus, InfoClient, Message, Subscription,
};
use log::info;
use std::{
    collections::VecDeque,
    io::{self, Write},
    thread::sleep,
    time::Duration,
};
use tokio::sync::mpsc::unbounded_channel;

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
struct TradeState {
    position: Option<(String, f64, u64, f64)>, // (direction, entry price, entry time, extreme price)
    realized_pnl: f64,
    cooldown_until_ms: Option<u64>,
}

fn linear_regression_slope(data: &[f64]) -> f64 {
    let n = data.len() as f64;
    let sum_x: f64 = (0..data.len()).map(|x| x as f64).sum();
    let sum_y: f64 = data.iter().sum();
    let sum_xy: f64 = data.iter().enumerate().map(|(x, y)| x as f64 * y).sum();
    let sum_x2: f64 = (0..data.len()).map(|x| (x as f64).powi(2)).sum();

    let numerator = n * sum_xy - sum_x * sum_y;
    let denominator = n * sum_x2 - sum_x.powi(2);
    if denominator.abs() < 1e-8 {
        0.0
    } else {
        numerator / denominator
    }
}

fn price_volatility(prices: &[f64]) -> f64 {
    let mean = prices.iter().copied().sum::<f64>() / prices.len() as f64;
    let variance = prices.iter().map(|p| (p - mean).powi(2)).sum::<f64>() / prices.len() as f64;
    variance.sqrt()
}

fn compute_qty(price: f64, usd_margin: f64, leverage: f64) -> f64 {
    let notional = usd_margin * leverage;
    (notional / price * 1000.0).round() / 1000.0
}

async fn send_order(
    exchange_client: &ExchangeClient,
    asset: &str,
    is_buy: bool,
    px: f64, // Limit price
    qty: f64,
    reduce_only: bool,
    wallet: &LocalWallet,
) {
    let order = ClientOrderRequest {
        asset: asset.to_string(),
        is_buy,
        reduce_only,
        limit_px: px, // Use limit price here
        sz: qty,
        cloid: None,
        order_type: ClientOrder::Limit(ClientLimit {
            tif: "Gtc".to_string(),
        }), // Change this to Limit for maker orders
    };

    let response = exchange_client.order(order, Some(wallet)).await.unwrap();
    info!("Order placed: {response:?}");

    match response {
        ExchangeResponseStatus::Ok(exchange_response) => {
            let status = exchange_response.data.unwrap().statuses[0].clone();
            match status {
                ExchangeDataStatus::Filled(order) => info!("Order filled: {order:?}"),
                ExchangeDataStatus::Resting(order) => info!("Order resting: {order:?}"),
                _ => panic!("Unexpected status: {status:?}"),
            }
        }
        ExchangeResponseStatus::Err(e) => panic!("Order error: {e}"),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let mut info_client = InfoClient::new(None, Some(BaseUrl::Mainnet)).await.unwrap();
    let (sender, mut receiver) = unbounded_channel();

    let wallet: LocalWallet = "".parse().unwrap();
    let exchange_client =
        ExchangeClient::new(None, wallet.clone(), Some(BaseUrl::Mainnet), None, None)
            .await
            .unwrap();

    let subscription_id = info_client
        .subscribe(
            Subscription::L2Book {
                coin: "BTC".to_string(),
            },
            sender,
        )
        .await
        .unwrap();

    let mut book_buffer: VecDeque<BookSample> = VecDeque::with_capacity(240);
    let mut trade_state = TradeState {
        position: None,
        realized_pnl: 0.0,
        cooldown_until_ms: None,
    };
    let mut last_direction: Option<String> = None;
    let mut last_direction_change: u64 = 0;

    while let Some(Message::L2Book(l2_book)) = receiver.recv().await {
        let now_ms = l2_book.data.time;
        let bids = &l2_book.data.levels[0];
        let asks = &l2_book.data.levels[1];
        if bids.is_empty() || asks.is_empty() {
            continue;
        }

        let best_bid = bids[0].px.parse::<f64>().unwrap();
        let best_ask = asks[0].px.parse::<f64>().unwrap();
        let mid_price = (best_bid + best_ask) / 2.0;
        let spread = best_ask - best_bid;
        let bid_volume: f64 = bids.iter().map(|b| b.sz.parse::<f64>().unwrap()).sum();
        let ask_volume: f64 = asks.iter().map(|a| a.sz.parse::<f64>().unwrap()).sum();
        let imbalance = (bid_volume - ask_volume) / (bid_volume + ask_volume);

        book_buffer.push_back(BookSample {
            timestamp_ms: now_ms,
            mid_price,
            best_bid,
            best_ask,
            bid_volume,
            ask_volume,
        });

        if book_buffer.len() > 40 {
            book_buffer.pop_front();
        }

        if book_buffer.len() >= 10 {
            let recent_prices: Vec<f64> = book_buffer.iter().map(|b| b.mid_price).collect();
            let slope = linear_regression_slope(&recent_prices);
            let volatility = price_volatility(&recent_prices);

            let exit_duration_threshold = (3000.0 + 10000.0 * volatility.min(0.01)) as u64;
            let exit_threshold_pct = if spread < 0.5 {
                0.001
            } else if volatility > 5.0 {
                0.004
            } else {
                0.0025
            };

            let trend_direction = if slope > 0.005 {
                "long"
            } else if slope < -0.005 {
                "short"
            } else {
                "neutral"
            };
            let volume_direction = if imbalance > 0.2 {
                "long"
            } else if imbalance < -0.2 {
                "short"
            } else {
                "neutral"
            };

            let mut direction = if trend_direction == volume_direction {
                trend_direction
            } else if trend_direction != "neutral" {
                trend_direction
            } else {
                volume_direction
            };

            // Close long or short positions based on conditions
            if let Some((pos_dir, entry_price, entry_time, _)) = &mut trade_state.position {
                let duration = now_ms - *entry_time;

                match pos_dir.as_str() {
                    "long" => {
                        // Profit tracking for long position
                        let profit = mid_price - *entry_price;
                        if profit > 0.05 {
                            // Lock profits if a certain percentage is reached
                            let exit_price = best_bid;
                            trade_state.realized_pnl += profit;
                            send_order(
                                &exchange_client,
                                "BTC",
                                true,       // Close long position (limit order)
                                exit_price, // Limit price is set here
                                compute_qty(mid_price, 11.0, 20.0),
                                true, // Reduce only
                                &wallet,
                            )
                            .await;
                            trade_state.position = None;
                            trade_state.cooldown_until_ms = Some(now_ms + 10_000);
                        }

                        // Trend reversal check for long position
                        if slope < -0.005 {
                            // A negative slope indicates the market might reverse
                            let new_qty = compute_qty(mid_price, 11.0, 20.0);
                            let price = best_bid - 1.00;
                            send_order(
                                &exchange_client,
                                "BTC",
                                false,         // Short position
                                price.floor(), // Slightly above the best bid for the short limit order
                                new_qty,
                                false, // Do not reduce only
                                &wallet,
                            )
                            .await;
                            trade_state.position =
                                Some(("short".to_string(), best_bid, now_ms, best_bid));
                        }
                    }
                    "short" => {
                        // Profit tracking for short position
                        let profit = *entry_price - mid_price;
                        if profit > 0.05 {
                            // Lock profits if a certain percentage is reached
                            let exit_price = best_ask;
                            trade_state.realized_pnl += profit;
                            send_order(
                                &exchange_client,
                                "BTC",
                                false,              // Close short position
                                exit_price.floor(), // Limit price is set here
                                compute_qty(mid_price, 11.0, 20.0),
                                true, // Reduce only
                                &wallet,
                            )
                            .await;
                            trade_state.position = None;
                            trade_state.cooldown_until_ms = Some(now_ms + 10_000);
                        }

                        // Trend reversal check for short position
                        if slope > 0.005 {
                            // A positive slope indicates the market might reverse
                            let new_qty = compute_qty(mid_price, 11.0, 20.0);
                            let price = best_bid + 1.00;

                            send_order(
                                &exchange_client,
                                "BTC",
                                true,          // Long position
                                price.floor(), // Slightly below the best ask for the long limit order
                                new_qty,
                                false, // Do not reduce only
                                &wallet,
                            )
                            .await;
                            trade_state.position =
                                Some(("long".to_string(), best_ask, now_ms, best_ask));
                        }
                    }
                    _ => {}
                }
            }

            // If no position is open, attempt to enter based on current conditions
            let can_enter = trade_state
                .cooldown_until_ms
                .map_or(true, |until| now_ms >= until);

            if trade_state.position.is_none() && can_enter {
                let confidence = slope.abs() > 0.004 && volatility < 20.0;
                if confidence {
                    let qty = compute_qty(mid_price, 11.0, 20.0);
                    fn adjust_price_for_tick_size(price: f64, tick_size: f64) -> f64 {
                        let precision = (1.0 / tick_size).round() as u64; // Calculate the precision multiplier
                        (price * precision as f64).round() / precision as f64
                    }

                    let tick_size = 0.01; // Assuming the tick size is 0.01
                    if direction == "long" && spread < 5.0 {
                        let taker_price = best_ask - 1.00; // Limit price just below best ask for long order
                        let adjusted_price = adjust_price_for_tick_size(taker_price, tick_size);
                        info!("LONG IT adjusted_price: {adjusted_price:?}, qty: {qty:?}");

                        send_order(
                            &exchange_client,
                            "BTC",
                            true,
                            adjusted_price.floor(), // Limit price
                            qty,
                            false,
                            &wallet,
                        )
                        .await;

                        trade_state.position =
                            Some(("long".to_string(), best_ask, now_ms, best_ask));
                    } else if direction == "short" && spread < 5.0 {
                        let taker_price = best_bid + 1.00; // Limit price just above best bid for short order
                        let adjusted_price = adjust_price_for_tick_size(taker_price, tick_size);
                        info!("SHORT IT adjusted_price: {adjusted_price:?}, qty: {qty:?}");

                        send_order(
                            &exchange_client,
                            "BTC",
                            false,
                            adjusted_price.floor(), // Limit price
                            qty,
                            false,
                            &wallet,
                        )
                        .await;

                        trade_state.position =
                            Some(("short".to_string(), best_bid, now_ms, best_bid));
                    }
                }
            }

            // Print out the current market information and position state
            let pos_string = match &trade_state.position {
                Some((dir, price, _, _)) => format!("{} @ {:.2}", dir.to_uppercase(), price),
                None => "NONE".to_string(),
            };

            print!(
                "\r[{}] Mid: {:.2} | Spread: {:.4} | Slope: {:.5} | Pos: {} | Total PnL: {:.4}",
                chrono::Utc::now().format("%H:%M:%S%.3f"),
                mid_price,
                spread,
                slope,
                pos_string,
                trade_state.realized_pnl
            );
            io::stdout().flush().unwrap();
        }
    }

    tokio::spawn(async move {
        sleep(Duration::from_secs(30));
        info!("Unsubscribing from L2 book data");
        info_client.unsubscribe(subscription_id).await.unwrap();
    });

    Ok(())
}
