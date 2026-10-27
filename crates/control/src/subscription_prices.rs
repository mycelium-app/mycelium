use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::{enum_text, parse_enum};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::{NaiveDate, Utc};
use mycelium_proto::conversation::AgentId;
use mycelium_proto::routing::{AccountPrice, Price, PricePeriod, SetAccountPriceRequest, YourPrice};
use serde_json::Value;
use std::time::Duration;
use uuid::Uuid;

const RATE_TIMEOUT: Duration = Duration::from_secs(5);
const STALE_RATE_DAYS: i64 = 7;
const MONTHS_IN_YEAR: f64 = 12.0;

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<AccountPrice>>> {
    let rows = sqlx::query_as::<_, (String, String, f64, String, String)>(
        "select agent, account, price_amount, price_currency, price_period from account_settings where owner = $1 and price_amount is not null order by agent, account",
    )
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;
    let mut prices = Vec::new();

    for (agent, account, amount, currency, period) in rows {
        prices.push(AccountPrice { agent: parse_enum(&agent)?, account, amount, currency, period: parse_enum(&period)? });
    }

    Ok(Json(prices))
}

pub async fn set(State(state): State<AppState>, user: AuthUser, Json(request): Json<SetAccountPriceRequest>) -> ApiResult<StatusCode> {
    let account = request.account.trim();

    if account.is_empty() {
        return Err(ApiError::bad_request("account_required", "Choose the account to change."));
    }

    let Some(price) = request.price else {
        sqlx::query("update account_settings set price_amount = null, price_currency = null, price_period = null where owner = $1 and agent = $2 and account = $3")
            .bind(user.id)
            .bind(enum_text(&request.agent))
            .bind(account)
            .execute(&state.pool)
            .await?;

        return Ok(StatusCode::NO_CONTENT);
    };
    let price = valid_price(price)?;

    sqlx::query(
        "insert into account_settings (owner, agent, account, price_amount, price_currency, price_period) values ($1, $2, $3, $4, $5, $6)
         on conflict (owner, agent, account) do update set price_amount = excluded.price_amount, price_currency = excluded.price_currency, price_period = excluded.price_period",
    )
    .bind(user.id)
    .bind(enum_text(&request.agent))
    .bind(account)
    .bind(price.amount)
    .bind(&price.currency)
    .bind(enum_text(&price.period))
    .execute(&state.pool)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

fn valid_price(price: Price) -> ApiResult<Price> {
    let currency = price.currency.trim().to_uppercase();

    if !price.amount.is_finite() || price.amount <= 0.0 {
        return Err(ApiError::bad_request("price_amount_invalid", "Enter an amount greater than 0, like 1800 or 150.50."));
    }

    if currency.len() != 3 || !currency.chars().all(|letter| letter.is_ascii_uppercase()) {
        return Err(ApiError::bad_request("price_currency_invalid", "Choose a currency from the list."));
    }

    Ok(Price { currency, ..price })
}

pub fn monthly_amount(price: &Price) -> f64 {
    match price.period {
        PricePeriod::Monthly => price.amount,
        PricePeriod::Yearly => price.amount / MONTHS_IN_YEAR,
    }
}

pub async fn your_price(state: &AppState, owner: Uuid, agent: AgentId, account: &str) -> ApiResult<Option<YourPrice>> {
    let stored = sqlx::query_as::<_, (f64, String, String)>(
        "select price_amount, price_currency, price_period from account_settings where owner = $1 and agent = $2 and account = $3 and price_amount is not null",
    )
    .bind(owner)
    .bind(enum_text(&agent))
    .bind(account)
    .fetch_optional(&state.pool)
    .await?;
    let Some((amount, currency, period)) = stored else {
        return Ok(None);
    };
    let price = Price { amount, currency, period: parse_enum(&period)? };
    let monthly = monthly_amount(&price);
    let rate = usd_per_unit(state, &price.currency, Utc::now().date_naive()).await;

    Ok(Some(YourPrice {
        amount: price.amount,
        period: price.period,
        monthly_amount: monthly,
        monthly_usd: rate.map(|(rate, _)| monthly * rate),
        rate_date: rate.map(|(_, day)| day.to_string()),
        currency: price.currency,
    }))
}

async fn usd_per_unit(state: &AppState, currency: &str, today: NaiveDate) -> Option<(f64, NaiveDate)> {
    if currency == "USD" {
        return Some((1.0, today));
    }

    let cached = sqlx::query_as::<_, (NaiveDate, f64)>("select day, usd_per_unit from exchange_rates where currency = $1 and day >= $2 order by day desc limit 1")
        .bind(currency)
        .bind(today - chrono::Duration::days(STALE_RATE_DAYS))
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten();

    if let Some((day, rate)) = cached.filter(|(day, _)| *day == today) {
        return Some((rate, day));
    }

    match fetch_rate(state, currency).await {
        Some(rate) => {
            let stored = sqlx::query("insert into exchange_rates (day, currency, usd_per_unit) values ($1, $2, $3) on conflict do nothing").bind(today).bind(currency).bind(rate).execute(&state.pool).await;

            if let Err(cause) = stored {
                tracing::warn!("could not cache the {currency} exchange rate: {cause}");
            }

            Some((rate, today))
        }
        None => cached.map(|(day, rate)| (rate, day)),
    }
}

async fn fetch_rate(state: &AppState, currency: &str) -> Option<f64> {
    let url = format!("{}/v1/latest?base={currency}&symbols=USD", state.config.exchange_rates_url.trim_end_matches('/'));
    let response = state.http.get(url).timeout(RATE_TIMEOUT).send().await.ok()?;

    if !response.status().is_success() {
        return None;
    }

    let body = response.json::<Value>().await.ok()?;

    body["rates"]["USD"].as_f64().filter(|rate| *rate > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn price(amount: f64, currency: &str, period: PricePeriod) -> Price {
        Price { amount, currency: currency.into(), period }
    }

    #[test]
    fn a_yearly_price_is_a_twelfth_each_month() {
        assert_eq!(monthly_amount(&price(1800.0, "GBP", PricePeriod::Yearly)), 150.0);
        assert_eq!(monthly_amount(&price(150.0, "GBP", PricePeriod::Monthly)), 150.0);
    }

    #[test]
    fn a_price_needs_a_positive_amount_and_a_three_letter_currency() {
        assert!(valid_price(price(0.0, "GBP", PricePeriod::Monthly)).is_err());
        assert!(valid_price(price(f64::NAN, "GBP", PricePeriod::Monthly)).is_err());
        assert!(valid_price(price(10.0, "pounds", PricePeriod::Monthly)).is_err());
        assert_eq!(valid_price(price(10.0, " gbp ", PricePeriod::Monthly)).unwrap().currency, "GBP");
    }
}
