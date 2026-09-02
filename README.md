# vedavid-connector

Answers PromQL queries from inside your cluster, over a connection it dials out.

Your Prometheus stays where it is. Nothing is scraped, copied or shipped
anywhere — a query arrives, the connector forwards it to the Prometheus you
point it at, and returns the result.

## Status

Early. What works today is the query path, served over a plain gRPC listener:

| RPC | |
| --- | --- |
| `InstantQuery` | `/api/v1/query` |
| `RangeQuery` | `/api/v1/query_range`, with the step derived from a point budget |
| `Labels`, `LabelValues`, `Series` | the matching `/api/v1` endpoints |

`BatchQuery`, `Events`, `InstallCertificate` and `Drain` return `Unimplemented`.
They belong with the outbound tunnel and mutual TLS, which are not built yet, so
this is not usable as a product — it is the query engine those will carry.

## Running it

```sh
VEDAVID_PROMETHEUS_URL=http://localhost:9090 \
VEDAVID_LISTEN=127.0.0.1:50051 \
cargo run
```

Both have defaults (`http://127.0.0.1:9090` and `127.0.0.1:50051`). `RUST_LOG`
controls logging.

There is no authentication on this listener. Until the tunnel exists, bind it to
loopback.

## How errors travel

A unary RPC maps a query failure to a gRPC code — `InvalidArgument` for bad
PromQL, `PermissionDenied`, `DeadlineExceeded`, `Unavailable` when Prometheus
cannot be reached, `Internal` otherwise — and passes the upstream message
through verbatim, because "parse error at 1:4" is the only thing that tells
someone what to fix.

Classification prefers Prometheus's own `errorType` over the HTTP status, since
a proxy in front of Prometheus may have rewritten the status.

## Tests

```sh
cargo test                                                   # unit tests only
VEDAVID_TEST_PROMETHEUS=http://127.0.0.1:9099 cargo test     # plus integration
```

The unit tests in `src/convert.rs` run against response bodies captured verbatim
from Prometheus 3.14 rather than hand-written JSON, because two details are easy
to get wrong from memory: matrix timestamps arrive as integers where vector and
scalar send floats, and there is no `warnings` key at all when there are none.
`NaN` and `+Inf` arrive as strings and would silently read as zero if dropped.

`tests/against_prometheus.rs` serves the real service on an ephemeral port and
queries a real Prometheus. Without `VEDAVID_TEST_PROMETHEUS` those tests skip
rather than fail, so `cargo test` stays useful on a machine without one. CI sets
it, so they always run there.

Building needs `protoc` on `PATH`, because the proto is compiled at build time.

## The wire contract

`proto/vedavid.proto` defines how the connector and the relay talk to each
other. This is where it lives, and it is compiled at build time, so changing it
changes the contract.
