// Concern: the named formats a workbook packs to, and reading one back from its name | Non-concern: writing those bytes (fsa1-xlsx), which verb takes one (ops.rs) | IO: (a name) -> PackFormat

use std::str::FromStr;

/// The formats `pack` can write, as a closed vocabulary rather than a string: no surface can name one
/// `fsa1-xlsx` does not write, and a second variant reaches both surfaces with no edit to either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackFormat {
    Xlsx,
}

impl PackFormat {
    /// Every format there is, in help-text order.
    pub const ALL: [PackFormat; 1] = [PackFormat::Xlsx];

    /// The single source of each variant's spelling, which [`PackFormat::from_str`] reads backward,
    /// and which names the extension a derived output takes.
    pub fn name(self) -> &'static str {
        match self {
            PackFormat::Xlsx => "xlsx",
        }
    }

    /// The accepted words, for a refusal or a schema. Beside the vocabulary rather than at each
    /// surface, so the two front ends cannot list it differently.
    pub fn choices() -> Vec<&'static str> {
        Self::ALL.iter().map(|f| f.name()).collect()
    }
}

/// The refusal carries nothing because the caller already holds both halves of what it prints: the
/// word it handed over, and [`PackFormat::choices`].
impl FromStr for PackFormat {
    type Err = ();

    fn from_str(s: &str) -> Result<PackFormat, ()> {
        PackFormat::ALL
            .into_iter()
            .find(|f| f.name() == s)
            .ok_or(())
    }
}
