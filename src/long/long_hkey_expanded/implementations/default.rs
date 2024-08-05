use std::sync::Arc;

use crate::long::LongHkeyExpanded;

impl Default for LongHkeyExpanded {
    fn default() -> Self {
        Self::new(0, 0, Arc::from([]))
    }
}
