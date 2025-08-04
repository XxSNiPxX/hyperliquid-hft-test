use ethers::types::H160;
use hyperliquid_rust_sdk::{BaseUrl, InfoClient};
use log::info;

const ADDRESS: &str = "0x61C828aEF3BfD0f080E7f16f24847803976FD02e";

#[tokio::main]
async fn main() {
    env_logger::init();
    let info_client = InfoClient::new(None, Some(BaseUrl::Mainnet)).await.unwrap();

    user_states_example(&info_client).await;
    recent_trades(&info_client).await;
    meta_example(&info_client).await;
    all_mids_example(&info_client).await;
    user_fills_example(&info_client).await;
    funding_history_example(&info_client).await;
    l2_snapshot_example(&info_client).await;
    candles_snapshot_example(&info_client).await;
    user_token_balances_example(&info_client).await;
    user_fees_example(&info_client).await;
    user_funding_example(&info_client).await;
    spot_meta_example(&info_client).await;
    spot_meta_and_asset_contexts_example(&info_client).await;
    query_order_by_oid_example(&info_client).await;
    query_referral_state_example(&info_client).await;
    historical_orders_example(&info_client).await;
}

fn address() -> H160 {
    ADDRESS.to_string().parse().unwrap()
}

async fn user_states_example(info_client: &InfoClient) {
    let user = address();
    let user_state = info_client.user_state(user).await.unwrap();
    let user_state = &user_state; // Extract the first (or only) element if you expect one response

    if let Some(asset_position) = user_state
        .asset_positions
        .iter()
        .find(|p| p.position.coin == "BTC")
    {
        let position = &asset_position.position;
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

async fn user_token_balances_example(info_client: &InfoClient) {
    let user = address();

    info!(
        "User token balances data for {user}: {:?}",
        info_client.user_token_balances(user).await.unwrap()
    );
}

async fn user_fees_example(info_client: &InfoClient) {
    let user = address();

    info!(
        "User fees data for {user}: {:?}",
        info_client.user_fees(user).await.unwrap()
    );
}

async fn recent_trades(info_client: &InfoClient) {
    let coin = "ETH";

    info!(
        "Recent trades for {coin}: {:?}",
        info_client.recent_trades(coin.to_string()).await.unwrap()
    );
}

async fn meta_example(info_client: &InfoClient) {
    info!("Metadata: {:?}", info_client.meta().await.unwrap());
}

async fn all_mids_example(info_client: &InfoClient) {
    info!("All mids: {:?}", info_client.all_mids().await.unwrap());
}

async fn user_fills_example(info_client: &InfoClient) {
    let user = address();

    info!(
        "User fills data for {user}: {:?}",
        info_client.user_fills(user).await.unwrap()
    );
}

async fn funding_history_example(info_client: &InfoClient) {
    let coin = "ETH";

    let start_timestamp = 1690540602225;
    let end_timestamp = 1690569402225;
    info!(
        "Funding data history for {coin} between timestamps {start_timestamp} and {end_timestamp}: {:?}",
        info_client.funding_history(coin.to_string(), start_timestamp, Some(end_timestamp)).await.unwrap()
    );
}

async fn l2_snapshot_example(info_client: &InfoClient) {
    let coin = "ETH";

    info!(
        "L2 snapshot data for {coin}: {:?}",
        info_client.l2_snapshot(coin.to_string()).await.unwrap()
    );
}

async fn candles_snapshot_example(info_client: &InfoClient) {
    let coin = "ETH";
    let start_timestamp = 1690540602225;
    let end_timestamp = 1690569402225;
    let interval = "1h";

    info!(
        "Candles snapshot data for {coin} between timestamps {start_timestamp} and {end_timestamp} with interval {interval}: {:?}",
        info_client
            .candles_snapshot(coin.to_string(), interval.to_string(), start_timestamp, end_timestamp)
            .await
            .unwrap()
    );
}

async fn user_funding_example(info_client: &InfoClient) {
    let user = address();
    let start_timestamp = 1690540602225;
    let end_timestamp = 1690569402225;
    info!(
        "Funding data history for {user} between timestamps {start_timestamp} and {end_timestamp}: {:?}",
        info_client.user_funding_history(user, start_timestamp, Some(end_timestamp)).await.unwrap()
    );
}

async fn spot_meta_example(info_client: &InfoClient) {
    info!("SpotMeta: {:?}", info_client.spot_meta().await.unwrap());
}

async fn spot_meta_and_asset_contexts_example(info_client: &InfoClient) {
    info!(
        "SpotMetaAndAssetContexts: {:?}",
        info_client.spot_meta_and_asset_contexts().await.unwrap()
    );
}

async fn query_order_by_oid_example(info_client: &InfoClient) {
    let user = address();
    let oid = 26342632321;
    info!(
        "Order status for {user} for oid {oid}: {:?}",
        info_client.query_order_by_oid(user, oid).await.unwrap()
    );
}

async fn query_referral_state_example(info_client: &InfoClient) {
    let user = address();
    info!(
        "Referral state for {user}: {:?}",
        info_client.query_referral_state(user).await.unwrap()
    );
}

async fn historical_orders_example(info_client: &InfoClient) {
    let user = address();
    info!(
        "Historical orders for {user}: {:?}",
        info_client.historical_orders(user).await.unwrap()
    );
}
