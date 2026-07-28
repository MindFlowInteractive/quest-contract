#![no_std]

//! API Gateway Module
//!
//! Central request router for the Quest Service ecosystem, providing:
//! - Route resolution to backend microservices
//! - JWT/OAuth-style token authentication
//! - Rate limiting (fixed-window)
//! - Request/response logging
//! - CORS origin validation
//! - Health check tracking
//! - Circuit breaker pattern for failing services
//! - Request transformation (path rewriting, header injection)
//!
//! Time-dependent operations take the current ledger timestamp as an
//! explicit `now: u64` parameter rather than reading `env.ledger()`
//! internally. Callers (contract entrypoints) read the timestamp once
//! from `env.ledger().timestamp()` and pass it down, which keeps this
//! module's logic independently testable.

use soroban_sdk::{contracttype, Env, String, Vec};

//
// ──────────────────────────────────────────────────────────
// ROUTING
// ──────────────────────────────────────────────────────────
//

/// HTTP-like methods supported by the router
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttpMethod {
    Get = 0,
    Post = 1,
    Put = 2,
    Delete = 3,
    Patch = 4,
}

/// A single route mapping a path + method to a backend service
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Route {
    pub path: String,
    pub method: HttpMethod,
    pub service: String,
    pub requires_auth: bool,
}

impl Route {
    pub fn new(path: String, method: HttpMethod, service: String, requires_auth: bool) -> Self {
        Self {
            path,
            method,
            service,
            requires_auth,
        }
    }

    pub fn matches(&self, path: &String, method: HttpMethod) -> bool {
        self.method == method && &self.path == path
    }
}

/// Routing table mapping registered routes to backend services
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteTable {
    pub routes: Vec<Route>,
}

impl RouteTable {
    pub fn new(env: &Env) -> Self {
        Self {
            routes: Vec::new(env),
        }
    }

    pub fn add_route(&mut self, route: Route) {
        self.routes.push_back(route);
    }

    pub fn resolve(&self, path: &String, method: HttpMethod) -> Option<Route> {
        self.routes.iter().find(|route| route.matches(path, method))
    }

    pub fn len(&self) -> u32 {
        self.routes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }
}

//
// ──────────────────────────────────────────────────────────
// AUTHENTICATION
// ──────────────────────────────────────────────────────────
//

/// A bearer token carrying subject, validity window, and scope,
/// analogous to the claims of a decoded JWT/OAuth access token.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthToken {
    pub subject: String,
    pub issued_at: u64,
    pub expires_at: u64,
    pub scope: String,
}

impl AuthToken {
    pub fn new(subject: String, issued_at: u64, ttl_seconds: u64, scope: String) -> Self {
        Self {
            subject,
            issued_at,
            expires_at: issued_at + ttl_seconds,
            scope,
        }
    }

    pub fn is_expired(&self, now: u64) -> bool {
        now >= self.expires_at
    }

    pub fn has_scope(&self, required: &String) -> bool {
        &self.scope == required
    }
}

/// Outcome of an authentication check
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthResult {
    Authorized = 0,
    Missing = 1,
    Expired = 2,
    InvalidScope = 3,
}

pub struct AuthMiddleware;

impl AuthMiddleware {
    pub fn authenticate(
        now: u64,
        token: Option<&AuthToken>,
        required_scope: &String,
    ) -> AuthResult {
        match token {
            None => AuthResult::Missing,
            Some(t) if t.is_expired(now) => AuthResult::Expired,
            Some(t) if !t.has_scope(required_scope) => AuthResult::InvalidScope,
            Some(_) => AuthResult::Authorized,
        }
    }
}

//
// ──────────────────────────────────────────────────────────
// RATE LIMITING
// ──────────────────────────────────────────────────────────
//

/// Fixed-window rate limiter state for a single client/route key
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateLimitState {
    pub requests_in_window: u32,
    pub window_start: u64,
    pub limit: u32,
    pub window_seconds: u64,
}

impl RateLimitState {
    pub fn new(limit: u32, window_seconds: u64, now: u64) -> Self {
        Self {
            requests_in_window: 0,
            window_start: now,
            limit,
            window_seconds,
        }
    }

    /// Records a request attempt, resetting the window if it has elapsed.
    /// Returns true if the request is allowed, false if it must be rejected.
    pub fn check_and_record(&mut self, now: u64) -> bool {
        if now.saturating_sub(self.window_start) >= self.window_seconds {
            self.window_start = now;
            self.requests_in_window = 0;
        }

        if self.requests_in_window >= self.limit {
            return false;
        }

        self.requests_in_window += 1;
        true
    }

