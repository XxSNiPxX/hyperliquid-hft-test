use ethers::signers::LocalWallet;
use hyperliquid_rust_sdk::InfoClient;
use log::info;

async fn main() {
    env_logger::init();
    // Key was randomly generated for testing and shouldn't be used with any real funds
    let wallet: LocalWallet = "0x9fecfddf6adc2b2a3791dc9355df39108403289427f2ee53334db34d034ec32f"
        .parse()
        .unwrap();

    let exchange_client = ExchangeClient::new(None, wallet, Some(BaseUrl::Mainnet), None, None)
        .await
        .unwrap();

    // // Market open order
    // let market_open_params = MarketOrderParams {
    //     asset: "ETH",
    //     is_buy: true,
    //     sz: 0.01,
    //     px: None,
    //     slippage: Some(0.01), // 1% slippage
    //     cloid: None,
    //     wallet: None,
    // };

    // let response = exchange_client
    //     .market_open(market_open_params)
    //     .await
    //     .unwrap();
    // info!("Market open order placed: {response:?}");

    // let response = match response {
    //     ExchangeResponseStatus::Ok(exchange_response) => exchange_response,
    //     ExchangeResponseStatus::Err(e) => panic!("Error with exchange response: {e}"),
    // };
    // let status = response.data.unwrap().statuses[0].clone();
    // match status {
    //     ExchangeDataStatus::Filled(order) => info!("Order filled: {order:?}"),
    //     ExchangeDataStatus::Resting(order) => info!("Order resting: {order:?}"),
    //     _ => panic!("Unexpected status: {status:?}"),
    // };

    // // Wait for a while before closing the position
    // sleep(Duration::from_secs(10));

    // Market close order

    const ADDRESS: &str = "0x61C828aEF3BfD0f080E7f16f24847803976FD02e";

    let info_client = InfoClient::new(None, Some(BaseUrl::Mainnet)).await.unwrap();
    fn address() -> H160 {
        ADDRESS.to_string().parse().unwrap()
    }
    let user = address();
    let user_state = info_client.user_state(user).await.unwrap();
    let user_state = &user_state; // Extract the first (or only) element if you expect one response
    println!("{:?}", user_state);

    if let Some(asset_position) = user_state
        .asset_positions
        .iter()
        .find(|p| p.position.coin == "BTC")
    {
        let position = &asset_position.position;
        let position_value = position.position_value.clone();
        // Convert String to Option<f64>
        let my_float: Option<f64> = position_value.parse().ok(); // `ok()` converts Result to Option
        println!("{:?}", my_float);

        let market_close_params = MarketCloseParams {
            asset: "BTC",
            sz: None, // Close entire position
            px: None,
            slippage: Some(0.01), // 1% slippage
            cloid: None,
            wallet: None,
        };
        let response = exchange_client
            .market_close(market_close_params)
            .await
            .unwrap();
        info!("Market close order placed: {response:?}");

        let response = match response {
            ExchangeResponseStatus::Ok(exchange_response) => exchange_response,
            ExchangeResponseStatus::Err(e) => panic!("Error with exchange response: {e}"),
        };
        let status = response.data.unwrap().statuses[0].clone();
        match status {
            ExchangeDataStatus::Filled(order) => info!("Close order filled: {order:?}"),
            ExchangeDataStatus::Resting(order) => info!("Close order resting: {order:?}"),
            _ => panic!("Unexpected status: {status:?}"),
        };

        info!("{:?}", position);

        // Extract data from the position
        let coin = position.coin.clone();
        let entry_price = position.entry_px.clone().unwrap_or("N/A".to_string());
        let position_value = position.position_value.clone();
        let unrealized_pnl = position.unrealized_pnl.clone();

        // Log the position details
        info!(
            "Position found for {}: entry price: {}, position value: {}, unrealized PnL: {}",
            coin, entry_price, position_value, unrealized_pnl
        );
    } else {
        // If no position found for BTC
        info!("No position found for BTC");
    }
}
