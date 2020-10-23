pub mod client;
pub mod model;
mod transport;

use crate::{
    errors::OpenLimitError,
    exchange::ExchangeAccount,
    exchange::ExchangeMarketData,
    exchange::{Exchange, ExchangeEssentials, ExchangeSpec},
    exchange_info::ExchangeInfo,
    exchange_info::ExchangeInfoRetrieval,
    model::{
        AskBid, Balance, CancelAllOrdersRequest, CancelOrderRequest, Candle,
        GetHistoricRatesRequest, GetHistoricTradesRequest, GetOrderHistoryRequest, GetOrderRequest,
        GetPriceTickerRequest, Interval, Liquidity, OpenLimitOrderRequest, OpenMarketOrderRequest,
        Order, OrderBookRequest, OrderBookResponse, OrderCanceled, OrderStatus, Paginator, Side,
        Ticker, Trade, TradeHistoryRequest,
    },
    shared::{timestamp_to_naive_datetime, Result},
};
use async_trait::async_trait;

use std::convert::TryFrom;
use transport::Transport;

#[derive(Clone)]
pub struct Coinbase {
    exchange_info: ExchangeInfo,
    transport: Transport,
}

pub struct CoinbaseCredentials {
    pub api_key: String,
    pub api_secret: String,
    pub passphrase: String,
}

#[derive(Default)]
pub struct CoinbaseParameters {
    pub sandbox: bool,
    pub credentials: Option<CoinbaseCredentials>,
}

impl CoinbaseParameters {
    pub fn sandbox() -> Self {
        Self {
            sandbox: true,
            ..Default::default()
        }
    }

    pub fn prod() -> Self {
        Self {
            sandbox: false,
            ..Default::default()
        }
    }
}

#[async_trait]
impl ExchangeEssentials for Coinbase {
    type Parameters = CoinbaseParameters;

    async fn new(parameters: Self::Parameters) -> Self {
        let coinbase = match parameters.credentials {
            Some(credentials) => Coinbase {
                exchange_info: ExchangeInfo::new(),
                transport: Transport::with_credential(
                    &credentials.api_key,
                    &credentials.api_secret,
                    &credentials.passphrase,
                    parameters.sandbox,
                )
                .unwrap(),
            },
            None => Coinbase {
                exchange_info: ExchangeInfo::new(),
                transport: Transport::new(parameters.sandbox).unwrap(),
            },
        };

        coinbase.refresh_market_info().await.unwrap();
        coinbase
    }
}

#[async_trait]
impl ExchangeSpec for Exchange<Coinbase> {
    type OrderId = String;
    type TradeId = u64;
    type Pagination = u64;
}

#[async_trait]
impl ExchangeMarketData for Exchange<Coinbase> {
    async fn order_book(&self, req: &OrderBookRequest) -> Result<OrderBookResponse> {
        self.inner
            .book::<model::BookRecordL2>(&req.market_pair)
            .await
            .map(Into::into)
    }

    async fn get_price_ticker(&self, req: &GetPriceTickerRequest) -> Result<Ticker> {
        Coinbase::ticker(&self.inner, &req.market_pair)
            .await
            .map(Into::into)
    }

    async fn get_historic_rates(&self, req: &GetHistoricRatesRequest<Self>) -> Result<Vec<Candle>> {
        let params = model::CandleRequestParams::try_from(req)?;
        Coinbase::candles(&self.inner, &req.market_pair, Some(&params))
            .await
            .map(|v| v.into_iter().map(Into::into).collect())
    }

    async fn get_historic_trades(
        &self,
        _req: &GetHistoricTradesRequest<Self>,
    ) -> Result<Vec<Trade<Self>>> {
        unimplemented!("Only implemented for Nash right now");
    }
}