    pub fn remaining(&self) -> u32 {
        self.limit.saturating_sub(self.requests_in_window)
    }
}

//
// ──────────────────────────────────────────────────────────
// LOGGING
// ──────────────────────────────────────────────────────────
//

/// Severity level assigned to a logged request based on its outcome
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogLevel {
    Info = 0,
    Warn = 1,
    Error = 2,
}

/// A structured request/response log entry
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestLogEntry {
    pub request_id: String,
    pub method: HttpMethod,
    pub path: String,
    pub status_code: u32,
    pub duration_ms: u32,
    pub level: LogLevel,
    pub timestamp: u64,
}

impl RequestLogEntry {
    pub fn new(
        timestamp: u64,
        request_id: String,
        method: HttpMethod,
        path: String,
        status_code: u32,
        duration_ms: u32,
    ) -> Self {
        let level = if status_code >= 500 {
            LogLevel::Error
        } else if status_code >= 400 {
            LogLevel::Warn
        } else {
            LogLevel::Info
        };

        Self {
            request_id,
            method,
            path,
            status_code,
            duration_ms,
            level,
            timestamp,
        }
    }
}

//
// ──────────────────────────────────────────────────────────
// CORS
// ──────────────────────────────────────────────────────────
//

/// CORS origin allow-list policy
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorsPolicy {
    pub allowed_origins: Vec<String>,
    pub allow_any: bool,
    pub allow_credentials: bool,
}

impl CorsPolicy {
    pub fn new(env: &Env, allow_credentials: bool) -> Self {
        Self {
            allowed_origins: Vec::new(env),
            allow_any: false,
            allow_credentials,
        }
    }

    pub fn allow_origin(&mut self, env: &Env, origin: String) {
        if origin == String::from_str(env, "*") {
            self.allow_any = true;
        } else {
            self.allowed_origins.push_back(origin);
        }
    }

    pub fn is_allowed(&self, origin: &String) -> bool {
        if self.allow_any {
            return true;
        }

        for allowed in self.allowed_origins.iter() {
            if &allowed == origin {
                return true;
            }
        }

        false
    }
}

//
// ──────────────────────────────────────────────────────────
// HEALTH CHECKS
// ──────────────────────────────────────────────────────────
//

/// Health status of a backend service
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HealthStatus {
    Healthy = 0,
    Degraded = 1,
    Unhealthy = 2,
}

/// Result of a health probe against a backend service
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HealthCheckResult {
    pub service: String,
    pub status: HealthStatus,
    pub checked_at: u64,
    pub latency_ms: u32,
}

impl HealthCheckResult {
    pub fn new(checked_at: u64, service: String, status: HealthStatus, latency_ms: u32) -> Self {
        Self {
            service,
            status,
            checked_at,
            latency_ms,
        }
    }

    pub fn is_operational(&self) -> bool {
        matches!(self.status, HealthStatus::Healthy | HealthStatus::Degraded)
    }
}

//
// ──────────────────────────────────────────────────────────
// CIRCUIT BREAKER
// ──────────────────────────────────────────────────────────
//

/// Circuit breaker state
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CircuitState {
    Closed = 0,
    Open = 1,
    HalfOpen = 2,
}

/// Circuit breaker guarding calls to a downstream service
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CircuitBreaker {
    pub state: CircuitState,
    pub failure_count: u32,
    pub failure_threshold: u32,
    pub last_failure_at: u64,
    pub reset_timeout_seconds: u64,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, reset_timeout_seconds: u64) -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            failure_threshold,
            last_failure_at: 0,
            reset_timeout_seconds,
        }
    }

    pub fn record_success(&mut self) {
        self.failure_count = 0;
        self.state = CircuitState::Closed;
    }

    pub fn record_failure(&mut self, now: u64) {
        self.failure_count += 1;
        self.last_failure_at = now;

        if self.failure_count >= self.failure_threshold {
            self.state = CircuitState::Open;
        }
    }

    /// Returns true if a request may proceed, transitioning Open -> HalfOpen
    /// once the reset timeout has elapsed.
    pub fn allow_request(&mut self, now: u64) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true,
            CircuitState::Open => {
                let elapsed = now.saturating_sub(self.last_failure_at);
                if elapsed >= self.reset_timeout_seconds {
                    self.state = CircuitState::HalfOpen;
                    true
                } else {
                    false
                }
            }
        }
    }
}

//
// ──────────────────────────────────────────────────────────
// REQUEST TRANSFORMATION
// ──────────────────────────────────────────────────────────
//

/// A single header key/value pair
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeaderEntry {
    pub key: String,
    pub value: String,
}

