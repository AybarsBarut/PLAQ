#![forbid(unsafe_code)]

//! Reversible signal primitives used by the PLAQ codec.
//!
//! The lossless modules use integers only. [`simulation`] is deliberately
//! separate because it models a physical stylus with floating-point state and
//! is not bit-perfect.

pub mod predictor;
pub mod rice;
pub mod simulation;
pub mod transform;

use std::fmt;

/// Errors raised by bounded decoding and reversible reconstruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    InvalidPredictor(u8),
    InvalidRiceParameter(u8),
    RiceQuotientTooLarge,
    UnexpectedEndOfPayload,
    SampleOutOfRange(i64),
    SizeOverflow,
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPredictor(id) => write!(f, "unknown predictor id {id}"),
            Self::InvalidRiceParameter(k) => write!(f, "invalid Rice parameter {k}"),
            Self::RiceQuotientTooLarge => write!(f, "Rice quotient exceeds the safety limit"),
            Self::UnexpectedEndOfPayload => write!(f, "compressed payload ended unexpectedly"),
            Self::SampleOutOfRange(value) => {
                write!(f, "reconstructed sample {value} is outside i32")
            }
            Self::SizeOverflow => write!(f, "encoded size calculation overflowed"),
        }
    }
}

impl std::error::Error for CoreError {}
