//! Whether this build of hydra is the primary server for scistarter.org.
//!
//! Until cutover, orrery is the primary: it sends the email, pushes to Constant
//! Contact, and so on, and hydra mirrors its data by sync. Anything hydra does
//! that reaches outside it would then happen twice, once from each. So a build
//! that is not the primary does all of its own work as usual, and stops only at
//! the point where the effect would leave hydra.
//!
//! Read from `HYDRA_PRIMARY` at compile time, from the root `.envrc` that every
//! service's `.envrc` sources. Only the exact value `false` turns it off: an
//! absent variable means the primary, so that after cutover the variable is
//! simply deleted. A build cannot lose it by accident, because the same
//! `.envrc` supplies the `env!` values (`DOMAIN` among them) that no service
//! compiles without.

/// Whether this build is the primary server, and so performs effects that
/// reach outside hydra.
pub const PRIMARY: bool = !is_false(option_env!("HYDRA_PRIMARY"));

const fn is_false(value: Option<&str>) -> bool {
    let Some(value) = value else {
        return false;
    };

    let (value, expected) = (value.as_bytes(), b"false");

    if value.len() != expected.len() {
        return false;
    }

    let mut i = 0;

    while i < expected.len() {
        if value[i] != expected[i] {
            return false;
        }

        i += 1;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::is_false;

    #[test]
    fn only_the_exact_word_false_turns_it_off() {
        assert!(is_false(Some("false")));

        for value in [
            None,
            Some(""),
            Some("true"),
            Some("False"),
            Some("0"),
            Some("no"),
        ] {
            assert!(!is_false(value), "{value:?}");
        }
    }
}