/// A rewrite rule mapping an incoming path to a backend-facing path
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransformRule {
    pub match_path: String,
    pub rewrite_path: String,
}

impl TransformRule {
    pub fn new(match_path: String, rewrite_path: String) -> Self {
        Self {
            match_path,
            rewrite_path,
        }
    }

    pub fn apply(&self, path: &String) -> Option<String> {
        if path == &self.match_path {
            Some(self.rewrite_path.clone())
        } else {
            None
        }
    }
}

/// Mutable request context carried through the gateway pipeline,
/// accumulating injected headers and path rewrites.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestContext {
    pub path: String,
    pub method: HttpMethod,
    pub headers: Vec<HeaderEntry>,
}

impl RequestContext {
    pub fn new(env: &Env, path: String, method: HttpMethod) -> Self {
        Self {
            path,
            method,
            headers: Vec::new(env),
        }
    }

    pub fn add_header(&mut self, key: String, value: String) {
        self.headers.push_back(HeaderEntry { key, value });
    }

    pub fn get_header(&self, key: &String) -> Option<String> {
        for header in self.headers.iter() {
            if &header.key == key {
                return Some(header.value.clone());
            }
        }
        None
    }

    pub fn apply_rewrite(&mut self, rule: &TransformRule) -> bool {
        if let Some(rewritten) = rule.apply(&self.path) {
            self.path = rewritten;
            true
        } else {
            false
        }
    }
}

//
// ──────────────────────────────────────────────────────────
// GATEWAY ORCHESTRATION
// ──────────────────────────────────────────────────────────
//

/// Result classification of a request processed by the gateway
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GatewayOutcome {
    Routed = 0,
    NotFound = 1,
    Unauthorized = 2,
    RateLimited = 3,
    CircuitOpen = 4,
    CorsRejected = 5,
}

/// Incoming request as seen by the gateway
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GatewayRequest {
    pub path: String,
    pub method: HttpMethod,
    pub origin: Option<String>,
    pub auth_token: Option<AuthToken>,
    pub required_scope: String,
}

/// Outcome of routing a request through the gateway pipeline
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GatewayResponse {
    pub outcome: GatewayOutcome,
    pub status_code: u32,
    pub service: Option<String>,
}

impl GatewayResponse {
    fn new(outcome: GatewayOutcome, status_code: u32, service: Option<String>) -> Self {
        Self {
            outcome,
            status_code,
            service,
        }
    }
}

