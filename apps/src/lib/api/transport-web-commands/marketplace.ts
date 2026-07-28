import { asRecord, type WebCommandDescriptor } from "./shared";

const unwrapPayload = (params?: Record<string, unknown>): Record<string, unknown> =>
  asRecord(params?.payload) ?? {};

export function createMarketplaceWebCommands(): Record<string, WebCommandDescriptor> {
  return {
    service_marketplace_source_list: { rpcMethod: "marketplace/sourceList" },
    service_marketplace_source_upsert: { rpcMethod: "marketplace/sourceUpsert", mapParams: unwrapPayload },
    service_marketplace_source_delete: { rpcMethod: "marketplace/sourceDelete" },
    service_marketplace_offer_list: { rpcMethod: "marketplace/offerList", mapParams: unwrapPayload },
    service_marketplace_offer_verify: { rpcMethod: "marketplace/offerVerify" },
    service_marketplace_favorite_merchant_list: { rpcMethod: "marketplace/favoriteMerchantList" },
    service_marketplace_favorite_merchant_set: { rpcMethod: "marketplace/favoriteMerchantSet" },
    service_marketplace_refresh: { rpcMethod: "marketplace/refresh" },
    service_marketplace_change_list: { rpcMethod: "marketplace/changeList", mapParams: unwrapPayload },
    service_marketplace_alert_list: { rpcMethod: "marketplace/alertList" },
    service_marketplace_alert_upsert: { rpcMethod: "marketplace/alertUpsert", mapParams: unwrapPayload },
    service_marketplace_alert_delete: { rpcMethod: "marketplace/alertDelete" },
    service_marketplace_notification_get: { rpcMethod: "marketplace/notificationGet" },
    service_marketplace_notification_set: { rpcMethod: "marketplace/notificationSet" },
  };
}
