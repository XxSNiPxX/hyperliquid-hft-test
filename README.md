
# 📈 Hyperliquid Signal Engine (WIP)

This is a work-in-progress trading signal engine and quote manager using the [`hyperliquid_rust_sdk`](https://github.com/hyperliquid-markets/hyperliquid-rust-sdk). It listens to live L2 order book and trade data for a specified coin (e.g. BTC), computes a variety of market signals (momentum, TWAP deviation, decay-weighted order flow), and proposes adaptive bid/ask quotes while respecting simple position risk limits.

---

## 🚧 Status

> ⚠️ **This is a prototype. It’s not production-ready.**
> Some features may not work as intended, and edge cases or connection issues may cause the engine to break silently. Expect bugs and broken logic until further debugging is done.

---

## 💡 Features

- Live subscription to L2 orderbook and trade feeds via `hyperliquid_rust_sdk`.
- Computes:
  - **Momentum trend**
  - **TWAP** (Time-Weighted Average Price)
  - **Decay-weighted order flow imbalance**
  - **Volatility estimates**
  - **TWAP deviation** for mean-reversion detection
- Quote generation logic that adjusts:
  - **Spread** (adaptive to volatility and aggression mode)
  - **Quote size** (scaled with volatility)
- Simple **risk management** with position limits to avoid overexposure.

---

## 📦 Code Structure

| Module / Struct        | Purpose |
|------------------------|---------|
| `SignalEngine`         | Processes book and trade updates; computes signals |
| `QuoteLayerManager`    | Builds quote proposals based on current signal state |
| `RiskManager`          | Accepts or rejects quotes based on inventory limits |
| `MessageRouter`        | Routes incoming WebSocket messages to appropriate handlers |
| `main()`               | Initializes clients, subscriptions, and runs event loop |

---

## 🛠 Requirements

- Rust (stable)
- Tokio async runtime
- `hyperliquid_rust_sdk`
- `env_logger` (optional for logging)
- Internet connection (to connect to Hyperliquid)

---

## ▶️ Running the Bot

1. **Clone the repo**
   ```bash
   git clone https://github.com/XxSNiPxX/hyperliquid-hft-test
   cd hyperliquid-hft-test
   ```

2. **Add dependencies** in your `Cargo.toml`:
   ```toml
   [dependencies]
   hyperliquid_rust_sdk = "0.1"
   tokio = { version = "1", features = ["full"] }
   env_logger = "0.10"
   ```

3. **Run the bot**
   ```bash
   cargo run bin/trade_new.rs
   ```

   You should start seeing logs like:

   ```
   [Signal] Trend: 0.123 | TWAP: 29250.5 | Slide: 0.003 | NormSlide: 0.12 | FillScore: 1.0 | Dev: 0.0015 | Vol: 8.45 | Aggro: true
   [Risk] Approved Quote: QuoteProposal { side: "Buy", price: 29251.0, size: 1.5 }
   ```

---

## 🔍 Debugging Tips

- Ensure your machine has internet access and Hyperliquid endpoints are not blocked.
- If `Message::L2Book` or `Message::Trades` parsing fails, the price or size parsing (`parse::<f64>()`) might be getting malformed data — add logging there.
- Add more `.unwrap_or_else` debug messages in case of silent failure.
- Check if the timestamps are too skewed, causing signals like decay-weighted slide to zero out.

---

## 🧠 Known Issues

- No actual order submission yet — the bot **simulates fills** to track inventory.
- All fills are **assumed instant and perfect**.
- No error handling on channel drops or disconnections.
- Market data subscriptions are **hardcoded to BTC**.
- Does not reconnect on failure or resubscribe.

---

## 📌 Next Steps / TODOs

- [ ] Add logging and backoff for subscription failures
- [ ] Integrate real order execution
- [ ] Make coin configurable via CLI or env
- [ ] Add unit tests for signal calculations
- [ ] Add support for multiple symbols

---

## 📜 License

MIT — open-source and hackable. Use it to build smarter, safer trading bots.
```
