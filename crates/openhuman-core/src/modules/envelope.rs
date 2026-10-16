//! Validation shared by host adapters that unwrap loadable-module envelopes.

/// A response with a valid shape contains either data or a structured error.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EnvelopeError<E> {
    /// The version or the `ok`/payload combination did not match the contract.
    Malformed,
    /// The module returned a valid structured failure.
    Operation(E),
}

/// Validates an optional version and the mutually exclusive result fields.
pub(crate) fn decode<T, E>(
    version: Option<&str>,
    expected_version: Option<&str>,
    ok: bool,
    data: Option<T>,
    error: Option<E>,
) -> Result<T, EnvelopeError<E>> {
    if expected_version.is_some_and(|expected| version != Some(expected))
        || ok != data.is_some()
        || ok == error.is_some()
    {
        return Err(EnvelopeError::Malformed);
    }
    match (ok, data, error) {
        (true, Some(data), None) => Ok(data),
        (false, None, Some(error)) => Err(EnvelopeError::Operation(error)),
        _ => Err(EnvelopeError::Malformed),
    }
}
