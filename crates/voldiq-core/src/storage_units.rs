use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageUnits {
    #[default]
    Binary,
    Decimal,
}

impl StorageUnits {
    pub fn format(self, bytes: u128) -> String {
        let (base, labels) = match self {
            Self::Binary => (
                1024,
                ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB"],
            ),
            Self::Decimal => (1000, ["B", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"]),
        };
        let mut index = 0;
        let mut scale = 1;
        while index < labels.len() - 1 && bytes >= scale * base {
            index += 1;
            scale *= base;
        }
        let whole = bytes / scale;
        let tenths = (bytes % scale * 10 + scale / 2) / scale;
        if tenths == 10 && whole + 1 == base && index < labels.len() - 1 {
            return format!("1 {}", labels[index + 1]);
        }
        let label = labels[index];
        match tenths {
            0 => format!("{whole} {label}"),
            10 => format!("{} {label}", whole + 1),
            tenths => format!("{whole}.{tenths} {label}"),
        }
    }
}

pub fn multiplier(suffix: &str) -> Option<u128> {
    let folded = suffix.to_ascii_lowercase();
    let (base, exponent) = match folded.as_str() {
        "" | "b" => return Some(1),
        "kb" => (1000, 1),
        "mb" => (1000, 2),
        "gb" => (1000, 3),
        "tb" => (1000, 4),
        "pb" => (1000, 5),
        "eb" => (1000, 6),
        "zb" => (1000, 7),
        "yb" => (1000, 8),
        "kib" => (1024, 1),
        "mib" => (1024, 2),
        "gib" => (1024, 3),
        "tib" => (1024, 4),
        "pib" => (1024, 5),
        "eib" => (1024, 6),
        "zib" => (1024, 7),
        "yib" => (1024, 8),
        _ => return None,
    };
    Some(u128::pow(base, exponent))
}
