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

/// Who keeps the rows that a snapshot sync copies from orrery's admin into
/// a table hydra's own admin also adds to: signup tag grants, Link Tokens,
/// Zooniverse projects. Such a copy carries orrery's row id; a row made here
/// carries none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyKeeper {
    /// Until cutover. A copy is listed here but not changed or deleted here:
    /// the next snapshot would undo it, and nothing hydra did with it would
    /// reach orrery, which is the one serving the public.
    Orrery,

    /// After cutover. The syncs stop, and a copy is hydra's like any other
    /// row.
    Hydra,
}

impl CopyKeeper {
    /// Who keeps the copies in this build.
    pub const NOW: CopyKeeper = if PRIMARY {
        CopyKeeper::Hydra
    } else {
        CopyKeeper::Orrery
    };

    /// Whether orrery keeps the copies, and so whether the snapshot syncs
    /// that make them run.
    pub const fn orrery_keeps_copies(self) -> bool {
        matches!(self, CopyKeeper::Orrery)
    }

    /// Whether the row whose orrery id is `orrery_id` is a copy orrery keeps.
    pub const fn orrery_owns(self, orrery_id: Option<i64>) -> bool {
        self.orrery_keeps_copies() && orrery_id.is_some()
    }
}

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
