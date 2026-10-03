//! Shared text-variable definitions. Instances refer to opaque identities rather
//! than names or vector positions. The kernel never interprets retained XML.
use serde::{Deserialize, Serialize};

/// A custom text definition shared by every referring story instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextVariable {
    pub id: String,
    pub name: String,
    /// Literal custom contents, not an instance's cached ResultText.
    pub contents: String,
}
