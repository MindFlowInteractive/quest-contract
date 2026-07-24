use soroban_sdk::{contracttype, Address, String, Symbol, Val, Vec};

#[contracttype]
#[derive(Clone, Debug)]
pub enum FieldType {
    String,
    Symbol,
    Address,
    U64,
    I128,
    Bool,
    Object(Symbol),
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct FieldDefinition {
    pub name: Symbol,
    pub field_type: FieldType,
    pub required: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct TypeDefinition {
    pub name: Symbol,
    pub fields: Vec<FieldDefinition>,
    pub description: String,
}

#[contracttype]
#[derive(Clone, Debug)]
pub enum FilterOp {
    Eq,
    Neq,
    Gt,
    Lt,
    Gte,
    Lte,
    In,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Filter {
    pub field: Symbol,
    pub operator: FilterOp,
    pub value: Val,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Sort {
    pub field: Symbol,
    pub direction: SortDirection,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Pagination {
    pub limit: u32,
    pub cursor: Option<Val>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct QueryInput {
    pub type_name: Symbol,
    pub filters: Vec<Filter>,
    pub sort: Option<Sort>,
    pub pagination: Pagination,
    pub fields: Vec<Symbol>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct QueryResult {
    pub items: Vec<Val>,
    pub total_count: u32,
    pub next_cursor: Option<Val>,
    pub has_more: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub enum MutationOp {
    Create,
    Update,
    Delete,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct MutationInput {
    pub type_name: Symbol,
    pub operation: MutationOp,
    pub id: Option<Val>,
    pub data: Vec<(Symbol, Val)>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct MutationResult {
    pub success: bool,
    pub id: Option<Val>,
    pub error: Option<String>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct SubscriptionInput {
    pub source_contract: Address,
    pub topic: Symbol,
    pub subscriber: Address,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Subscription {
    pub id: u64,
    pub source_contract: Address,
    pub topic: Symbol,
    pub subscriber: Address,
    pub created_at: u64,
    pub active: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct SubscriptionEvent {
    pub subscription_id: u64,
    pub event_id: u64,
    pub timestamp: u64,
    pub data: Val,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct BatchQueryInput {
    pub queries: Vec<QueryInput>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct BatchQueryResult {
    pub results: Vec<QueryResult>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub enum Role {
    Admin,
    Operator,
    Reader,
}

#[contracttype]
pub enum DataKey {
    Admin,
    Initialized,
    TypeRegistry(Symbol),
    TypeCount,
    Record(Symbol, Val),
    RecordIndex(Symbol, u32),
    RecordCounter(Symbol),
    Subscription(u64),
    SubscriptionCount,
    SubscriptionByContract(Symbol, u64),
    SubscriptionEvent(u64, u64),
    SubscriptionEventCount(u64),
    Role(Address),
    Paused,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EngineError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    TypeNotFound = 4,
    TypeAlreadyExists = 5,
    RecordNotFound = 6,
    InvalidFilter = 7,
    InvalidField = 8,
    Paused = 9,
    NotPaused = 10,
    InvalidPagination = 11,
    SubscriptionNotFound = 12,
    InvalidMutation = 13,
    BatchLimitExceeded = 14,
}

pub const MAX_PAGE_SIZE: u32 = 50;
pub const MAX_BATCH_SIZE: u32 = 20;