# API Gateway Module

A Soroban smart contract library providing the central request routing,
authentication, rate limiting, and resiliency logic for the Quest Service
ecosystem's API gateway.

## Features

### Routing
- **Route** / **RouteTable**: Registers `(path, method)` pairs mapped to a
  backend service, with an `requires_auth` flag per route
- **HttpMethod**: Get, Post, Put, Delete, Patch

### Authentication
- **AuthToken**: Bearer token modeled after decoded JWT/OAuth claims
  (subject, issued/expiry timestamps, scope)
- **AuthMiddleware**: Validates presence, expiry, and scope, returning an
  **AuthResult** (Authorized, Missing, Expired, InvalidScope)

### Rate Limiting
- **RateLimitState**: Fixed-window counter per client/route key with
  automatic window reset and remaining-quota tracking

### Request/Response Logging
- **RequestLogEntry**: Structured log record with an automatically derived
  **LogLevel** (Info/Warn/Error) based on status code

### CORS Handling
- **CorsPolicy**: Origin allow-list with wildcard (`*`) support and a
  credentials flag

### Health Checks
- **HealthCheckResult** / **HealthStatus**: Tracks per-service health
  (Healthy, Degraded, Unhealthy) and whether the service is operational

### Circuit Breaker
- **CircuitBreaker** / **CircuitState**: Closed → Open → HalfOpen state
  machine that trips after a configurable failure threshold and recovers
  after a reset timeout

### Request Transformation
- **TransformRule**: Path rewrite rules applied to an incoming request
- **RequestContext**: Carries a request's path/method and accumulated
  headers through the gateway pipeline

### Gateway Orchestration
- **process_request**: Runs a request through CORS validation, the circuit
  breaker, rate limiting, route resolution, and authentication (in that
  order) and returns a **GatewayResponse** with an outcome and status code

## Design Notes

Time-dependent operations (token expiry, rate-limit windows, circuit
breaker timers) take the current ledger timestamp as an explicit `now: u64`
parameter instead of reading `env.ledger()` internally. A contract
entrypoint reads `env.ledger().timestamp()` once and passes it down, which
keeps this module's logic testable without the `testutils` feature.

## Usage

### Registering routes

```rust
use api_gateway::{RouteTable, Route, HttpMethod, Env, String};

let env = Env::default();
let mut routes = RouteTable::new(&env);
routes.add_route(Route::new(
    String::from_str(&env, "/quests"),
    HttpMethod::Get,
    String::from_str(&env, "quest-service"),
    false,
));
```

### Processing a request

```rust
use api_gateway::{
    process_request, RouteTable, RateLimitState, CircuitBreaker, CorsPolicy, GatewayRequest,
};

let now = env.ledger().timestamp();
let response = process_request(now, &routes, &mut rate_limiter, &mut circuit, &cors, &request);

match response.outcome {
    GatewayOutcome::Routed => { /* forward to response.service */ }
    GatewayOutcome::Unauthorized => { /* return 401 */ }
    _ => { /* handle other outcomes via response.status_code */ }
}
```

### Circuit breaker

```rust
use api_gateway::CircuitBreaker;

let mut breaker = CircuitBreaker::new(5, 60); // trip after 5 failures, retry after 60s
if breaker.allow_request(now) {
    // call downstream service, then:
    breaker.record_success(); // or breaker.record_failure(now);
}
```

## Testing

The module includes 45 unit tests covering routing, authentication (valid,
missing, expired, and wrong-scope tokens), rate limiting (allow, reject,
window reset), logging, CORS (allow-list, wildcard, rejection), health
checks, the circuit breaker state machine (closed → open → half-open →
closed recovery), request transformation, and full gateway pipeline
orchestration for every `GatewayOutcome` branch (`Routed`, `NotFound`,
`Unauthorized`, `RateLimited`, `CircuitOpen`, `CorsRejected`).

Run tests with:
```bash
cargo test -p api-gateway --lib
```

## Building

```bash
# Build the library
cargo build -p api-gateway

# Build optimized release
cargo build -p api-gateway --release

# Run tests
cargo test -p api-gateway --lib

# Check compilation
cargo check -p api-gateway
```

## Dependencies

- **soroban-sdk** (21.0.0+): Stellar Soroban SDK for smart contracts

## License

MIT License - See project root for details
