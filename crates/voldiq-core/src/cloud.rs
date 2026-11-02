use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum CloudState {
    #[default]
    Unknown = 0,
    NotPlaceholder = 1,
    FileNoRecallFlag = 2,
    FileRecallRequired = 3,
    DirectoryNoRecallFlag = 4,
    DirectoryRecallRequired = 5,
}

impl CloudState {
    pub fn is_file(self) -> bool {
        matches!(self, Self::FileNoRecallFlag | Self::FileRecallRequired)
    }
    pub fn is_directory(self) -> bool {
        matches!(
            self,
            Self::DirectoryNoRecallFlag | Self::DirectoryRecallRequired
        )
    }
    pub fn is_placeholder(self) -> bool {
        self.is_file() || self.is_directory()
    }
    pub fn recall_required(self) -> bool {
        matches!(
            self,
            Self::FileRecallRequired | Self::DirectoryRecallRequired
        )
    }
    pub(crate) fn code(self) -> i64 {
        self as u8 as i64
    }
    pub(crate) fn from_code(code: i64) -> Option<Self> {
        match code {
            0 => Some(Self::Unknown),
            1 => Some(Self::NotPlaceholder),
            2 => Some(Self::FileNoRecallFlag),
            3 => Some(Self::FileRecallRequired),
            4 => Some(Self::DirectoryNoRecallFlag),
            5 => Some(Self::DirectoryRecallRequired),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CloudFilter {
    Placeholder,
    RecallRequired,
    NoRecallFlag,
    NotPlaceholder,
    Unknown,
}

impl CloudFilter {
    pub fn matches(self, state: CloudState) -> bool {
        match self {
            Self::Placeholder => state.is_placeholder(),
            Self::RecallRequired => state.recall_required(),
            Self::NoRecallFlag => state.is_placeholder() && !state.recall_required(),
            Self::NotPlaceholder => state == CloudState::NotPlaceholder,
            Self::Unknown => state == CloudState::Unknown,
        }
    }
}

#[cfg(any(windows, test))]
pub(crate) fn from_windows_handle_attributes(attributes: u32, tag: Option<u32>) -> CloudState {
    if attributes & 0x400 == 0 {
        return if tag.is_none_or(|tag| tag == 0) {
            CloudState::NotPlaceholder
        } else {
            CloudState::Unknown
        };
    }
    let Some(tag) = tag else {
        return CloudState::Unknown;
    };
    // Windows Cloud Files tag family; other providers and unrecognized reparse tags remain unknown.
    if tag & !0xf000 != 0x9000001a {
        return if matches!(tag, 0xa0000003 | 0xa000000c) {
            CloudState::NotPlaceholder
        } else {
            CloudState::Unknown
        };
    }
    // 0x40000 means EA in handle metadata; RECALL_ON_OPEN uses it only in directory enumeration.
    let recall = attributes & (0x1000 | 0x400000) != 0;
    match (attributes & 0x10 != 0, recall) {
        (false, false) => CloudState::FileNoRecallFlag,
        (false, true) => CloudState::FileRecallRequired,
        (true, false) => CloudState::DirectoryNoRecallFlag,
        (true, true) => CloudState::DirectoryRecallRequired,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classification_requires_cloud_tag_evidence_and_keeps_unknown_separate() {
        assert_eq!(
            from_windows_handle_attributes(0x80, None),
            CloudState::NotPlaceholder
        );
        assert_eq!(
            from_windows_handle_attributes(0x400, None),
            CloudState::Unknown
        );
        assert_eq!(
            from_windows_handle_attributes(0x400, Some(0xa000000c)),
            CloudState::NotPlaceholder
        );
        assert_eq!(
            from_windows_handle_attributes(0x410, Some(0xa0000003)),
            CloudState::NotPlaceholder
        );
        for tag in [0x80000021, 0x80000015, 0x80000017, 0x9001001a, 0] {
            assert_eq!(
                from_windows_handle_attributes(0x400, Some(tag)),
                CloudState::Unknown
            );
        }
        assert_eq!(
            from_windows_handle_attributes(0x80, Some(0x9000001a)),
            CloudState::Unknown
        );
    }
    #[test]
    fn all_cloud_tag_variants_classify_file_directory_and_observed_recall_flags() {
        for variant in 0..16 {
            let tag = Some(0x9000001a | variant << 12);
            assert_eq!(
                from_windows_handle_attributes(0x400, tag),
                CloudState::FileNoRecallFlag
            );
            assert_eq!(
                from_windows_handle_attributes(0x410, tag),
                CloudState::DirectoryNoRecallFlag
            );
            for flag in [0x1000, 0x400000] {
                assert_eq!(
                    from_windows_handle_attributes(0x400 | flag, tag),
                    CloudState::FileRecallRequired
                );
                assert_eq!(
                    from_windows_handle_attributes(0x410 | flag, tag),
                    CloudState::DirectoryRecallRequired
                );
            }
            for pin in [0x40000, 0x80000, 0x100000] {
                assert_eq!(
                    from_windows_handle_attributes(0x400 | pin, tag),
                    CloudState::FileNoRecallFlag
                );
            }
        }
    }
    #[test]
    fn compact_state_storage_and_filters_do_not_launder_unknown_evidence() {
        assert_eq!(std::mem::size_of::<CloudState>(), 1);
        assert_eq!(CloudState::default(), CloudState::Unknown);
        for code in 0..6 {
            let state = CloudState::from_code(code).unwrap();
            assert_eq!(state.code(), code);
            assert_eq!(
                serde_json::from_str::<CloudState>(&serde_json::to_string(&state).unwrap())
                    .unwrap(),
                state
            );
        }
        assert_eq!(CloudState::from_code(-1), None);
        assert_eq!(CloudState::from_code(6), None);
        assert!(CloudFilter::Unknown.matches(CloudState::Unknown));
        assert!(!CloudFilter::NotPlaceholder.matches(CloudState::Unknown));
        assert!(!CloudFilter::NoRecallFlag.matches(CloudState::Unknown));
        assert!(CloudFilter::Placeholder.matches(CloudState::DirectoryRecallRequired));
        assert!(!CloudFilter::RecallRequired.matches(CloudState::FileNoRecallFlag));
        assert!(serde_json::from_str::<CloudFilter>("\"online_only\"").is_err());
    }
}
