import type { WebCommandDescriptor } from "./shared";

export function createAggregateApiWebCommands(): Record<string, WebCommandDescriptor> {
  return {
    service_aggregate_api_list: { rpcMethod: "aggregateApi/list" },
    service_aggregate_api_create: { rpcMethod: "aggregateApi/create" },
    service_aggregate_api_update: { rpcMethod: "aggregateApi/update" },
    service_aggregate_api_recover: { rpcMethod: "aggregateApi/recover" },
    service_aggregate_api_update_sorts: { rpcMethod: "aggregateApi/updateSorts" },
    service_aggregate_api_delete: { rpcMethod: "aggregateApi/delete" },
    service_aggregate_api_read_secret: { rpcMethod: "aggregateApi/readSecret" },
    service_aggregate_api_test_connection: { rpcMethod: "aggregateApi/testConnection" },
    service_aggregate_api_discover_models: { rpcMethod: "aggregateApi/discoverModels" },
    service_aggregate_api_refresh_balance: { rpcMethod: "aggregateApi/refreshBalance" },
  };
}
