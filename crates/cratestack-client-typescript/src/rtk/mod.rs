//! `--rtk` (issue #906, epic #893's story #897): generates `src/rtk-api.ts`
//! — a typed RTK Query `createApi` endpoint set covering every model
//! operation and procedure in the schema, with `providesTags`/
//! `invalidatesTags` **derived from the schema** rather than hand-written.
//!
//! Precedent: `--tanstack` (#617, `crate::tanstack_collisions` +
//! `templates/src/{rest,rpc}-react-query.ts.j2`) is the closest sibling —
//! same additive-flag shape (`crate::templates::specs` appends one extra
//! spec per transport, `crate::generator` never treats it as replacing
//! anything), same "derives identifiers the schema author never wrote, so
//! refuse a collision rather than push it downstream" collision posture.
//! `--rtk` is not folded into that module because the endpoint-map naming
//! scheme, the dependency list, and the tag-derivation logic are each
//! substantial enough to want their own file under this repo's ~200-LoC
//! convention — hence this submodule rather than a same-shaped sibling of
//! `tanstack_collisions.rs`.
//!
//! # Why REST and RPC dispatch differently, and why that's still "no
//! second transport implementation"
//!
//! `@cratestack/adapter-rtk`'s `createRpcBaseQuery` is inherently RPC-only
//! — it dispatches an `{ opId, input }` pair through a generated RPC
//! client's `RpcCaller.call()`, which has no REST equivalent (REST has no
//! single opId-addressed call surface to adapt). So:
//!
//! - **RPC** (`templates/src/rtk-rpc.ts.j2`): `baseQuery:
//!   createRpcBaseQuery(client.runtime)`, and every endpoint's `queryFn`
//!   calls the RESOLVED base query (RTK Query's own 4th `queryFn` argument,
//!   `(arg, api, extraOptions, baseQuery) => ...` — the officially
//!   supported way to invoke it manually) rather than a plain `query:` —
//!   because the adapter's raw `RpcCaller.call()` returns the WIRE shape
//!   (no `Decimal`/`Bytes` revival), while the generated client's own
//!   per-model methods apply `reviveWireFields`/`revivePagedWireFields`
//!   after that same call. Skipping the revival step would type-check
//!   (`Decimal`/`Uint8Array` are structurally opaque to a raw wire value)
//!   while shipping the wrong data at runtime — so `queryFn` calls the
//!   SAME base query the adapter exports and then applies the SAME
//!   revival step the generated client's class methods already apply,
//!   rather than reimplementing either.
//! - **REST** (`templates/src/rtk-rest.ts.j2`): no adapter package exists
//!   to dispatch through, so `baseQuery: fakeBaseQuery<...>()` (RTK
//!   Query's documented "I only use `queryFn`" placeholder) and every
//!   endpoint's `queryFn` calls this SAME generated package's own REST
//!   client methods (`client.{{ accessor }}.list()`/`.get()`/etc. —
//!   `templates/src/rest-client.ts.j2`), which already revive wire fields
//!   internally. Still "no second transport implementation": the HTTP
//!   call is made by the existing generated client, not reimplemented
//!   here.
//!
//! # Tag derivation
//!
//! `providesTags`/`invalidatesTags` for the five model CRUD endpoints are
//! a fixed, well-known RTK Query convention (list provides `{type,
//! id:'LIST'}` + one per-item tag; get provides one per-id tag; create
//! invalidates the list tag; update/delete invalidate both the specific-id
//! and list tags). The genuinely schema-derived part — and this ticket's
//! actual point — is `submodule::touch`'s per-procedure `touched_model_names`:
//! a `procedure`/`mutation procedure`'s OWN `args`/`return_type` are the
//! only schema-declared signal linking it to the models it operates on
//! (there is no separate `touches` attribute), so that is exactly what
//! gets walked, and the resulting model set becomes that procedure's
//! `providesTags` (query) / `invalidatesTags` (mutation) — one `{type,
//! id:'LIST'}` per touched model.

pub(crate) mod collisions;
pub(crate) mod deps;
pub(crate) mod naming;
pub(crate) mod specs;
pub(crate) mod touch;
