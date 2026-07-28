use rusqlite::{params, OptionalExtension, Result, Row};
use serde::{Deserialize, Serialize};

use super::{now_ts, Storage};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSource {
    pub id: String,
    pub product_id: String,
    pub tags_json: String,
    pub merchant: Option<String>,
    pub enabled: bool,
    pub verify_enabled: bool,
    pub last_successful_sync_id: i64,
    pub last_sync_at: Option<i64>,
    pub last_sync_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceOffer {
    pub offer_key: String,
    pub source_config_id: String,
    pub offer_id: String,
    pub product_id: String,
    pub source_id: Option<String>,
    pub source_name: Option<String>,
    pub source_included_at: Option<String>,
    pub source_shop_created_at: Option<String>,
    pub collector_kind: Option<String>,
    pub title: Option<String>,
    pub price: Option<f64>,
    pub listed_price: Option<f64>,
    pub currency: String,
    pub raw_status: Option<String>,
    pub effective_status: Option<String>,
    pub freshness_status: Option<String>,
    pub priceai_updated_at: Option<String>,
    pub expires_at: Option<String>,
    pub url: Option<String>,
    pub tags_json: String,
    pub filter_tags_json: String,
    pub stock_count: Option<i64>,
    pub raw_json: String,
    pub local_status: String,
    pub local_checked_at: Option<i64>,
    pub local_error: Option<String>,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub updated_at: i64,
    pub is_current: bool,
    /// Derived by the service for UI grouping; it is not stored in marketplace_offers.
    pub merchant_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceOfferChange {
    pub id: i64,
    pub offer_key: String,
    pub change_type: String,
    pub summary_json: String,
    pub created_at: i64,
    pub notified_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceAlertRule {
    pub id: String,
    pub name: String,
    pub source_config_id: Option<String>,
    pub product_id: Option<String>,
    pub tags_json: String,
    pub merchant: Option<String>,
    pub currency: String,
    pub max_price: Option<f64>,
    pub drop_amount: Option<f64>,
    pub drop_percent: Option<f64>,
    pub notify_restock: bool,
    pub notify_verified: bool,
    pub notify_invalid_link: bool,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceAlertState {
    pub rule_id: String,
    pub offer_key: String,
    pub signature: String,
    pub condition_active: bool,
    pub baseline_ready: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceFavoriteMerchant {
    pub merchant_key: String,
    pub source_id: Option<String>,
    pub source_name: Option<String>,
    pub collector_kind: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct MarketplaceSourceInput {
    pub id: String,
    pub product_id: String,
    pub tags_json: String,
    pub merchant: Option<String>,
    pub enabled: bool,
    pub verify_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct MarketplaceOfferInput {
    pub offer_key: String,
    pub source_config_id: String,
    pub offer_id: String,
    pub product_id: String,
    pub source_id: Option<String>,
    pub source_name: Option<String>,
    pub source_included_at: Option<String>,
    pub source_shop_created_at: Option<String>,
    pub collector_kind: Option<String>,
    pub title: Option<String>,
    pub price: Option<f64>,
    pub listed_price: Option<f64>,
    pub currency: String,
    pub raw_status: Option<String>,
    pub effective_status: Option<String>,
    pub freshness_status: Option<String>,
    pub priceai_updated_at: Option<String>,
    pub expires_at: Option<String>,
    pub url: Option<String>,
    pub tags_json: String,
    pub filter_tags_json: String,
    pub stock_count: Option<i64>,
    pub raw_json: String,
    pub local_status: String,
    pub local_checked_at: Option<i64>,
    pub local_error: Option<String>,
    pub last_seen_sync_id: i64,
}

#[derive(Debug, Clone)]
pub struct MarketplaceAlertRuleInput {
    pub id: String,
    pub name: String,
    pub source_config_id: Option<String>,
    pub product_id: Option<String>,
    pub tags_json: String,
    pub merchant: Option<String>,
    pub currency: String,
    pub max_price: Option<f64>,
    pub drop_amount: Option<f64>,
    pub drop_percent: Option<f64>,
    pub notify_restock: bool,
    pub notify_verified: bool,
    pub notify_invalid_link: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct MarketplaceFavoriteMerchantInput {
    pub merchant_key: String,
    pub source_id: Option<String>,
    pub source_name: Option<String>,
    pub collector_kind: Option<String>,
}

const OFFER_SELECT: &str = "o.offer_key,o.source_config_id,o.offer_id,o.product_id,o.source_id,o.source_name,o.source_included_at,o.source_shop_created_at,o.collector_kind,o.title,o.price,o.listed_price,o.currency,o.raw_status,o.effective_status,o.freshness_status,o.priceai_updated_at,o.expires_at,o.url,o.tags_json,o.filter_tags_json,o.stock_count,o.raw_json,o.local_status,o.local_checked_at,o.local_error,o.first_seen_at,o.last_seen_at,o.updated_at,CASE WHEN o.last_seen_sync_id>=s.last_successful_sync_id THEN 1 ELSE 0 END AS is_current";

fn map_offer(row: &Row<'_>) -> Result<MarketplaceOffer> {
    Ok(MarketplaceOffer {
        offer_key: row.get(0)?,
        source_config_id: row.get(1)?,
        offer_id: row.get(2)?,
        product_id: row.get(3)?,
        source_id: row.get(4)?,
        source_name: row.get(5)?,
        source_included_at: row.get(6)?,
        source_shop_created_at: row.get(7)?,
        collector_kind: row.get(8)?,
        title: row.get(9)?,
        price: row.get(10)?,
        listed_price: row.get(11)?,
        currency: row.get(12)?,
        raw_status: row.get(13)?,
        effective_status: row.get(14)?,
        freshness_status: row.get(15)?,
        priceai_updated_at: row.get(16)?,
        expires_at: row.get(17)?,
        url: row.get(18)?,
        tags_json: row.get(19)?,
        filter_tags_json: row.get(20)?,
        stock_count: row.get(21)?,
        raw_json: row.get(22)?,
        local_status: row.get(23)?,
        local_checked_at: row.get(24)?,
        local_error: row.get(25)?,
        first_seen_at: row.get(26)?,
        last_seen_at: row.get(27)?,
        updated_at: row.get(28)?,
        is_current: row.get::<_, i64>(29)? != 0,
        merchant_key: None,
    })
}

impl Storage {
    pub fn marketplace_sources(&self) -> Result<Vec<MarketplaceSource>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,product_id,tags_json,merchant,enabled,verify_enabled,last_successful_sync_id,last_sync_at,last_sync_error,created_at,updated_at
             FROM marketplace_sources
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(MarketplaceSource {
                id: row.get(0)?,
                product_id: row.get(1)?,
                tags_json: row.get(2)?,
                merchant: row.get(3)?,
                enabled: row.get::<_, i64>(4)? != 0,
                verify_enabled: row.get::<_, i64>(5)? != 0,
                last_successful_sync_id: row.get(6)?,
                last_sync_at: row.get(7)?,
                last_sync_error: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })?;
        rows.collect()
    }

    pub fn marketplace_source_upsert(
        &self,
        input: &MarketplaceSourceInput,
    ) -> Result<MarketplaceSource> {
        let now = now_ts();
        self.conn.execute(
            "INSERT INTO marketplace_sources
             (id,product_id,tags_json,merchant,enabled,verify_enabled,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?7)
             ON CONFLICT(id) DO UPDATE SET
               product_id=excluded.product_id,
               tags_json=excluded.tags_json,
               merchant=excluded.merchant,
               enabled=excluded.enabled,
               verify_enabled=excluded.verify_enabled,
               updated_at=excluded.updated_at",
            params![
                &input.id,
                &input.product_id,
                &input.tags_json,
                &input.merchant,
                input.enabled as i64,
                input.verify_enabled as i64,
                now,
            ],
        )?;
        self.marketplace_sources()?
            .into_iter()
            .find(|source| source.id == input.id)
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn marketplace_source_delete(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM marketplace_offers WHERE source_config_id=?1",
            [id],
        )?;
        self.conn
            .execute("DELETE FROM marketplace_sources WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn marketplace_source_sync_succeeded(&self, id: &str, sync_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE marketplace_sources
             SET last_successful_sync_id=?2,last_sync_at=?3,last_sync_error=NULL
             WHERE id=?1",
            params![id, sync_id, now_ts()],
        )?;
        Ok(())
    }

    pub fn marketplace_source_partial_sync_succeeded(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE marketplace_sources SET last_sync_at=?2,last_sync_error=NULL WHERE id=?1",
            params![id, now_ts()],
        )?;
        Ok(())
    }

    pub fn marketplace_source_sync_failed(&self, id: &str, error: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE marketplace_sources SET last_sync_at=?2,last_sync_error=?3 WHERE id=?1",
            params![id, now_ts(), error],
        )?;
        Ok(())
    }

    pub fn marketplace_offers(
        &self,
        source_config_id: Option<&str>,
        product_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<MarketplaceOffer>> {
        let sql = format!(
            "SELECT {OFFER_SELECT}
             FROM marketplace_offers o
             JOIN marketplace_sources s ON s.id=o.source_config_id
             WHERE (?1 IS NULL OR o.source_config_id=?1)
               AND (?2 IS NULL OR o.product_id=?2)
             ORDER BY is_current DESC,
               CASE
                 WHEN o.local_status='available' THEN 0
                 WHEN o.raw_status<>'out_of_stock'
                   AND o.price IS NOT NULL
                   AND o.url IS NOT NULL AND o.url<>''
                   AND (o.effective_status IS NULL OR o.effective_status NOT IN ('unavailable','stale','failed'))
                   AND (o.freshness_status IS NULL OR o.freshness_status NOT IN ('expired','failed'))
                   AND (o.expires_at IS NULL OR unixepoch(o.expires_at)>unixepoch('now'))
                   THEN 1
                 ELSE 2
               END,
               o.price IS NULL,o.price ASC,o.updated_at DESC
             LIMIT ?3"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![source_config_id, product_id, limit.clamp(1, 10_000)],
            map_offer,
        )?;
        rows.collect()
    }

    pub fn marketplace_offer_by_key(&self, key: &str) -> Result<Option<MarketplaceOffer>> {
        let sql = format!(
            "SELECT {OFFER_SELECT}
             FROM marketplace_offers o
             JOIN marketplace_sources s ON s.id=o.source_config_id
             WHERE o.offer_key=?1"
        );
        self.conn.query_row(&sql, [key], map_offer).optional()
    }

    pub fn marketplace_offer_upsert(
        &self,
        input: &MarketplaceOfferInput,
    ) -> Result<Option<MarketplaceOffer>> {
        let previous = self.marketplace_offer_by_key(&input.offer_key)?;
        let now = now_ts();
        self.conn.execute(
            "INSERT INTO marketplace_offers
             (offer_key,source_config_id,offer_id,product_id,source_id,source_name,source_included_at,source_shop_created_at,collector_kind,title,price,listed_price,currency,raw_status,effective_status,freshness_status,priceai_updated_at,expires_at,url,tags_json,filter_tags_json,stock_count,raw_json,local_status,local_checked_at,local_error,first_seen_at,last_seen_at,updated_at,last_seen_sync_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,COALESCE((SELECT first_seen_at FROM marketplace_offers WHERE offer_key=?1),?27),?27,?27,?28)
             ON CONFLICT(offer_key) DO UPDATE SET
               source_config_id=excluded.source_config_id,
               offer_id=excluded.offer_id,
               product_id=excluded.product_id,
               source_id=excluded.source_id,
               source_name=excluded.source_name,
               source_included_at=COALESCE(excluded.source_included_at,marketplace_offers.source_included_at),
               source_shop_created_at=COALESCE(excluded.source_shop_created_at,marketplace_offers.source_shop_created_at),
               collector_kind=excluded.collector_kind,
               title=excluded.title,
               price=excluded.price,
               listed_price=excluded.listed_price,
               currency=excluded.currency,
               raw_status=excluded.raw_status,
               effective_status=excluded.effective_status,
               freshness_status=excluded.freshness_status,
               priceai_updated_at=excluded.priceai_updated_at,
               expires_at=excluded.expires_at,
               url=excluded.url,
               tags_json=excluded.tags_json,
               filter_tags_json=excluded.filter_tags_json,
               stock_count=excluded.stock_count,
               raw_json=excluded.raw_json,
               local_status=excluded.local_status,
               local_checked_at=excluded.local_checked_at,
               local_error=excluded.local_error,
               last_seen_at=excluded.last_seen_at,
               updated_at=excluded.updated_at,
               last_seen_sync_id=excluded.last_seen_sync_id",
            params![
                &input.offer_key,
                &input.source_config_id,
                &input.offer_id,
                &input.product_id,
                &input.source_id,
                &input.source_name,
                &input.source_included_at,
                &input.source_shop_created_at,
                &input.collector_kind,
                &input.title,
                input.price,
                input.listed_price,
                &input.currency,
                &input.raw_status,
                &input.effective_status,
                &input.freshness_status,
                &input.priceai_updated_at,
                &input.expires_at,
                &input.url,
                &input.tags_json,
                &input.filter_tags_json,
                input.stock_count,
                &input.raw_json,
                &input.local_status,
                input.local_checked_at,
                &input.local_error,
                now,
                input.last_seen_sync_id,
            ],
        )?;
        Ok(previous)
    }

    pub fn marketplace_offer_verification_update(
        &self,
        key: &str,
        local_status: &str,
        local_error: Option<&str>,
    ) -> Result<Option<MarketplaceOffer>> {
        self.conn.execute(
            "UPDATE marketplace_offers
             SET local_status=?2,local_checked_at=?3,local_error=?4,updated_at=?3
             WHERE offer_key=?1",
            params![key, local_status, now_ts(), local_error],
        )?;
        self.marketplace_offer_by_key(key)
    }

    pub fn marketplace_change_insert(
        &self,
        offer_key: &str,
        change_type: &str,
        summary_json: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO marketplace_offer_changes
             (offer_key,change_type,summary_json,created_at)
             VALUES (?1,?2,?3,?4)",
            params![offer_key, change_type, summary_json, now_ts()],
        )?;
        Ok(())
    }

    pub fn marketplace_changes(&self, limit: i64) -> Result<Vec<MarketplaceOfferChange>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,offer_key,change_type,summary_json,created_at,notified_at
             FROM marketplace_offer_changes
             ORDER BY id DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit.clamp(1, 1_000)], |row| {
            Ok(MarketplaceOfferChange {
                id: row.get(0)?,
                offer_key: row.get(1)?,
                change_type: row.get(2)?,
                summary_json: row.get(3)?,
                created_at: row.get(4)?,
                notified_at: row.get(5)?,
            })
        })?;
        rows.collect()
    }

    pub fn marketplace_rules(&self) -> Result<Vec<MarketplaceAlertRule>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,name,source_config_id,product_id,tags_json,merchant,currency,max_price,drop_amount,drop_percent,notify_restock,notify_verified,notify_invalid_link,enabled,created_at,updated_at
             FROM marketplace_alert_rules
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(MarketplaceAlertRule {
                id: row.get(0)?,
                name: row.get(1)?,
                source_config_id: row.get(2)?,
                product_id: row.get(3)?,
                tags_json: row.get(4)?,
                merchant: row.get(5)?,
                currency: row.get(6)?,
                max_price: row.get(7)?,
                drop_amount: row.get(8)?,
                drop_percent: row.get(9)?,
                notify_restock: row.get::<_, i64>(10)? != 0,
                notify_verified: row.get::<_, i64>(11)? != 0,
                notify_invalid_link: row.get::<_, i64>(12)? != 0,
                enabled: row.get::<_, i64>(13)? != 0,
                created_at: row.get(14)?,
                updated_at: row.get(15)?,
            })
        })?;
        rows.collect()
    }

    pub fn marketplace_rule_upsert(
        &self,
        input: &MarketplaceAlertRuleInput,
    ) -> Result<MarketplaceAlertRule> {
        let now = now_ts();
        self.conn.execute(
            "INSERT INTO marketplace_alert_rules
             (id,name,source_config_id,product_id,tags_json,merchant,currency,max_price,drop_amount,drop_percent,notify_restock,notify_verified,notify_invalid_link,enabled,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,COALESCE((SELECT created_at FROM marketplace_alert_rules WHERE id=?1),?15),?15)
             ON CONFLICT(id) DO UPDATE SET
               name=excluded.name,
               source_config_id=excluded.source_config_id,
               product_id=excluded.product_id,
               tags_json=excluded.tags_json,
               merchant=excluded.merchant,
               currency=excluded.currency,
               max_price=excluded.max_price,
               drop_amount=excluded.drop_amount,
               drop_percent=excluded.drop_percent,
               notify_restock=excluded.notify_restock,
               notify_verified=excluded.notify_verified,
               notify_invalid_link=excluded.notify_invalid_link,
               enabled=excluded.enabled,
               updated_at=excluded.updated_at",
            params![
                &input.id,
                &input.name,
                &input.source_config_id,
                &input.product_id,
                &input.tags_json,
                &input.merchant,
                &input.currency,
                input.max_price,
                input.drop_amount,
                input.drop_percent,
                input.notify_restock as i64,
                input.notify_verified as i64,
                input.notify_invalid_link as i64,
                input.enabled as i64,
                now,
            ],
        )?;
        self.marketplace_rules()?
            .into_iter()
            .find(|rule| rule.id == input.id)
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn marketplace_rule_delete(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM marketplace_alert_state WHERE rule_id=?1", [id])?;
        self.conn
            .execute("DELETE FROM marketplace_alert_rules WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn marketplace_alert_state_get(
        &self,
        rule_id: &str,
        offer_key: &str,
    ) -> Result<Option<MarketplaceAlertState>> {
        self.conn
            .query_row(
                "SELECT rule_id,offer_key,signature,condition_active,baseline_ready,updated_at
                 FROM marketplace_alert_state
                 WHERE rule_id=?1 AND offer_key=?2",
                params![rule_id, offer_key],
                |row| {
                    Ok(MarketplaceAlertState {
                        rule_id: row.get(0)?,
                        offer_key: row.get(1)?,
                        signature: row.get(2)?,
                        condition_active: row.get::<_, i64>(3)? != 0,
                        baseline_ready: row.get::<_, i64>(4)? != 0,
                        updated_at: row.get(5)?,
                    })
                },
            )
            .optional()
    }

    pub fn marketplace_alert_state_put(&self, state: &MarketplaceAlertState) -> Result<()> {
        self.conn.execute(
            "INSERT INTO marketplace_alert_state
             (rule_id,offer_key,signature,condition_active,baseline_ready,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(rule_id,offer_key) DO UPDATE SET
               signature=excluded.signature,
               condition_active=excluded.condition_active,
               baseline_ready=excluded.baseline_ready,
               updated_at=excluded.updated_at",
            params![
                &state.rule_id,
                &state.offer_key,
                &state.signature,
                state.condition_active as i64,
                state.baseline_ready as i64,
                state.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn marketplace_setting_get(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT value FROM marketplace_settings WHERE key=?1",
                [key],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn marketplace_setting_set(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO marketplace_settings(key,value,updated_at)
             VALUES (?1,?2,?3)
             ON CONFLICT(key) DO UPDATE SET
               value=excluded.value,
               updated_at=excluded.updated_at",
            params![key, value, now_ts()],
        )?;
        Ok(())
    }

    pub fn marketplace_favorite_merchants(&self) -> Result<Vec<MarketplaceFavoriteMerchant>> {
        let mut stmt = self.conn.prepare(
            "SELECT merchant_key,source_id,source_name,collector_kind,created_at,updated_at
             FROM marketplace_favorite_merchants
             ORDER BY updated_at DESC,merchant_key ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(MarketplaceFavoriteMerchant {
                merchant_key: row.get(0)?,
                source_id: row.get(1)?,
                source_name: row.get(2)?,
                collector_kind: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        rows.collect()
    }

    pub fn marketplace_favorite_merchant_set(
        &self,
        input: &MarketplaceFavoriteMerchantInput,
        favorite: bool,
    ) -> Result<()> {
        if favorite {
            let now = now_ts();
            self.conn.execute(
                "INSERT INTO marketplace_favorite_merchants
                 (merchant_key,source_id,source_name,collector_kind,created_at,updated_at)
                 VALUES (?1,?2,?3,?4,?5,?5)
                 ON CONFLICT(merchant_key) DO UPDATE SET
                   source_id=excluded.source_id,
                   source_name=excluded.source_name,
                   collector_kind=excluded.collector_kind,
                   updated_at=excluded.updated_at",
                params![
                    &input.merchant_key,
                    &input.source_id,
                    &input.source_name,
                    &input.collector_kind,
                    now,
                ],
            )?;
        } else {
            self.conn.execute(
                "DELETE FROM marketplace_favorite_merchants WHERE merchant_key=?1",
                [&input.merchant_key],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(key: &str, price: f64, raw_available: bool) -> MarketplaceOfferInput {
        MarketplaceOfferInput {
            offer_key: key.to_string(),
            source_config_id: "source-1".to_string(),
            offer_id: key.to_string(),
            product_id: "chatgpt-plus".to_string(),
            source_id: Some("merchant-1".to_string()),
            source_name: Some("Merchant".to_string()),
            source_included_at: None,
            source_shop_created_at: None,
            collector_kind: Some("shopApi".to_string()),
            title: Some(key.to_string()),
            price: Some(price),
            listed_price: None,
            currency: "CNY".to_string(),
            raw_status: Some(
                if raw_available {
                    "in_stock"
                } else {
                    "out_of_stock"
                }
                .to_string(),
            ),
            effective_status: Some(
                if raw_available {
                    "available"
                } else {
                    "unavailable"
                }
                .to_string(),
            ),
            freshness_status: Some("fresh".to_string()),
            priceai_updated_at: None,
            expires_at: None,
            url: Some(format!("https://example.com/{key}")),
            tags_json: "[]".to_string(),
            filter_tags_json: "[]".to_string(),
            stock_count: None,
            raw_json: "{}".to_string(),
            local_status: "unknown".to_string(),
            local_checked_at: None,
            local_error: None,
            last_seen_sync_id: 100,
        }
    }

    fn storage() -> Storage {
        let storage = Storage::open_in_memory().expect("open marketplace storage");
        storage.init().expect("initialize marketplace storage");
        storage
            .marketplace_source_upsert(&MarketplaceSourceInput {
                id: "source-1".to_string(),
                product_id: "chatgpt-plus".to_string(),
                tags_json: "[]".to_string(),
                merchant: None,
                enabled: true,
                verify_enabled: true,
            })
            .expect("insert marketplace source");
        storage
    }

    #[test]
    fn offer_upsert_returns_previous_and_preserves_merchant_times() {
        let storage = storage();
        let mut first = offer("offer-1", 10.0, true);
        first.source_included_at = Some("2026-06-01T00:00:00+08:00".to_string());
        first.source_shop_created_at = Some("2025-01-01T00:00:00+08:00".to_string());
        assert!(storage
            .marketplace_offer_upsert(&first)
            .expect("insert offer")
            .is_none());

        let mut updated = offer("offer-1", 8.0, true);
        let previous = storage
            .marketplace_offer_upsert(&updated)
            .expect("update offer")
            .expect("previous offer");
        assert_eq!(previous.price, Some(10.0));
        updated.price = Some(7.0);

        let stored = storage
            .marketplace_offer_by_key("offer-1")
            .expect("read offer")
            .expect("offer exists");
        assert_eq!(stored.price, Some(8.0));
        assert_eq!(
            stored.source_included_at.as_deref(),
            Some("2026-06-01T00:00:00+08:00")
        );
        assert_eq!(
            stored.source_shop_created_at.as_deref(),
            Some("2025-01-01T00:00:00+08:00")
        );
    }

    #[test]
    fn partial_sync_does_not_invalidate_full_snapshot() {
        let storage = storage();
        storage
            .marketplace_offer_upsert(&offer("inside", 10.0, true))
            .expect("insert inside offer");
        storage
            .marketplace_offer_upsert(&offer("outside", 20.0, true))
            .expect("insert outside offer");
        storage
            .marketplace_source_sync_succeeded("source-1", 100)
            .expect("commit full snapshot");

        let mut inside = offer("inside", 9.0, true);
        inside.last_seen_sync_id = 100;
        storage
            .marketplace_offer_upsert(&inside)
            .expect("update partial offer");
        storage
            .marketplace_source_partial_sync_succeeded("source-1")
            .expect("record partial sync");

        let offers = storage
            .marketplace_offers(Some("source-1"), None, 10)
            .expect("list offers");
        assert_eq!(offers.len(), 2);
        assert!(offers.iter().all(|offer| offer.is_current));
    }

    #[test]
    fn favorite_merchant_can_be_saved_updated_and_removed() {
        let storage = storage();
        let mut favorite = MarketplaceFavoriteMerchantInput {
            merchant_key: "source:shopapi:merchant-1".to_string(),
            source_id: Some("merchant-1".to_string()),
            source_name: Some("First name".to_string()),
            collector_kind: Some("shopApi".to_string()),
        };
        storage
            .marketplace_favorite_merchant_set(&favorite, true)
            .expect("save favorite");
        let saved = storage
            .marketplace_favorite_merchants()
            .expect("list favorites");
        assert_eq!(saved.len(), 1);

        favorite.source_name = Some("Renamed merchant".to_string());
        storage
            .marketplace_favorite_merchant_set(&favorite, true)
            .expect("update favorite");
        let updated = storage
            .marketplace_favorite_merchants()
            .expect("list updated favorites");
        assert_eq!(updated[0].source_name.as_deref(), Some("Renamed merchant"));
        assert_eq!(updated[0].created_at, saved[0].created_at);

        storage
            .marketplace_favorite_merchant_set(&favorite, false)
            .expect("remove favorite");
        assert!(storage
            .marketplace_favorite_merchants()
            .expect("list empty favorites")
            .is_empty());
    }
}
