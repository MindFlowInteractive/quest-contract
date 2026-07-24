#![no_std]

//! Backend Core Module
//!
//! Provides consistent response formatting with:
//! - Response envelope pattern for all responses
//! - Pagination metadata and support
//! - Consistent error response formatting
//! - Caching headers support
//! - Content negotiation support
//! - Compression metadata support

use soroban_sdk::{contracttype, Env, String};

//
// ──────────────────────────────────────────────────────────
// RESPONSE ENVELOPE STRUCTURES
// ──────────────────────────────────────────────────────────
//

/// HTTP-like status codes for response categorization
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResponseStatus {
    Success = 200,
    Created = 201,
    BadRequest = 400,
    Unauthorized = 401,
    Forbidden = 403,
    NotFound = 404,
    Conflict = 409,
    UnprocessableEntity = 422,
    InternalServerError = 500,
}

impl ResponseStatus {
    pub fn is_success(self) -> bool {
        matches!(self, ResponseStatus::Success | ResponseStatus::Created)
    }

    pub fn is_error(self) -> bool {
        !self.is_success()
    }
}

/// Pagination metadata for paginated responses
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaginationMetadata {
    pub page: u32,
    pub page_size: u32,
    pub total_items: u32,
    pub total_pages: u32,
    pub has_next_page: bool,
    pub has_prev_page: bool,
}

impl PaginationMetadata {
    pub fn new(page: u32, page_size: u32, total_items: u32) -> Self {
        if page_size == 0 {
            return Self {
                page: 0,
                page_size: 1,
                total_items,
                total_pages: total_items,
                has_next_page: false,
                has_prev_page: false,
            };
        }

        let total_pages = (total_items + page_size - 1) / page_size;
        let has_next_page = page < total_pages.saturating_sub(1);
        let has_prev_page = page > 0;

        Self {
            page,
            page_size,
            total_items,
            total_pages,
            has_next_page,
            has_prev_page,
        }
    }

    pub fn validate(&self) -> bool {
        self.page_size > 0 && (self.total_items == 0 || self.page < self.total_pages)
    }
}

/// Content type enumeration for content negotiation
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentType {
    Json = 0,
    OctetStream = 1,
    TextPlain = 2,
}

/// Compression type enumeration
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompressionType {
    None = 0,
    Gzip = 1,
    Deflate = 2,
    Brotli = 3,
}

/// Error details for error responses
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorDetail {
    pub code: u32,
    pub message: String,
}

impl ErrorDetail {
    pub fn new(code: u32, message: String) -> Self {
        Self { code, message }
    }
}

/// Response metadata containing timing and technical information
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResponseMetadata {
    pub request_id: String,
    pub timestamp: u64,
    pub processing_time_ms: u32,
}

impl ResponseMetadata {
    pub fn new(env: &Env, request_id: String) -> Self {
        Self {
            request_id,
            timestamp: env.ledger().timestamp(),
            processing_time_ms: 0,
        }
    }

    pub fn with_processing_time(mut self, ms: u32) -> Self {
        self.processing_time_ms = ms;
        self
    }
}

/// Standard response envelope for all API responses
#[contracttype]
#[derive(Clone, Debug)]
pub struct ResponseEnvelope {
    pub status: ResponseStatus,
    pub metadata: ResponseMetadata,
    pub error: Option<ErrorDetail>,
    pub pagination: Option<PaginationMetadata>,
    pub message: String,
}

impl ResponseEnvelope {
    pub fn success(env: &Env, message: String, request_id: String) -> Self {
        Self {
            status: ResponseStatus::Success,
            metadata: ResponseMetadata::new(env, request_id),
            error: None,
            pagination: None,
            message,
        }
    }

    pub fn created(env: &Env, message: String, request_id: String) -> Self {
        Self {
            status: ResponseStatus::Created,
            metadata: ResponseMetadata::new(env, request_id),
            error: None,
            pagination: None,
            message,
        }
    }

    pub fn error(
        env: &Env,
        status: ResponseStatus,
        error: ErrorDetail,
        request_id: String,
    ) -> Self {
        let message = error.message.clone();
        Self {
            status,
            metadata: ResponseMetadata::new(env, request_id),
            error: Some(error),
            pagination: None,
            message,
        }
    }

    pub fn paginated(
        env: &Env,
        message: String,
        pagination: PaginationMetadata,
        request_id: String,
    ) -> Self {
        Self {
            status: ResponseStatus::Success,
            metadata: ResponseMetadata::new(env, request_id),
            error: None,
            pagination: Some(pagination),
            message,
        }
    }