impl From<model::Book<model::BookRecordL2>> for OrderBookResponse {
    fn from(book: model::Book<model::BookRecordL2>) -> Self {
        Self {
            last_update_id: None,
            bids: book.bids.into_iter().map(Into::into).collect(),
            asks: book.asks.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<model::BookRecordL2> for AskBid {
    fn from(bids: model::BookRecordL2) -> Self {
        Self {
            price: bids.price,
            qty: bids.size,
        }
    }
}

impl From<model::Order> for Order<Exchange<Coinbase>> {
    fn from(order: model::Order) -> Self {
        let (price, size, order_type) = match order._type {
            model::OrderType::Limit {
                price,
                size,
                time_in_force: _,
            } => (Some(price), size, "limit"),
            model::OrderType::Market { size, funds: _ } => (None, size, "market"),
        };

        Self {
            id: order.id,
            market_pair: order.product_id,
            client_order_id: None,
            created_at: Some((order.created_at.timestamp_millis()) as u64),
            price,
            size,
            side: order.side.into(),
            status: order.status.into(),
            order_type: String::from(order_type),
        }
    }
}

#[async_trait]
impl ExchangeAccount for Exchange<Coinbase> {
    async fn limit_buy(&self, req: &OpenLimitOrderRequest) -> Result<Order<Self>> {
        Coinbase::limit_buy(&self.inner, &req.market_pair, req.size, req.price)
            .await
            .map(Into::into)
    }

    async fn limit_sell(&self, req: &OpenLimitOrderRequest) -> Result<Order<Self>> {
        Coinbase::limit_sell(&self.inner, &req.market_pair, req.size, req.price)
            .await
            .map(Into::into)
    }

    async fn market_buy(&self, req: &OpenMarketOrderRequest) -> Result<Order<Self>> {
        Coinbase::market_buy(&self.inner, &req.market_pair, req.size)
            .await
            .map(Into::into)
    }

    async fn market_sell(&self, req: &OpenMarketOrderRequest) -> Result<Order<Self>> {
        Coinbase::market_sell(&self.inner, &req.market_pair, req.size)
            .await
            .map(Into::into)
    }

    async fn cancel_order(&self, req: &CancelOrderRequest<Self>) -> Result<OrderCanceled<Self>> {
        Coinbase::cancel_order(&self.inner, req.id.clone(), req.market_pair.as_deref())
            .await
            .map(Into::into)
    }

    async fn cancel_all_orders(
        &self,
        req: &CancelAllOrdersRequest,
    ) -> Result<Vec<OrderCanceled<Self>>> {
        Coinbase::cancel_all_orders(&self.inner, req.market_pair.as_deref())
            .await
            .map(|v| v.into_iter().map(Into::into).collect())
    }

    async fn get_all_open_orders(&self) -> Result<Vec<Order<Self>>> {
        let params = model::GetOrderRequest {
            status: Some(String::from("open")),
            paginator: None,
            product_id: None,
        };

        Coinbase::get_orders(&self.inner, Some(&params))
            .await
            .map(|v| v.into_iter().map(Into::into).collect())
    }

    async fn get_order_history(
        &self,
        req: &GetOrderHistoryRequest<Self>,
    ) -> Result<Vec<Order<Self>>> {
        let req: model::GetOrderRequest = req.into();

        Coinbase::get_orders(&self.inner, Some(&req))
            .await
            .map(|v| v.into_iter().map(Into::into).collect())
    }

    async fn get_trade_history(&self, req: &TradeHistoryRequest<Self>) -> Result<Vec<Trade<Self>>> {
        let req = req.into();

        Coinbase::get_fills(&self.inner, Some(&req))
            .await
            .map(|v| v.into_iter().map(Into::into).collect())
    }

    async fn get_account_balances(
        &self,
        paginator: Option<&Paginator<Self>>,
    ) -> Result<Vec<Balance>> {
        let paginator: Option<model::Paginator> = paginator.map(|p| p.into());

        Coinbase::get_account(&self.inner, paginator.as_ref())
            .await
            .map(|v| v.into_iter().map(Into::into).collect())
    }

    async fn get_order(&self, req: &GetOrderRequest<Self>) -> Result<Order<Self>> {
        let id = req.id.clone();

        Coinbase::get_order(&self.inner, id).await.map(Into::into)
    }
}

impl From<String> for OrderCanceled<Exchange<Coinbase>> {
    fn from(id: String) -> Self {
        Self { id }
    }
}

impl From<model::Account> for Balance {
    fn from(account: model::Account) -> Self {
        Self {
            asset: account.currency,
            free: account.available,
            total: account.balance,
        }
    }
}

impl From<model::Fill> for Trade<Exchange<Coinbase>> {
    fn from(fill: model::Fill) -> Self {
        Self {
            id: fill.trade_id,
            order_id: fill.order_id,
            market_pair: fill.product_id,
            price: fill.price,
            qty: fill.size,
            fees: Some(fill.fee),
            side: match fill.side.as_str() {
                "buy" => Side::Buy,
                _ => Side::Sell,
            },
            liquidity: match fill.liquidity.as_str() {
                "M" => Some(Liquidity::Maker),
                "T" => Some(Liquidity::Taker),
                _ => None,
            },
            created_at: (fill.created_at.timestamp_millis()) as u64,
        }
    }
}

impl From<model::Ticker> for Ticker {
    fn from(ticker: model::Ticker) -> Self {
        Self {
            price: ticker.price,
        }
    }
}

impl From<model::Candle> for Candle {
    fn from(candle: model::Candle) -> Self {
        Self {
            time: candle.time * 1000,
            low: candle.low,
            high: candle.high,
            open: candle.open,
            close: candle.close,
            volume: candle.volume,
        }
    }
}

impl TryFrom<Interval> for u32 {
    type Error = OpenLimitError;
    fn try_from(value: Interval) -> Result<Self> {
        match value {
            Interval::OneMinute => Ok(60),
            Interval::FiveMinutes => Ok(300),
            Interval::FifteenMinutes => Ok(900),
            Interval::OneHour => Ok(3600),
            Interval::SixHours => Ok(21600),
            Interval::OneDay => Ok(86400),
            _ => Err(OpenLimitError::MissingParameter(format!(
                "{:?} is not supported in Coinbase",
                value,
            ))),
        }
    }
}

impl TryFrom<&GetHistoricRatesRequest<Exchange<Coinbase>>> for model::CandleRequestParams {
    type Error = OpenLimitError;
    fn try_from(params: &GetHistoricRatesRequest<Exchange<Coinbase>>) -> Result<Self> {
        let granularity = u32::try_from(params.interval)?;
        Ok(Self {
            daterange: params.paginator.clone().map(|p| p.into()),
            granularity: Some(granularity),
        })
    }
}

impl From<&GetOrderHistoryRequest<Exchange<Coinbase>>> for model::GetOrderRequest {
    fn from(req: &GetOrderHistoryRequest<Exchange<Coinbase>>) -> Self {
        Self {
            product_id: req.market_pair.clone(),
            paginator: req.paginator.clone().map(|p| p.into()),
            status: None,
        }
    }
}

impl From<Paginator<Exchange<Coinbase>>> for model::Paginator {
    fn from(paginator: Paginator<Exchange<Coinbase>>) -> Self {
        Self {
            after: paginator.after,
            before: paginator.before,
            limit: paginator.limit,
        }
    }
}

impl From<&Paginator<Exchange<Coinbase>>> for model::Paginator {
    fn from(paginator: &Paginator<Exchange<Coinbase>>) -> Self {
        Self {
            after: paginator.after,
            before: paginator.before,
            limit: paginator.limit,
        }
    }
}

impl From<Paginator<Exchange<Coinbase>>> for model::DateRange {
    fn from(paginator: Paginator<Exchange<Coinbase>>) -> Self {
        Self {
            start: paginator.start_time.map(timestamp_to_naive_datetime),
            end: paginator.end_time.map(timestamp_to_naive_datetime),
        }
    }
}

impl From<&Paginator<Exchange<Coinbase>>> for model::DateRange {
    fn from(paginator: &Paginator<Exchange<Coinbase>>) -> Self {
        Self {
            start: paginator.start_time.map(timestamp_to_naive_datetime),
            end: paginator.end_time.map(timestamp_to_naive_datetime),
        }
    }
}

impl From<&TradeHistoryRequest<Exchange<Coinbase>>> for model::GetFillsReq {
    fn from(req: &TradeHistoryRequest<Exchange<Coinbase>>) -> Self {
        Self {
            order_id: req.order_id.clone(),
            paginator: req.paginator.clone().map(|p| p.into()),
            product_id: req.market_pair.clone(),
        }
    }
}

impl From<model::OrderSide> for Side {
    fn from(req: model::OrderSide) -> Self {
        match req {
            model::OrderSide::Buy => Side::Buy,
            model::OrderSide::Sell => Side::Sell,
        }
    }
}

impl From<model::OrderStatus> for OrderStatus {
    fn from(req: model::OrderStatus) -> OrderStatus {
        match req {
            model::OrderStatus::Active => OrderStatus::Active,
            model::OrderStatus::Done => OrderStatus::Filled,
            model::OrderStatus::Open => OrderStatus::Open,
            model::OrderStatus::Pending => OrderStatus::Pending,
            model::OrderStatus::Rejected => OrderStatus::Rejected,
        }
    }
}