/// Runs a request through CORS validation, the circuit breaker, rate
/// limiting, route resolution, and authentication, in that order —
/// mirroring the order a real gateway would reject cheapest-first.
pub fn process_request(
    now: u64,
    routes: &RouteTable,
    rate_limiter: &mut RateLimitState,
    circuit: &mut CircuitBreaker,
    cors: &CorsPolicy,
    request: &GatewayRequest,
) -> GatewayResponse {
    if let Some(origin) = &request.origin {
        if !cors.is_allowed(origin) {
            return GatewayResponse::new(GatewayOutcome::CorsRejected, 403, None);
        }
    }

    if !circuit.allow_request(now) {
        return GatewayResponse::new(GatewayOutcome::CircuitOpen, 503, None);
    }

    if !rate_limiter.check_and_record(now) {
        return GatewayResponse::new(GatewayOutcome::RateLimited, 429, None);
    }

    let route = match routes.resolve(&request.path, request.method) {
        Some(route) => route,
        None => return GatewayResponse::new(GatewayOutcome::NotFound, 404, None),
    };

    if route.requires_auth {
        let auth_result =
            AuthMiddleware::authenticate(now, request.auth_token.as_ref(), &request.required_scope);
        if auth_result != AuthResult::Authorized {
            return GatewayResponse::new(GatewayOutcome::Unauthorized, 401, None);
        }
    }

    GatewayResponse::new(GatewayOutcome::Routed, 200, Some(route.service.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Env {
        Env::default()
    }

    // ── Routing ──────────────────────────────────────────

    #[test]
    fn test_route_matches() {
        let e = env();
        let route = Route::new(
            String::from_str(&e, "/quests"),
            HttpMethod::Get,
            String::from_str(&e, "quest-service"),
            false,
        );
        assert!(route.matches(&String::from_str(&e, "/quests"), HttpMethod::Get));
        assert!(!route.matches(&String::from_str(&e, "/quests"), HttpMethod::Post));
        assert!(!route.matches(&String::from_str(&e, "/other"), HttpMethod::Get));
    }

    #[test]
    fn test_route_table_resolve_found() {
        let e = env();
        let mut table = RouteTable::new(&e);
        table.add_route(Route::new(
            String::from_str(&e, "/quests"),
            HttpMethod::Get,
            String::from_str(&e, "quest-service"),
            false,
        ));

        let resolved = table.resolve(&String::from_str(&e, "/quests"), HttpMethod::Get);
        assert!(resolved.is_some());
        assert_eq!(
            resolved.unwrap().service,
            String::from_str(&e, "quest-service")
        );
    }

    #[test]
    fn test_route_table_resolve_not_found() {
        let e = env();
        let table = RouteTable::new(&e);
        let resolved = table.resolve(&String::from_str(&e, "/missing"), HttpMethod::Get);
        assert!(resolved.is_none());
    }

    #[test]
    fn test_route_table_is_empty() {
        let e = env();
        let mut table = RouteTable::new(&e);
        assert!(table.is_empty());
        table.add_route(Route::new(
            String::from_str(&e, "/a"),
            HttpMethod::Get,
            String::from_str(&e, "svc"),
            false,
        ));
        assert!(!table.is_empty());
    }

    #[test]
    fn test_route_table_len() {
        let e = env();
        let mut table = RouteTable::new(&e);
        assert_eq!(table.len(), 0);
        table.add_route(Route::new(
            String::from_str(&e, "/a"),
            HttpMethod::Get,
            String::from_str(&e, "svc"),
            false,
        ));
        assert_eq!(table.len(), 1);
    }

    // ── Authentication ───────────────────────────────────

    #[test]
    fn test_auth_token_not_expired() {
        let e = env();
        let token = AuthToken::new(
            String::from_str(&e, "user-1"),
            0,
            3600,
            String::from_str(&e, "read"),
        );
        assert!(!token.is_expired(0));
    }

    #[test]
    fn test_auth_token_expired() {
        let e = env();
        let token = AuthToken::new(
            String::from_str(&e, "user-1"),
            0,
            3600,
            String::from_str(&e, "read"),
        );
        assert!(token.is_expired(3600));
    }

    #[test]
    fn test_auth_token_scope() {
        let e = env();
        let token = AuthToken::new(
            String::from_str(&e, "user-1"),
            0,
            3600,
            String::from_str(&e, "read"),
        );
        assert!(token.has_scope(&String::from_str(&e, "read")));
        assert!(!token.has_scope(&String::from_str(&e, "write")));
    }

    #[test]
    fn test_auth_middleware_missing_token() {
        let e = env();
        let result = AuthMiddleware::authenticate(0, None, &String::from_str(&e, "read"));
        assert_eq!(result, AuthResult::Missing);
    }

    #[test]
    fn test_auth_middleware_authorized() {
        let e = env();
        let token = AuthToken::new(
            String::from_str(&e, "user-1"),
            0,
            3600,
            String::from_str(&e, "read"),
        );
        let result = AuthMiddleware::authenticate(0, Some(&token), &String::from_str(&e, "read"));
        assert_eq!(result, AuthResult::Authorized);
    }

    #[test]
    fn test_auth_middleware_expired() {
        let e = env();
        let token = AuthToken::new(
            String::from_str(&e, "user-1"),
            0,
            3600,
            String::from_str(&e, "read"),
        );
        let result =
            AuthMiddleware::authenticate(3600, Some(&token), &String::from_str(&e, "read"));
        assert_eq!(result, AuthResult::Expired);
    }

    #[test]
    fn test_auth_middleware_invalid_scope() {
        let e = env();
        let token = AuthToken::new(
            String::from_str(&e, "user-1"),
            0,
            3600,
            String::from_str(&e, "read"),
        );
        let result = AuthMiddleware::authenticate(0, Some(&token), &String::from_str(&e, "write"));
        assert_eq!(result, AuthResult::InvalidScope);
    }

    // ── Rate limiting ─────────────────────────────────────

    #[test]
    fn test_rate_limit_allows_under_limit() {
        let mut state = RateLimitState::new(2, 60, 0);
        assert!(state.check_and_record(0));
        assert!(state.check_and_record(0));
        assert_eq!(state.remaining(), 0);
    }

    #[test]
    fn test_rate_limit_rejects_over_limit() {
        let mut state = RateLimitState::new(1, 60, 0);
        assert!(state.check_and_record(0));
        assert!(!state.check_and_record(0));
    }

    #[test]
    fn test_rate_limit_resets_after_window() {
        let mut state = RateLimitState::new(1, 60, 0);
        assert!(state.check_and_record(0));
        assert!(!state.check_and_record(30));
        assert!(state.check_and_record(60));
    }

    // ── Logging ───────────────────────────────────────────

    #[test]
    fn test_log_entry_level_info() {
        let e = env();
        let entry = RequestLogEntry::new(
            0,
            String::from_str(&e, "req-1"),
            HttpMethod::Get,
            String::from_str(&e, "/quests"),
            200,
            10,
        );
        assert_eq!(entry.level, LogLevel::Info);
    }

    #[test]
    fn test_log_entry_level_warn() {
        let e = env();
        let entry = RequestLogEntry::new(
            0,
            String::from_str(&e, "req-2"),
            HttpMethod::Get,
            String::from_str(&e, "/quests"),
            404,
            5,
        );
        assert_eq!(entry.level, LogLevel::Warn);
    }

    #[test]
    fn test_log_entry_level_error() {
        let e = env();
        let entry = RequestLogEntry::new(
            0,
            String::from_str(&e, "req-3"),
            HttpMethod::Get,
            String::from_str(&e, "/quests"),
            500,
            5,
        );
        assert_eq!(entry.level, LogLevel::Error);
    }

    // ── CORS ──────────────────────────────────────────────

    #[test]
    fn test_cors_allows_registered_origin() {
        let e = env();
        let mut cors = CorsPolicy::new(&e, false);
        cors.allow_origin(&e, String::from_str(&e, "https://app.example.com"));
        assert!(cors.is_allowed(&String::from_str(&e, "https://app.example.com")));
        assert!(!cors.is_allowed(&String::from_str(&e, "https://evil.example.com")));
    }

    #[test]
    fn test_cors_wildcard_allows_any() {
        let e = env();
        let mut cors = CorsPolicy::new(&e, false);
        cors.allow_origin(&e, String::from_str(&e, "*"));
        assert!(cors.is_allowed(&String::from_str(&e, "https://anything.example.com")));
    }

    #[test]
    fn test_cors_rejects_unlisted_origin() {
        let e = env();
        let cors = CorsPolicy::new(&e, false);
        assert!(!cors.is_allowed(&String::from_str(&e, "https://app.example.com")));
    }

    // ── Health checks ─────────────────────────────────────

    #[test]
    fn test_health_check_healthy_is_operational() {
        let e = env();
        let result = HealthCheckResult::new(
            0,
            String::from_str(&e, "quest-service"),
            HealthStatus::Healthy,
            20,
        );
        assert!(result.is_operational());
    }

    #[test]
    fn test_health_check_degraded_is_operational() {
        let e = env();
        let result = HealthCheckResult::new(
            0,
            String::from_str(&e, "quest-service"),
            HealthStatus::Degraded,
            200,
        );
        assert!(result.is_operational());
    }

    #[test]
    fn test_health_check_unhealthy_not_operational() {
        let e = env();
        let result = HealthCheckResult::new(
            0,
            String::from_str(&e, "quest-service"),
            HealthStatus::Unhealthy,
            5000,
        );
        assert!(!result.is_operational());
    }

    // ── Circuit breaker ───────────────────────────────────

    #[test]
    fn test_circuit_breaker_starts_closed() {
        let mut breaker = CircuitBreaker::new(3, 60);
        assert_eq!(breaker.state, CircuitState::Closed);
        assert!(breaker.allow_request(0));
    }

    #[test]
    fn test_circuit_breaker_opens_after_threshold() {
        let mut breaker = CircuitBreaker::new(2, 60);
        breaker.record_failure(0);
        assert_eq!(breaker.state, CircuitState::Closed);
        breaker.record_failure(0);
        assert_eq!(breaker.state, CircuitState::Open);
        assert!(!breaker.allow_request(0));
    }

    #[test]
    fn test_circuit_breaker_half_opens_after_timeout() {
        let mut breaker = CircuitBreaker::new(1, 60);
        breaker.record_failure(0);
        assert_eq!(breaker.state, CircuitState::Open);
        assert!(!breaker.allow_request(30));

        assert!(breaker.allow_request(60));
        assert_eq!(breaker.state, CircuitState::HalfOpen);
    }

    #[test]
    fn test_circuit_breaker_success_resets() {
        let mut breaker = CircuitBreaker::new(1, 60);
        breaker.record_failure(0);
        assert_eq!(breaker.state, CircuitState::Open);

        breaker.record_success();
        assert_eq!(breaker.state, CircuitState::Closed);
        assert_eq!(breaker.failure_count, 0);
    }

    // ── Request transformation ────────────────────────────

    #[test]
    fn test_transform_rule_applies_on_match() {
        let e = env();
        let rule = TransformRule::new(
            String::from_str(&e, "/api/quests"),
            String::from_str(&e, "/internal/v2/quests"),
        );
        let result = rule.apply(&String::from_str(&e, "/api/quests"));
        assert_eq!(result, Some(String::from_str(&e, "/internal/v2/quests")));
    }

    #[test]
    fn test_transform_rule_no_match() {
        let e = env();
        let rule = TransformRule::new(
            String::from_str(&e, "/api/quests"),
            String::from_str(&e, "/internal/v2/quests"),
        );
        assert_eq!(rule.apply(&String::from_str(&e, "/api/other")), None);
    }

    #[test]
    fn test_request_context_headers() {
        let e = env();
        let mut ctx = RequestContext::new(&e, String::from_str(&e, "/quests"), HttpMethod::Get);
        ctx.add_header(
            String::from_str(&e, "X-Request-Id"),
            String::from_str(&e, "req-42"),
        );
        assert_eq!(
            ctx.get_header(&String::from_str(&e, "X-Request-Id")),
            Some(String::from_str(&e, "req-42"))
        );
        assert_eq!(ctx.get_header(&String::from_str(&e, "Missing")), None);
    }

    #[test]
    fn test_request_context_rewrite() {
        let e = env();
        let mut ctx = RequestContext::new(&e, String::from_str(&e, "/api/quests"), HttpMethod::Get);
        let rule = TransformRule::new(
            String::from_str(&e, "/api/quests"),
            String::from_str(&e, "/internal/v2/quests"),
        );
        assert!(ctx.apply_rewrite(&rule));
        assert_eq!(ctx.path, String::from_str(&e, "/internal/v2/quests"));
    }

    #[test]
    fn test_request_context_rewrite_no_match_leaves_path() {
        let e = env();
        let mut ctx = RequestContext::new(&e, String::from_str(&e, "/api/quests"), HttpMethod::Get);
        let rule = TransformRule::new(
            String::from_str(&e, "/api/other"),
            String::from_str(&e, "/internal/v2/other"),
        );
        assert!(!ctx.apply_rewrite(&rule));
        assert_eq!(ctx.path, String::from_str(&e, "/api/quests"));
    }

    // ── Gateway orchestration ─────────────────────────────

    fn sample_routes(e: &Env) -> RouteTable {
        let mut table = RouteTable::new(e);
        table.add_route(Route::new(
            String::from_str(e, "/public"),
            HttpMethod::Get,
            String::from_str(e, "public-service"),
            false,
        ));
        table.add_route(Route::new(
            String::from_str(e, "/private"),
            HttpMethod::Get,
            String::from_str(e, "private-service"),
            true,
        ));
        table
    }

    #[test]
    fn test_process_request_routed_public() {
        let e = env();
        let routes = sample_routes(&e);
        let mut rate_limiter = RateLimitState::new(10, 60, 0);
        let mut circuit = CircuitBreaker::new(3, 60);
        let cors = CorsPolicy::new(&e, false);

        let request = GatewayRequest {
            path: String::from_str(&e, "/public"),
            method: HttpMethod::Get,
            origin: None,
            auth_token: None,
            required_scope: String::from_str(&e, "read"),
        };

        let response =
            process_request(0, &routes, &mut rate_limiter, &mut circuit, &cors, &request);
        assert_eq!(response.outcome, GatewayOutcome::Routed);
        assert_eq!(response.status_code, 200);
        assert_eq!(
            response.service,
            Some(String::from_str(&e, "public-service"))
        );
    }

    #[test]
    fn test_process_request_not_found() {
        let e = env();
        let routes = sample_routes(&e);
        let mut rate_limiter = RateLimitState::new(10, 60, 0);
        let mut circuit = CircuitBreaker::new(3, 60);
        let cors = CorsPolicy::new(&e, false);

        let request = GatewayRequest {
            path: String::from_str(&e, "/missing"),
            method: HttpMethod::Get,
            origin: None,
            auth_token: None,
            required_scope: String::from_str(&e, "read"),
        };

        let response =
            process_request(0, &routes, &mut rate_limiter, &mut circuit, &cors, &request);
        assert_eq!(response.outcome, GatewayOutcome::NotFound);
        assert_eq!(response.status_code, 404);
    }

    #[test]
    fn test_process_request_unauthorized_without_token() {
        let e = env();
        let routes = sample_routes(&e);
        let mut rate_limiter = RateLimitState::new(10, 60, 0);
        let mut circuit = CircuitBreaker::new(3, 60);
        let cors = CorsPolicy::new(&e, false);

        let request = GatewayRequest {
            path: String::from_str(&e, "/private"),
            method: HttpMethod::Get,
            origin: None,
            auth_token: None,
            required_scope: String::from_str(&e, "read"),
        };

        let response =
            process_request(0, &routes, &mut rate_limiter, &mut circuit, &cors, &request);
        assert_eq!(response.outcome, GatewayOutcome::Unauthorized);
        assert_eq!(response.status_code, 401);
    }

    #[test]
    fn test_process_request_authorized_with_token() {
        let e = env();
        let routes = sample_routes(&e);
        let mut rate_limiter = RateLimitState::new(10, 60, 0);
        let mut circuit = CircuitBreaker::new(3, 60);
        let cors = CorsPolicy::new(&e, false);
        let token = AuthToken::new(
            String::from_str(&e, "user-1"),
            0,
            3600,
            String::from_str(&e, "read"),
        );

        let request = GatewayRequest {
            path: String::from_str(&e, "/private"),
            method: HttpMethod::Get,
            origin: None,
            auth_token: Some(token),
            required_scope: String::from_str(&e, "read"),
        };

        let response =
            process_request(0, &routes, &mut rate_limiter, &mut circuit, &cors, &request);
        assert_eq!(response.outcome, GatewayOutcome::Routed);
        assert_eq!(
            response.service,
            Some(String::from_str(&e, "private-service"))
        );
    }

    #[test]
    fn test_process_request_rate_limited() {
        let e = env();
        let routes = sample_routes(&e);
        let mut rate_limiter = RateLimitState::new(1, 60, 0);
        let mut circuit = CircuitBreaker::new(3, 60);
        let cors = CorsPolicy::new(&e, false);

        let request = GatewayRequest {
            path: String::from_str(&e, "/public"),
            method: HttpMethod::Get,
            origin: None,
            auth_token: None,
            required_scope: String::from_str(&e, "read"),
        };

        let first = process_request(0, &routes, &mut rate_limiter, &mut circuit, &cors, &request);
        assert_eq!(first.outcome, GatewayOutcome::Routed);

        let second = process_request(0, &routes, &mut rate_limiter, &mut circuit, &cors, &request);
        assert_eq!(second.outcome, GatewayOutcome::RateLimited);
        assert_eq!(second.status_code, 429);
    }

    #[test]
    fn test_process_request_circuit_open() {
        let e = env();
        let routes = sample_routes(&e);
        let mut rate_limiter = RateLimitState::new(10, 60, 0);
        let mut circuit = CircuitBreaker::new(1, 60);
        circuit.record_failure(0);
        let cors = CorsPolicy::new(&e, false);

        let request = GatewayRequest {
            path: String::from_str(&e, "/public"),
            method: HttpMethod::Get,
            origin: None,
            auth_token: None,
            required_scope: String::from_str(&e, "read"),
        };

        let response =
            process_request(0, &routes, &mut rate_limiter, &mut circuit, &cors, &request);
        assert_eq!(response.outcome, GatewayOutcome::CircuitOpen);
        assert_eq!(response.status_code, 503);
    }

    #[test]
    fn test_process_request_cors_rejected() {
        let e = env();
        let routes = sample_routes(&e);
        let mut rate_limiter = RateLimitState::new(10, 60, 0);
        let mut circuit = CircuitBreaker::new(3, 60);
        let cors = CorsPolicy::new(&e, false);

        let request = GatewayRequest {
            path: String::from_str(&e, "/public"),
            method: HttpMethod::Get,
            origin: Some(String::from_str(&e, "https://evil.example.com")),
            auth_token: None,
            required_scope: String::from_str(&e, "read"),
        };

        let response =
            process_request(0, &routes, &mut rate_limiter, &mut circuit, &cors, &request);
        assert_eq!(response.outcome, GatewayOutcome::CorsRejected);
        assert_eq!(response.status_code, 403);
    }

    #[test]
    fn test_process_request_cors_allowed_origin_routes() {
        let e = env();
        let routes = sample_routes(&e);
        let mut rate_limiter = RateLimitState::new(10, 60, 0);
        let mut circuit = CircuitBreaker::new(3, 60);
        let mut cors = CorsPolicy::new(&e, false);
        cors.allow_origin(&e, String::from_str(&e, "https://app.example.com"));

        let request = GatewayRequest {
            path: String::from_str(&e, "/public"),
            method: HttpMethod::Get,
            origin: Some(String::from_str(&e, "https://app.example.com")),
            auth_token: None,
            required_scope: String::from_str(&e, "read"),
        };

        let response =
            process_request(0, &routes, &mut rate_limiter, &mut circuit, &cors, &request);
        assert_eq!(response.outcome, GatewayOutcome::Routed);
        assert_eq!(response.status_code, 200);
    }

    #[test]
    fn test_process_request_expired_token_unauthorized() {
        let e = env();
        let routes = sample_routes(&e);
        let mut rate_limiter = RateLimitState::new(10, 60, 0);
        let mut circuit = CircuitBreaker::new(3, 60);
        let cors = CorsPolicy::new(&e, false);
        // Token issued at t=0 with a 100s TTL; the request arrives at t=200,
        // well past expiry.
        let token = AuthToken::new(
            String::from_str(&e, "user-1"),
            0,
            100,
            String::from_str(&e, "read"),
        );

        let request = GatewayRequest {
            path: String::from_str(&e, "/private"),
            method: HttpMethod::Get,
            origin: None,
            auth_token: Some(token),
            required_scope: String::from_str(&e, "read"),
        };

        let response = process_request(
            200,
            &routes,
            &mut rate_limiter,
            &mut circuit,
            &cors,
            &request,
        );
        assert_eq!(response.outcome, GatewayOutcome::Unauthorized);
        assert_eq!(response.status_code, 401);
    }

    #[test]
    fn test_process_request_invalid_scope_unauthorized() {
        let e = env();
        let routes = sample_routes(&e);
        let mut rate_limiter = RateLimitState::new(10, 60, 0);
        let mut circuit = CircuitBreaker::new(3, 60);
        let cors = CorsPolicy::new(&e, false);
        let token = AuthToken::new(
            String::from_str(&e, "user-1"),
            0,
            3600,
            String::from_str(&e, "write"),
        );

        let request = GatewayRequest {
            path: String::from_str(&e, "/private"),
            method: HttpMethod::Get,
            origin: None,
            auth_token: Some(token),
            required_scope: String::from_str(&e, "read"),
        };

        let response =
            process_request(0, &routes, &mut rate_limiter, &mut circuit, &cors, &request);
        assert_eq!(response.outcome, GatewayOutcome::Unauthorized);
        assert_eq!(response.status_code, 401);
    }

    #[test]
    fn test_process_request_rate_limit_resets_after_window() {
        let e = env();
        let routes = sample_routes(&e);
        let mut rate_limiter = RateLimitState::new(1, 60, 0);
        let mut circuit = CircuitBreaker::new(3, 60);
        let cors = CorsPolicy::new(&e, false);

        let request = GatewayRequest {
            path: String::from_str(&e, "/public"),
            method: HttpMethod::Get,
            origin: None,
            auth_token: None,
            required_scope: String::from_str(&e, "read"),
        };

        let first = process_request(0, &routes, &mut rate_limiter, &mut circuit, &cors, &request);
        assert_eq!(first.outcome, GatewayOutcome::Routed);

        let second = process_request(
            10,
            &routes,
            &mut rate_limiter,
            &mut circuit,
            &cors,
            &request,
        );
        assert_eq!(second.outcome, GatewayOutcome::RateLimited);

        // Once the 60s window elapses, the same client is allowed again.
        let third = process_request(
            60,
            &routes,
            &mut rate_limiter,
            &mut circuit,
            &cors,
            &request,
        );
        assert_eq!(third.outcome, GatewayOutcome::Routed);
    }

    #[test]
    fn test_process_request_circuit_recovers_through_half_open() {
        let e = env();
        let routes = sample_routes(&e);
        let mut rate_limiter = RateLimitState::new(10, 60, 0);
        let mut circuit = CircuitBreaker::new(1, 30);
        circuit.record_failure(0);
        let cors = CorsPolicy::new(&e, false);

        let request = GatewayRequest {
            path: String::from_str(&e, "/public"),
            method: HttpMethod::Get,
            origin: None,
            auth_token: None,
            required_scope: String::from_str(&e, "read"),
        };

        // Still within the reset timeout: requests keep failing fast.
        let blocked = process_request(
            10,
            &routes,
            &mut rate_limiter,
            &mut circuit,
            &cors,
            &request,
        );
        assert_eq!(blocked.outcome, GatewayOutcome::CircuitOpen);

        // Reset timeout elapsed: the breaker moves to HalfOpen and lets the
        // request reach routing.
        let recovered = process_request(
            30,
            &routes,
            &mut rate_limiter,
            &mut circuit,
            &cors,
            &request,
        );
        assert_eq!(recovered.outcome, GatewayOutcome::Routed);
        assert_eq!(circuit.state, CircuitState::HalfOpen);

        // A subsequent success fully closes the breaker again.
        circuit.record_success();
        assert_eq!(circuit.state, CircuitState::Closed);
    }
}
