#![no_std]
use soroban_sdk::{Address, Env, String, Vec};

#[derive(Debug, Clone, PartialEq)]
pub enum ValidationError {
    InvalidAmount,
    AmountTooLow,
    AmountTooHigh,
    InvalidAddress,
    AddressNotAuthorized,
    ValueOutOfRange,
    InvalidEnumValue,
    StringTooLong,
    StringTooShort,
    StringInvalidChars,
    EmptyInput,
    CustomValidationFailed,
    InvalidLength,
}

pub trait Validator<T> {
    fn validate(&self, env: &Env, value: &T) -> Result<(), ValidationError>;
}

// Amount validation functions
pub fn validate_positive_amount(amount: i128) -> Result<(), ValidationError> {
    if amount <= 0 {
        return Err(ValidationError::InvalidAmount);
    }
    Ok(())
}

pub fn validate_amount_range(amount: i128, min: i128, max: i128) -> Result<(), ValidationError> {
    if amount < min {
        return Err(ValidationError::AmountTooLow);
    }
    if amount > max {
        return Err(ValidationError::AmountTooHigh);
    }
    Ok(())
}

pub fn validate_amount_non_zero(amount: i128) -> Result<(), ValidationError> {
    if amount == 0 {
        return Err(ValidationError::InvalidAmount);
    }
    Ok(())
}

// Address validation functions
pub fn validate_address_not_zero(address: &Address) -> Result<(), ValidationError> {
    if address.is_zero() {
        return Err(ValidationError::InvalidAddress);
    }
    Ok(())
}

pub fn validate_same_address(address1: &Address, address2: &Address) -> Result<(), ValidationError> {
    if address1 != address2 {
        return Err(ValidationError::InvalidAddress);
    }
    Ok(())
}

pub fn validate_different_address(address1: &Address, address2: &Address) -> Result<(), ValidationError> {
    if address1 == address2 {
        return Err(ValidationError::InvalidAddress);
    }
    Ok(())
}

pub fn is_authorized(address: &Address, authorized: &Vec<Address>) -> bool {
    authorized.contains(address)
}

// Range validation for numeric types
pub fn validate_range<T: PartialOrd>(value: T, min: T, max: T) -> Result<(), ValidationError> {
    if value < min || value > max {
        return Err(ValidationError::ValueOutOfRange);
    }
    Ok(())
}

pub fn validate_min<T: PartialOrd>(value: T, min: T) -> Result<(), ValidationError> {
    if value < min {
        return Err(ValidationError::ValueOutOfRange);
    }
    Ok(())
}

pub fn validate_max<T: PartialOrd>(value: T, max: T) -> Result<(), ValidationError> {
    if value > max {
        return Err(ValidationError::ValueOutOfRange);
    }
    Ok(())
}

// Enum validation
pub fn validate_enum_value<T: PartialEq>(value: T, allowed_values: &[T]) -> Result<(), ValidationError> {
    if !allowed_values.contains(&value) {
        return Err(ValidationError::InvalidEnumValue);
    }
    Ok(())
}

// String validation functions
pub fn validate_string_length(s: &String, min_len: usize, max_len: usize) -> Result<(), ValidationError> {
    let len = s.len();
    if len < min_len {
        return Err(ValidationError::StringTooShort);
    }
    if len > max_len {
        return Err(ValidationError::StringTooLong);
    }
    Ok(())
}

pub fn validate_string_not_empty(s: &String) -> Result<(), ValidationError> {
    if s.is_empty() {
        return Err(ValidationError::EmptyInput);
    }
    Ok(())
}

pub fn validate_alphanumeric(s: &String) -> Result<(), ValidationError> {
    for c in s.chars() {
        if !c.is_alphanumeric() {
            return Err(ValidationError::StringInvalidChars);
        }
    }
    Ok(())
}

// Custom validator support
pub struct CustomValidator<F> {
    validator: F,
}