    pub fn with_processing_time(mut self, ms: u32) -> Self {
        self.metadata = self.metadata.with_processing_time(ms);
        self
    }
}

//
// ──────────────────────────────────────────────────────────
// CACHE CONTROL STRUCTURES
// ──────────────────────────────────────────────────────────
//

/// Cache control policy
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CachePolicy {
    NoCache = 0,
    ShortTerm = 1,  // 5 minutes
    MediumTerm = 2, // 1 hour
    LongTerm = 3,   // 1 day
    Immutable = 4,
}

impl CachePolicy {
    pub fn duration_seconds(&self) -> u32 {
        match self {
            CachePolicy::NoCache => 0,
            CachePolicy::ShortTerm => 300,
            CachePolicy::MediumTerm => 3600,
            CachePolicy::LongTerm => 86400,
            CachePolicy::Immutable => 31536000,
        }
    }
}

/// ETag for cache validation
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ETag {
    pub value: String,
    pub is_weak: bool,
}

impl ETag {
    pub fn strong(value: String) -> Self {
        Self {
            value,
            is_weak: false,
        }
    }

    pub fn weak(value: String) -> Self {
        Self {
            value,
            is_weak: true,
        }
    }
}

/// Cache headers for HTTP response
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheHeaders {
    pub policy: CachePolicy,
    pub etag: Option<ETag>,
    pub last_modified: u64,
}

impl CacheHeaders {
    pub fn new(env: &Env, policy: CachePolicy) -> Self {
        Self {
            policy,
            etag: None,
            last_modified: env.ledger().timestamp(),
        }
    }

    pub fn with_etag(mut self, etag: ETag) -> Self {
        self.etag = Some(etag);
        self
    }

    pub fn is_expired(&self, env: &Env) -> bool {
        let current = env.ledger().timestamp();
        let age = current.saturating_sub(self.last_modified);
        age > (self.policy.duration_seconds() as u64)
    }
}

//
// ──────────────────────────────────────────────────────────
// CONTENT NEGOTIATION STRUCTURES
// ──────────────────────────────────────────────────────────
//

/// Accept header preference with quality factor
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptPreference {
    pub content_type: ContentType,
    pub quality: u32,
}

impl AcceptPreference {
    pub fn new(content_type: ContentType, quality: u32) -> Self {
        Self {
            content_type,
            quality: quality.min(100),
        }
    }

    pub fn is_acceptable(&self) -> bool {
        self.quality > 0
    }
}

/// Content negotiation result
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NegotiationResult {
    pub content_type: ContentType,
    pub compression: CompressionType,
    pub is_acceptable: bool,
}

impl NegotiationResult {
    pub fn success(content_type: ContentType, compression: CompressionType) -> Self {
        Self {
            content_type,
            compression,
            is_acceptable: true,
        }
    }

    pub fn unacceptable() -> Self {
        Self {
            content_type: ContentType::Json,
            compression: CompressionType::None,
            is_acceptable: false,
        }
    }
}

//
// ──────────────────────────────────────────────────────────
// COMPRESSION STRUCTURES
// ──────────────────────────────────────────────────────────
//

/// Compression statistics
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompressionStats {
    pub original_size: u32,
    pub compressed_size: u32,
    pub compression_ratio: u32,
    pub compression_time_ms: u32,
}

impl CompressionStats {
    pub fn new(original_size: u32, compressed_size: u32, time_ms: u32) -> Self {
        let ratio = if original_size > 0 {
            ((original_size.saturating_sub(compressed_size)) * 100) / original_size
        } else {
            0
        };

        Self {
            original_size,
            compressed_size,
            compression_ratio: ratio,
            compression_time_ms: time_ms,
        }
    }

    pub fn was_effective(&self) -> bool {
        self.compression_ratio > 10
    }

    pub fn expansion_percentage(&self) -> u32 {
        if self.original_size == 0 {
            0
        } else {
            (self.compressed_size * 100) / self.original_size
        }
    }
}

/// Compression decision based on content
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompressionDecision {
    pub recommended: CompressionType,
    pub should_compress: bool,
    pub estimated_savings: u32,
}

impl CompressionDecision {
    pub fn negotiate(content_size: u32, content_type: ContentType) -> Self {
        let should_compress = match content_type {
            ContentType::Json => content_size > 256,
            ContentType::TextPlain => content_size > 128,
            ContentType::OctetStream => false,
        };

        let estimated_savings = match content_type {
            ContentType::Json => 60,
            ContentType::TextPlain => 40,
            ContentType::OctetStream => 0,
        };

        Self {
            recommended: if should_compress {
                CompressionType::Gzip
            } else {
                CompressionType::None
            },
            should_compress,
            estimated_savings,
        }
    }
}

