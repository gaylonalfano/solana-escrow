// Defining an error type using handy thiserror library:
// https://docs.rs/thiserror/1.0.25/thiserror/
// NOTE Instead of having to write the fmt::Display implementation
// ourselves, we use the handy thiserror library that does it for us
// using the #[error("..")] notation
use thiserror::Error;

// Let's implement a way to turn an EscrowError into a ProgramError
use solana_program::program_error::ProgramError;

#[derive(Error, Debug, Copy, Clone)]
pub enum EscrowError {
    /// Invalid instruction
    #[error("Invalid Instruction")]
    InvalidInstruction,
    /// Not Rent Exempt
    #[error("Not Rent Exempt")]
    NotRentExempt,
    /// Expected Amount Mismatch
    #[error("Expected Amount Mismatch")]
    ExpectedAmountMismatch,
    /// Amount Overflow
    #[error("Amount Overflow")]
    AmountOverflow,
}

// Let's implement a way to turn an EscrowError into a ProgramError
// Q: What is 'impl'?
// A: We're implementing a GENERIC TRAIT ('From' trait), which carries
// out the conversion. The ProgramError enum provides the Custom variant
// that allows us to convert from our program's EscrowError to a ProgramError.
// NOTE The reason we do this conversion is because the entrypoint returns
// a Result of either nothing or a ProgramError.
impl From<EscrowError> for ProgramError {
    fn from(e: EscrowError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
