# Backend Core Module

A Soroban smart contract library providing consistent response formatting, compression support, caching headers, pagination metadata, and content negotiation for the Quest Service ecosystem.

## Features

### Response Envelope Pattern
- **ResponseEnvelope**: Standardized response wrapper for all API responses
- **ResponseStatus**: HTTP-like status codes (Success, Created, BadRequest, Unauthorized, etc.)
- **ResponseMetadata**: Request tracking with ID, timestamp, and processing time
- **ErrorDetail**: Structured error information with code and message

### Pagination Support
- **PaginationMetadata**: Complete pagination information
  - Page number and size tracking
  - Total items and pages calculation
  - Next/previous page indicators
  - Validation helper functions

### Error Handling
- Standardized error details with code and message
- Error creation helper functions
- HTTP-compliant status codes
- Extensible error structure

### Caching Control
- **CachePolicy**: Five-tier caching strategy (NoCache, ShortTerm, MediumTerm, LongTerm, Immutable)
- **CacheHeaders**: Complete cache header support with ETag validation
- **ETag**: Both strong and weak ETag support for cache validation
- Automatic cache expiration checking
- Configurable cache durations

### Compression Support
- **CompressionType**: Support for multiple compression algorithms (None, Gzip, Deflate, Brotli)
- **CompressionStats**: Monitor compression effectiveness
- **CompressionDecision**: Intelligent compression selection based on content type and size
- Compression ratio tracking and effectiveness metrics

### Content Negotiation
- **ContentType**: Three content types (Json, OctetStream, TextPlain)
- **AcceptPreference**: Quality-based accept header preferences
- **NegotiationResult**: Content negotiation decision results
- Automatic selection of best content type based on quality factors

## Usage

### Creating a Successful Response

```rust
use backend_core::{ResponseEnvelope, Env, String};

let env = Env::default();
let response = ResponseEnvelope::success(
    &env,
    String::from_str(&env, "Operation completed"),
    String::from_str(&env, "req-12345")
);
```

### Paginated Responses

```rust
use backend_core::{ResponseEnvelope, PaginationMetadata, Env, String};

let env = Env::default();
let pagination = PaginationMetadata::new(0, 10, 42);

let response = ResponseEnvelope::paginated(
    &env,
    String::from_str(&env, "Results retrieved"),
    pagination,
    String::from_str(&env, "req-12345")
);
```

### Error Responses

```rust
use backend_core::{ResponseEnvelope, ResponseStatus, ErrorDetail, Env, String};

let env = Env::default();
let error = ErrorDetail::new(
    404,
    String::from_str(&env, "Resource not found")
);

let response = ResponseEnvelope::error(
    &env,
    ResponseStatus::NotFound,
    error,
    String::from_str(&env, "req-12345")
);
```

### Cache Control

```rust
use backend_core::{CacheHeaders, CachePolicy, Env};

let env = Env::default();
let cache_headers = CacheHeaders::new(&env, CachePolicy::MediumTerm);

// Check if cache is expired
if cache_headers.is_expired(&env) {
    // Revalidate or refresh
}
```

### Compression Decision

```rust
use backend_core::{CompressionDecision, ContentType};

let decision = CompressionDecision::negotiate(512, ContentType::Json);

if decision.should_compress {
    // Apply compression with decision.recommended
}
```

## Response Structures

### ResponseEnvelope
```rust
pub struct ResponseEnvelope {
    pub status: ResponseStatus,
    pub metadata: ResponseMetadata,
    pub error: Option<ErrorDetail>,
    pub pagination: Option<PaginationMetadata>,
    pub message: String,
}
```

### PaginationMetadata
```rust
pub struct PaginationMetadata {
    pub page: u32,
    pub page_size: u32,
    pub total_items: u32,
    pub total_pages: u32,
    pub has_next_page: bool,
    pub has_prev_page: bool,
}
```

### CacheHeaders
```rust
pub struct CacheHeaders {
    pub policy: CachePolicy,
    pub etag: Option<ETag>,
    pub last_modified: u64,
}
```

## Validation

The module includes validation helpers:

```rust
use backend_core::validate_pagination;

// Validates page and page_size parameters
assert!(validate_pagination(0, 10));  // Valid
assert!(!validate_pagination(0, 0));  // Invalid (page_size = 0)
assert!(!validate_pagination(0, 2000)); // Invalid (exceeds max)
```

## Testing

The module includes 17 comprehensive unit tests covering:
- Response status checking
- Pagination metadata creation and validation
- Error detail creation
- Cache policy duration calculations
- ETag creation (strong and weak)
- Accept preferences and negotiation
- Compression statistics and decisions

Run tests with:
```bash
cargo test -p backend-core --lib
```

## Status Codes

Supported HTTP-like status codes:
- **200**: Success (default success response)
- **201**: Created (successful creation)
- **400**: Bad Request
- **401**: Unauthorized
- **403**: Forbidden
- **404**: Not Found
- **409**: Conflict
- **422**: Unprocessable Entity (validation error)
- **500**: Internal Server Error

## Cache Policies

- **NoCache**: No caching (duration: 0 seconds)
- **ShortTerm**: Short duration caching (duration: 300 seconds / 5 minutes)
- **MediumTerm**: Medium duration caching (duration: 3600 seconds / 1 hour)
- **LongTerm**: Long duration caching (duration: 86400 seconds / 1 day)
- **Immutable**: Immutable content (duration: 31536000 seconds / 1 year)

## Compression Types

- **None**: No compression
- **Gzip**: gzip compression (RFC 1952)
- **Deflate**: Deflate compression (RFC 1951)
- **Brotli**: Brotli compression

## Content Types

- **Json**: application/json
- **OctetStream**: application/octet-stream
- **TextPlain**: text/plain

## Building

```bash
# Build the library
cargo build -p backend-core

# Build optimized release
cargo build -p backend-core --release

# Run tests
cargo test -p backend-core --lib

# Check compilation
cargo check -p backend-core
```

## Dependencies

- **soroban-sdk** (21.0.0+): Stellar Soroban SDK for smart contracts

## Architecture

The module is designed as a single self-contained library following Soroban patterns:
- All structures use `#[contracttype]` macro for Soroban compatibility
- No external dependencies beyond soroban-sdk
- Zero-copy design suitable for blockchain constraints
- Comprehensive validation helpers
- Extensible design for future additions

## Contribution Guidelines

When extending this module:
1. Keep structures Soroban-compatible (use `#[contracttype]`)
2. Use u32 and u64 instead of u8 or u16 for contract types
3. Add unit tests for new functionality
4. Document public APIs with comments
5. Follow the existing code style and organization

## License

MIT License - See project root for details
