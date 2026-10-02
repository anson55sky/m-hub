import { onRequest as __v1_chat_completions_ts_onRequest } from "/Users/sky/Desktop/项目/CODE/m-hub/server/functions/v1/chat/completions.ts"
import { onRequest as __api_v1___path___ts_onRequest } from "/Users/sky/Desktop/项目/CODE/m-hub/server/functions/api/v1/[[path]].ts"
import { onRequest as __me_ts_onRequest } from "/Users/sky/Desktop/项目/CODE/m-hub/server/functions/me.ts"

export const routes = [
    {
      routePath: "/v1/chat/completions",
      mountPath: "/v1/chat",
      method: "",
      middlewares: [],
      modules: [__v1_chat_completions_ts_onRequest],
    },
  {
      routePath: "/api/v1/:path*",
      mountPath: "/api/v1",
      method: "",
      middlewares: [],
      modules: [__api_v1___path___ts_onRequest],
    },
  {
      routePath: "/me",
      mountPath: "/",
      method: "",
      middlewares: [],
      modules: [__me_ts_onRequest],
    },
  ]