impl<F> CustomValidator<F>
where
    F: Fn(&Env, &[u8]) -> Result<(), ValidationError>,
{
    pub fn new(f: F) -> Self {
        CustomValidator { validator: f }
    }
}

impl<F> Validator<&[u8]> for CustomValidator<F>
where
    F: Fn(&Env, &[u8]) -> Result<(), ValidationError>,
{
    fn validate(&self, env: &Env, value: &&[u8]) -> Result<(), ValidationError> {
        (self.validator)(env, value)
    }
}

// Validation macros
#[macro_export]
macro_rules! require {
    ($condition:expr, $error:expr) => {
        if !($condition) {
            return Err($error);
        }
    };
}

#[macro_export]
macro_rules! validate_amount {
    ($amount:expr) => {
        crate::validate_positive_amount($amount)?;
    };
    ($amount:expr, $min:expr, $max:expr) => {
        crate::validate_amount_range($amount, $min, $max)?;
    };
}

#[macro_export]
macro_rules! validate_address {
    ($address:expr) => {
        crate::validate_address_not_zero($address)?;
    };
}

#[macro_export]
macro_rules! validate_range {
    ($value:expr, $min:expr, $max:expr) => {
        crate::validate_range($value, $min, $max)?;
    };
}

#[macro_export]
macro_rules! validate_string {
    ($s:expr) => {
        crate::validate_string_not_empty($s)?;
    };
    ($s:expr, $min:expr, $max:expr) => {
        crate::validate_string_length($s, $min, $max)?;
    };
}

// Vec validation
pub fn validate_vec_not_empty<T>(vec: &Vec<T>) -> Result<(), ValidationError> {
    if vec.is_empty() {
        return Err(ValidationError::EmptyInput);
    }
    Ok(())
}

pub fn validate_vec_length<T>(vec: &Vec<T>, min_len: usize, max_len: usize) -> Result<(), ValidationError> {
    let len = vec.len();
    if len < min_len {
        return Err(ValidationError::InvalidLength);
    }
    if len > max_len {
        return Err(ValidationError::InvalidLength);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::Env;

    #[test]
    fn test_validate_positive_amount() {
        assert!(validate_positive_amount(100).is_ok());
        assert!(matches!(validate_positive_amount(0), Err(ValidationError::InvalidAmount)));
        assert!(matches!(validate_positive_amount(-50), Err(ValidationError::InvalidAmount)));
    }

    #[test]
    fn test_validate_amount_range() {
        assert!(validate_amount_range(50, 0, 100).is_ok());
        assert!(matches!(validate_amount_range(-10, 0, 100), Err(ValidationError::AmountTooLow)));
        assert!(matches!(validate_amount_range(150, 0, 100), Err(ValidationError::AmountTooHigh)));
    }

    #[test]
    fn test_validate_range() {
        assert!(validate_range(50u32, 0u32, 100u32).is_ok());
        assert!(matches!(validate_range(150u32, 0u32, 100u32), Err(ValidationError::ValueOutOfRange)));
    }

    #[test]
    fn test_string_validation() {
        let env = Env::default();
        let s = String::from_str(&env, "test123");
        assert!(validate_string_not_empty(&s).is_ok());
        assert!(validate_string_length(&s, 1, 10).is_ok());
        assert!(validate_alphanumeric(&s).is_ok());
    }

    #[test]
    fn test_custom_validator() {
        let env = Env::default();
        let validator = CustomValidator::new(|_env, data| {
            if data.len() > 5 {
                Ok(())
            } else {
                Err(ValidationError::InvalidLength)
            }
        });
        
        let data = b"123456";
        assert!(validator.validate(&env, &data).is_ok());
        
        let short_data = b"123";
        assert!(matches!(validator.validate(&env, &short_data), Err(ValidationError::InvalidLength)));
    }

    #[test]
    fn test_macros() {
        let result = || -> Result<(), ValidationError> {
            validate_amount!(100);
            validate_range!(50u32, 0u32, 100u32);
            Ok(())
        }();
        assert!(result.is_ok());
    }
}