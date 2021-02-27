//! Aids for [`serde`].

/// Indicates whether the given [`bool`] is `false`.
///
/// # Purpose
///
/// Signature of this function matches for a convenient use in a
/// `skip_serializing_if` [`serde`]'s attribute.
#[allow(clippy::trivially_copy_pass_by_ref)] // required for `serde`
#[inline]
#[must_use]
pub fn is_false(val: &bool) -> bool {
    !*val
}
