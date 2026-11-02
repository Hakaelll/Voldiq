use super::QueryError;

pub(super) struct ByteThreshold {
    pub whole: u64,
    pub fractional: bool,
}

pub(super) fn bytes(value: &str, offset: usize) -> Result<ByteThreshold, QueryError> {
    let error = || {
        QueryError::new(
            offset,
            "Use a nonnegative byte size with at most 9 decimal places, such as 1.5GiB or 2GB, within u64 range.",
        )
    };
    let end = value
        .find(|ch: char| !ch.is_ascii_digit() && ch != '.')
        .unwrap_or(value.len());
    let number = &value[..end];
    let multiplier: u128 = match value[end..].trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "kb" => 1000,
        "mb" => 1000_u128.pow(2),
        "gb" => 1000_u128.pow(3),
        "tb" => 1000_u128.pow(4),
        "pb" => 1000_u128.pow(5),
        "eb" => 1000_u128.pow(6),
        "kib" => 1 << 10,
        "mib" => 1 << 20,
        "gib" => 1 << 30,
        "tib" => 1 << 40,
        "pib" => 1 << 50,
        "eib" => 1 << 60,
        _ => return Err(error()),
    };
    let (whole, fraction) = number
        .split_once('.')
        .map_or((number, None), |(w, f)| (w, Some(f)));
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return Err(error());
    }
    let (scale, fraction) = if let Some(fraction) = fraction {
        if fraction.is_empty()
            || fraction.len() > 9
            || !fraction.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(error());
        }
        (
            10_u128.pow(fraction.len() as u32),
            fraction.parse::<u128>().map_err(|_| error())?,
        )
    } else {
        (1, 0)
    };
    let numerator = whole
        .parse::<u128>()
        .map_err(|_| error())?
        .checked_mul(scale)
        .and_then(|v| v.checked_add(fraction))
        .and_then(|v| v.checked_mul(multiplier))
        .ok_or_else(error)?;
    if numerator > u128::from(u64::MAX) * scale {
        return Err(error());
    }
    Ok(ByteThreshold {
        whole: (numerator / scale) as u64,
        fractional: numerator % scale != 0,
    })
}

pub(crate) struct DateValue {
    pub start: u64,
    pub end: Option<u64>,
}

pub(crate) fn date(value: &str, offset: usize) -> Result<DateValue, QueryError> {
    let error = || {
        QueryError::new(
            offset,
            "Use a valid UTC date YYYY-MM-DD or YYYY-MM-DDTHH:MM:SSZ, from 1970 through 9999.",
        )
    };
    if !value.is_ascii()
        || !matches!(value.len(), 10 | 20)
        || &value[4..5] != "-"
        || &value[7..8] != "-"
    {
        return Err(error());
    }
    let digits = |s: &str| -> Result<u64, QueryError> {
        if !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(error());
        }
        s.parse().map_err(|_| error())
    };
    let year = digits(&value[..4])?;
    let month = digits(&value[5..7])?;
    let day = digits(&value[8..10])?;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let lengths = [
        31,
        28 + u64::from(leap),
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1970..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || day == 0
        || day > lengths[(month - 1) as usize]
    {
        return Err(error());
    }
    let leap_days = |y: u64| y / 4 - y / 100 + y / 400;
    let days = (year - 1970) * 365 + leap_days(year - 1) - leap_days(1969)
        + lengths[..(month - 1) as usize].iter().sum::<u64>()
        + day
        - 1;
    let mut start = days * 86400;
    let end = if value.len() == 10 {
        Some(start + 86400)
    } else {
        if &value[10..11] != "T"
            || &value[13..14] != ":"
            || &value[16..17] != ":"
            || &value[19..20] != "Z"
        {
            return Err(error());
        }
        let hour = digits(&value[11..13])?;
        let minute = digits(&value[14..16])?;
        let second = digits(&value[17..19])?;
        if hour > 23 || minute > 59 || second > 59 {
            return Err(error());
        }
        start += hour * 3600 + minute * 60 + second;
        None
    };
    Ok(DateValue { start, end })
}
