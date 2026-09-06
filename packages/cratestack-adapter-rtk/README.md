# @cratestack/adapter-rtk

An [RTK Query](https://redux-toolkit.js.org/rtk-query/overview) `BaseQueryFn` adapter over
CrateStack's generated TypeScript RPC client (`transport rpc` schemas) — dispatches every endpoint
through the same runtime and `RpcLink` chain (`@cratestack/link-batch`, `@cratestack/link-logger`,
etc.) the rest of the generated client uses, instead of RTK Query's default `fetchBaseQuery`
reimplementing the wire protocol.

This package is the primitive; `cratestack generate-typescript --rtk` (cratestack#906) generates a
full `createApi` endpoint set on top of it — `src/rtk-api.ts`'s `createCratestackRtkApi(client)`, one
entry per model operation and per `procedure`, with `providesTags`/`invalidatesTags` derived from the
schema rather than hand-written. The hand-written shape below is still exactly what that generated
code does under the hood, and is the reference for anyone customizing beyond the generator's output
or writing a `--template-dir` override for `rtk-rpc.ts.j2`.

## Usage

```ts
import { createRpcBaseQuery } from "@cratestack/adapter-rtk";
import { createApi } from "@reduxjs/toolkit/query/react";
import { client } from "./generated/client"; // your project's generated client instance

export const api = createApi({
  reducerPath: "api",
  baseQuery: createRpcBaseQuery(client.runtime),
  endpoints: (builder) => ({
    getWidget: builder.query<Widget, number>({
      query: (id) => ({ opId: "model.Widget.get", input: { id } }),
    }),
    createOrder: builder.mutation<Order, CreateOrderInput>({
      query: (input) => ({ opId: "model.Order.create", input }),
    }),
  }),
});
```

`createRpcBaseQuery` takes an `RpcCaller` — any object with a `call<I, O>(opId, input, options?)`
method, which is exactly the shape of a generated client's public `.runtime` field
(`CratestackRpcRuntime`). A failed call surfaces as `{ status: "RPC_ERROR", data: RpcErrorBody }`
when the server returned a structured error, or `{ status: "RPC_TRANSPORT_ERROR" }` for a network
failure with no such body.