//
// ──────────────────────────────────────────────────────────
// HELPER FUNCTIONS
// ──────────────────────────────────────────────────────────
//

pub fn validate_pagination(page: u32, page_size: u32) -> bool {
    const MAX_PAGE_SIZE: u32 = 1000;
    const MAX_PAGE: u32 = 1_000_000;
    page <= MAX_PAGE && page_size > 0 && page_size <= MAX_PAGE_SIZE
}

pub fn create_error(code: u32, message: String) -> ErrorDetail {
    ErrorDetail::new(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_status_success() {
        assert!(ResponseStatus::Success.is_success());
        assert!(ResponseStatus::Created.is_success());
        assert!(!ResponseStatus::BadRequest.is_success());
    }

    #[test]
    fn test_pagination_metadata_creation() {
        let pagination = PaginationMetadata::new(0, 10, 25);
        assert_eq!(pagination.page, 0);
        assert_eq!(pagination.page_size, 10);
        assert_eq!(pagination.total_items, 25);
        assert_eq!(pagination.total_pages, 3);
        assert!(pagination.has_next_page);
        assert!(!pagination.has_prev_page);
    }

    #[test]
    fn test_pagination_metadata_last_page() {
        let pagination = PaginationMetadata::new(2, 10, 25);
        assert_eq!(pagination.page, 2);
        assert!(!pagination.has_next_page);
        assert!(pagination.has_prev_page);
    }

    #[test]
    fn test_pagination_metadata_exact_pages() {
        let pagination = PaginationMetadata::new(0, 10, 20);
        assert_eq!(pagination.total_pages, 2);
        assert!(pagination.has_next_page);
    }

    #[test]
    fn test_error_detail_creation() {
        let env = Env::default();
        let error = ErrorDetail::new(400, String::from_str(&env, "Bad request"));
        assert_eq!(error.code, 400);
    }

    #[test]
    fn test_validate_pagination_valid() {
        assert!(validate_pagination(0, 10));
        assert!(validate_pagination(5, 50));
    }

    #[test]
    fn test_validate_pagination_invalid() {
        assert!(!validate_pagination(0, 0));
        assert!(!validate_pagination(0, 2000));
    }

    #[test]
    fn test_cache_policy_duration() {
        assert_eq!(CachePolicy::NoCache.duration_seconds(), 0);
        assert_eq!(CachePolicy::ShortTerm.duration_seconds(), 300);
        assert_eq!(CachePolicy::MediumTerm.duration_seconds(), 3600);
        assert_eq!(CachePolicy::LongTerm.duration_seconds(), 86400);
        assert_eq!(CachePolicy::Immutable.duration_seconds(), 31536000);
    }

    #[test]
    fn test_etag_creation() {
        let env = Env::default();
        let etag = ETag::strong(String::from_str(&env, "abc123"));
        assert!(!etag.is_weak);
    }

    #[test]
    fn test_etag_weak() {
        let env = Env::default();
        let etag = ETag::weak(String::from_str(&env, "abc123"));
        assert!(etag.is_weak);
    }

    #[test]
    fn test_accept_preference_creation() {
        let pref = AcceptPreference::new(ContentType::Json, 100);
        assert_eq!(pref.quality, 100);
        assert!(pref.is_acceptable());
    }

    #[test]
    fn test_accept_preference_not_acceptable() {
        let pref = AcceptPreference::new(ContentType::Json, 0);
        assert!(!pref.is_acceptable());
    }

    #[test]
    fn test_negotiation_result_success() {
        let result = NegotiationResult::success(ContentType::Json, CompressionType::Gzip);
        assert!(result.is_acceptable);
    }

    #[test]
    fn test_compression_stats_calculation() {
        let stats = CompressionStats::new(1000, 400, 50);
        assert_eq!(stats.original_size, 1000);
        assert_eq!(stats.compressed_size, 400);
        assert_eq!(stats.compression_ratio, 60);
        assert_eq!(stats.expansion_percentage(), 40);
    }

    #[test]
    fn test_compression_stats_effectiveness() {
        let effective = CompressionStats::new(1000, 800, 50);
        assert!(effective.was_effective());

        let not_effective = CompressionStats::new(1000, 950, 50);
        assert!(!not_effective.was_effective());
    }

    #[test]
    fn test_compression_decision_json() {
        let decision = CompressionDecision::negotiate(500, ContentType::Json);
        assert!(decision.should_compress);
    }

    #[test]
    fn test_compression_decision_json_small() {
        let decision = CompressionDecision::negotiate(100, ContentType::Json);
        assert!(!decision.should_compress);
    }
}
